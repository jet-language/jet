//! Execution of canonical optimized MIR.
//!
//! This module is deliberately a machine over `MirProgram`.  It does not lower
//! source syntax, inspect TIR, or rediscover checker policy.  The MIR rows are
//! the executable contract; this tier only marshals values and selects the
//! canonical Core/Prelude dispatcher named by each row.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
#[cfg(unix)]
use std::ffi::{c_char, c_int, c_void, CString};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(unix)]
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}

use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::Game::JetGameReplay;
use jet_foundation::MatchScan::{
    jet_binary_pattern_match, jet_text_pattern_match, JetBinMatchPart, JetPatternCapture,
    JetTextHoleKind, JetTextMatchPart,
};
use jet_foundation::Numeric::CtBigInt;
use jet_foundation::Shape::ShapeProjectionKind;
use jet_foundation::TestingHistory::HistoryProvenance;
use jet_foundation::AST::{
    ClosureData, CtFloat, CtKey, CtOpaque, CtReport, CtValue, Lambda, LambdaBody, LambdaMeta, Type,
};
use jet_foundation::MIR::{
    MirAccess, MirArtifactId, MirArtifactTarget, MirBinaryDispatch, MirBinaryOp,
    MirBinaryPatternPart, MirBlockId, MirCallee, MirCaptureFacts, MirCaptureOperand, MirCliDefault,
    MirCliEntry, MirCliInput, MirCliInputShape, MirCliValueKind, MirConstKey, MirConstReport,
    MirConstant, MirConversion, MirCoreCallId, MirCoreClosureKind, MirDataPlan, MirDropKind,
    MirEntryKind, MirEntryOutput, MirEnumArg, MirExecutionIdentity, MirFailureCarrier, MirFieldId,
    MirFrameIdentity, MirFunction, MirFunctionForm, MirFunctionId, MirHandleToken, MirHardwareOp,
    MirHardwareSetup, MirHarnessId, MirIndexKind, MirInstruction, MirJobId, MirLinkUnitId,
    MirLoopSourceKind, MirOperation, MirOwnershipMode, MirPanicContext, MirPanicLoc, MirPlaceBase,
    MirPlaceId, MirPreludeCall, MirPreludeCallId, MirProjection, MirRequireKind, MirScopeId,
    MirScopeKind, MirSemanticOp, MirSerdeCodec, MirSourceFileId, MirStringPart, MirTagMarker,
    MirInternalTag, MirTaskGroupKind, MirTerminator, MirTestScopeMember, MirTextHoleKind,
    MirTextPatternPart, MirTraitMethodId, MirTraitRef, MirType, MirTypeDefKind, MirTypeKind,
    MirUnaryOp, MirValueId, MirVariantPayload,
};
#[allow(dead_code, unused_imports)]
mod mir_ui_kernel {
    use jet_foundation::Devtools::{JetDevtoolsEnvelope, JetDevtoolsViewState};
    use jet_foundation::Outcome::{JetAbsent, JetOutcome};

    use crate::Codegen::{
        JET_CANONICAL_ARABIC_FONT_BYTES, JET_CANONICAL_FONT_BYTES, JET_CANONICAL_SYMBOLS_FONT_BYTES,
    };

    pub trait JetShow {
        fn jet_show(&self) -> String;
    }

    mod jet_std {
        pub fn jet_reactive_effect_rooted<F: Fn() + Send + Sync + 'static>(body: F) {
            body();
        }
    }

    pub mod jet_tui_kernel {
        include!("../../../jet-foundation/src/Prelude/TuiKernel.rs");
    }

    include!("../Prelude/Core/HostServices.rs");
    include!("../Prelude/Ui.rs");
}
#[allow(dead_code)]
mod mir_human_output_semantics {
    include!("../Prelude/Core/HumanOutput.rs");
}

#[allow(dead_code)]
mod mir_measurement_prelude {
    include!("../Prelude/Core/Measurement.rs");
}
enum MirUiBackend {
    Null(mir_ui_kernel::JetNullBackend),
    Tui(mir_ui_kernel::JetTuiBackend),
}

fn mir_ui_backend_value(backend: MirUiBackend) -> CtValue {
    CtValue::Closure(Arc::new(ClosureData {
        lambda: Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            result_type: None,
            error_type: None,
            effects: None,
            body: LambdaBody::Block(Vec::new()),
            span: Span::new(0, 0),
            meta: LambdaMeta::default(),
        },
        captured: HashMap::new(),
        return_type: None,
        opaque: Some(CtOpaque::new(backend)),
    }))
}

fn mir_ui_field<'a>(value: &'a CtValue, name: &str) -> Option<&'a CtValue> {
    match value {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value)),
        _ => None,
    }
}

fn mir_ui_float(value: &CtValue, span: Span) -> Result<f64, Diagnostic> {
    match value {
        CtValue::Float(CtFloat::F32(value)) => Ok(f64::from(*value)),
        CtValue::Float(CtFloat::F64(value)) => Ok(*value),
        _ => Err(mir_error_at("MIR UI value requires Float", span)),
    }
}

fn mir_ui_present(value: Option<&CtValue>) -> Option<&CtValue> {
    match value {
        Some(CtValue::Present(value)) => Some(value.as_ref()),
        _ => None,
    }
}

fn mir_ui_node(value: &CtValue, span: Span) -> Result<mir_ui_kernel::JetUiNode, Diagnostic> {
    let CtValue::Struct { type_name, .. } = value else {
        return Err(mir_error_at("MIR UI backend expects a UiNode", span));
    };
    if type_name != "UiNode" {
        return Err(mir_error_at("MIR UI backend expects a UiNode", span));
    }
    let label = match mir_ui_field(value, "label") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(mir_error_at("MIR UiNode has no String label", span)),
    };
    let width = mir_ui_field(value, "width")
        .ok_or_else(|| mir_error_at("MIR UiNode has no width", span))
        .and_then(|value| mir_ui_float(value, span))?;
    let height = mir_ui_field(value, "height")
        .ok_or_else(|| mir_error_at("MIR UiNode has no height", span))
        .and_then(|value| mir_ui_float(value, span))?;
    let role = match mir_ui_present(mir_ui_field(value, "role")) {
        Some(CtValue::Enum { variant, .. }) => match variant.as_str() {
            "Button" => Some(mir_ui_kernel::JetAriaRole::Button),
            "TextInput" => Some(mir_ui_kernel::JetAriaRole::TextInput),
            "Label" => Some(mir_ui_kernel::JetAriaRole::Label),
            "Container" => Some(mir_ui_kernel::JetAriaRole::Container),
            _ => None,
        },
        _ => None,
    };
    let color = mir_ui_present(mir_ui_field(value, "color")).and_then(|value| match value {
        CtValue::Str(value) => Some(value.clone()),
        _ => None,
    });
    let kind = match mir_ui_field(value, "kind") {
        Some(CtValue::Enum { variant, .. }) => match variant.as_str() {
            "Text" => mir_ui_kernel::JetUiNodeKind::Text,
            "Box" => mir_ui_kernel::JetUiNodeKind::Box,
            "Button" => mir_ui_kernel::JetUiNodeKind::Button,
            "TextInput" => mir_ui_kernel::JetUiNodeKind::TextInput,
            _ => mir_ui_kernel::JetUiNodeKind::Custom,
        },
        _ => mir_ui_kernel::JetUiNodeKind::Custom,
    };
    let children = match mir_ui_field(value, "children") {
        Some(CtValue::List(values)) => values
            .iter()
            .map(|value| mir_ui_node(value, span))
            .collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    Ok(mir_ui_kernel::JetUiNode {
        label,
        width,
        height,
        role,
        accessibility: None,
        ime: None,
        color,
        style: None,
        kind,
        children,
        key: None,
        on_click: None,
        on_drop: None,
        shortcut: None,
    })
}

fn mir_ui_constraint(
    value: &CtValue,
    span: Span,
) -> Result<mir_ui_kernel::JetSizeConstraint, Diagnostic> {
    Ok(mir_ui_kernel::JetSizeConstraint {
        min_width: mir_ui_float(
            mir_ui_field(value, "min_width")
                .ok_or_else(|| mir_error_at("MIR UI constraint has no min_width", span))?,
            span,
        )?,
        min_height: mir_ui_float(
            mir_ui_field(value, "min_height")
                .ok_or_else(|| mir_error_at("MIR UI constraint has no min_height", span))?,
            span,
        )?,
        max_width: mir_ui_float(
            mir_ui_field(value, "max_width")
                .ok_or_else(|| mir_error_at("MIR UI constraint has no max_width", span))?,
            span,
        )?,
        max_height: mir_ui_float(
            mir_ui_field(value, "max_height")
                .ok_or_else(|| mir_error_at("MIR UI constraint has no max_height", span))?,
            span,
        )?,
    })
}

fn mir_ui_rect(value: &CtValue, span: Span) -> Result<mir_ui_kernel::JetRect, Diagnostic> {
    Ok(mir_ui_kernel::JetRect {
        x: mir_ui_float(
            mir_ui_field(value, "x").ok_or_else(|| mir_error_at("MIR UI rect has no x", span))?,
            span,
        )?,
        y: mir_ui_float(
            mir_ui_field(value, "y").ok_or_else(|| mir_error_at("MIR UI rect has no y", span))?,
            span,
        )?,
        width: mir_ui_float(
            mir_ui_field(value, "width")
                .ok_or_else(|| mir_error_at("MIR UI rect has no width", span))?,
            span,
        )?,
        height: mir_ui_float(
            mir_ui_field(value, "height")
                .ok_or_else(|| mir_error_at("MIR UI rect has no height", span))?,
            span,
        )?,
    })
}

fn mir_ui_size(value: mir_ui_kernel::JetSize) -> CtValue {
    CtValue::Struct {
        type_name: "Size".to_string(),
        fields: vec![
            (
                "width".to_string(),
                CtValue::Float(CtFloat::f64(value.width)),
            ),
            (
                "height".to_string(),
                CtValue::Float(CtFloat::f64(value.height)),
            ),
        ],
    }
}

fn mir_ui_event(value: &CtValue, span: Span) -> Result<mir_ui_kernel::JetInputEvent, Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(mir_error_at("MIR UI backend expects an InputEvent", span));
    };
    if type_name != "InputEvent" {
        return Err(mir_error_at("MIR UI backend expects an InputEvent", span));
    }
    match variant.as_str() {
        "Key" => {
            let code = args
                .iter()
                .find_map(|(name, value)| (name.as_deref() == Some("code")).then_some(value))
                .and_then(|value| match value {
                    CtValue::Str(value) => Some(value.clone()),
                    _ => None,
                })
                .ok_or_else(|| mir_error_at("MIR InputEvent.Key has no code", span))?;
            Ok(mir_ui_kernel::JetInputEvent::Key { code })
        }
        "Resize" => {
            let size = args
                .iter()
                .find_map(|(name, value)| (name.as_deref() == Some("size")).then_some(value))
                .ok_or_else(|| mir_error_at("MIR InputEvent.Resize has no size", span))?;
            let size = mir_ui_rect(
                &CtValue::Struct {
                    type_name: "Rect".to_string(),
                    fields: vec![
                        ("x".to_string(), CtValue::Float(CtFloat::f64(0.0))),
                        ("y".to_string(), CtValue::Float(CtFloat::f64(0.0))),
                        (
                            "width".to_string(),
                            mir_ui_field(size, "width")
                                .cloned()
                                .ok_or_else(|| mir_error_at("MIR Size has no width", span))?,
                        ),
                        (
                            "height".to_string(),
                            mir_ui_field(size, "height")
                                .cloned()
                                .ok_or_else(|| mir_error_at("MIR Size has no height", span))?,
                        ),
                    ],
                },
                span,
            )?;
            Ok(mir_ui_kernel::JetInputEvent::Resize {
                size: mir_ui_kernel::JetSize {
                    width: size.width,
                    height: size.height,
                },
            })
        }
        _ => Err(mir_error_at(
            "MIR UI backend received an unknown InputEvent",
            span,
        )),
    }
}

fn mir_ui_event_result(value: mir_ui_kernel::JetEventResult) -> CtValue {
    CtValue::Enum {
        type_name: "EventResult".to_string(),
        variant: match value {
            mir_ui_kernel::JetEventResult::Handled => "Handled",
            mir_ui_kernel::JetEventResult::Ignored => "Ignored",
        }
        .to_string(),
        args: Vec::new(),
    }
}

fn mir_ui_backend_method(
    receiver: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let CtValue::Closure(data) = receiver else {
        return None;
    };
    let opaque = data.opaque.as_ref()?;
    let backend = opaque.downcast_ref::<MirUiBackend>()?;
    Some((|| match method {
        "measure" => {
            let [node, constraint] = args else {
                return Err(mir_error_at(
                    "MIR UI measure expects node and constraint",
                    span,
                ));
            };
            let node = mir_ui_node(node, span)?;
            let constraint = mir_ui_constraint(constraint, span)?;
            let size = match backend {
                MirUiBackend::Null(backend) => backend.measure_node(node, constraint),
                MirUiBackend::Tui(backend) => backend.measure_node(node, constraint),
            };
            Ok(mir_ui_size(size))
        }
        "layout" => {
            let [node, frame] = args else {
                return Err(mir_error_at("MIR UI layout expects node and frame", span));
            };
            let node = mir_ui_node(node, span)?;
            let frame = mir_ui_rect(frame, span)?;
            match backend {
                MirUiBackend::Null(backend) => backend.layout_node(node, frame),
                MirUiBackend::Tui(backend) => backend.layout_node(node, frame),
            }
            Ok(CtValue::Unit)
        }
        "paint" => {
            let [node] = args else {
                return Err(mir_error_at("MIR UI paint expects one node", span));
            };
            let node = mir_ui_node(node, span)?;
            match backend {
                MirUiBackend::Null(backend) => backend.paint_node(node),
                MirUiBackend::Tui(backend) => backend.paint_node(node),
            }
            Ok(CtValue::Unit)
        }
        "mount" => {
            let ([node] | [node, _]) = args else {
                return Err(mir_error_at("MIR UI mount expects one node", span));
            };
            let node = mir_ui_node(node, span)?;
            match (backend, args) {
                (MirUiBackend::Null(backend), [_, constraint]) => {
                    backend.mount_node(node, mir_ui_constraint(constraint, span)?)
                }
                (MirUiBackend::Tui(backend), [_, constraint]) => {
                    backend.mount_node(node, mir_ui_constraint(constraint, span)?)
                }
                (MirUiBackend::Null(backend), [_]) => backend.mount_node_default(node),
                (MirUiBackend::Tui(backend), [_]) => backend.mount_node_default(node),
                _ => unreachable!(),
            }
            Ok(CtValue::Unit)
        }
        "mount_default" => {
            let [node] = args else {
                return Err(mir_error_at("MIR UI mount_default expects one node", span));
            };
            let node = mir_ui_node(node, span)?;
            match backend {
                MirUiBackend::Null(backend) => backend.mount_node_default(node),
                MirUiBackend::Tui(backend) => backend.mount_node_default(node),
            }
            Ok(CtValue::Unit)
        }
        "on_event" => {
            let [event] = args else {
                return Err(mir_error_at("MIR UI on_event expects one event", span));
            };
            let event = mir_ui_event(event, span)?;
            let result = match backend {
                MirUiBackend::Null(backend) => backend.dispatch_event(event),
                MirUiBackend::Tui(backend) => backend.dispatch_event(event),
            };
            Ok(mir_ui_event_result(result))
        }
        "commands" => match backend {
            MirUiBackend::Null(backend) => Ok(CtValue::List(
                backend
                    .paint_commands()
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            )),
            MirUiBackend::Tui(_) => Err(mir_error_at("MIR TUI backend has no commands", span)),
        },
        "frame_lines" => match backend {
            MirUiBackend::Tui(backend) => Ok(CtValue::List(
                backend
                    .frame_lines()
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            )),
            MirUiBackend::Null(_) => Err(mir_error_at("MIR null backend has no frame lines", span)),
        },
        "render_count" => match backend {
            MirUiBackend::Tui(backend) => Ok(CtValue::Int(backend.render_count())),
            MirUiBackend::Null(_) => {
                Err(mir_error_at("MIR null backend has no render count", span))
            }
        },
        "focused_label" => {
            let value = match backend {
                MirUiBackend::Null(backend) => backend.focused_label(),
                MirUiBackend::Tui(backend) => backend.focused_label(),
            };
            Ok(CtValue::Str(value))
        }
        _ => Err(mir_error_at("MIR UI backend method is unsupported", span)),
    })())
}

fn history_provenance_for_execution(
    program: &jet_foundation::MIR::MirProgram,
    execution: Option<&MirExecutionIdentity>,
) -> Option<HistoryProvenance> {
    let artifact = &execution?.artifact;
    Some(HistoryProvenance {
        source: artifact.identity_digest(),
        tool: format!(
            "{}:{}",
            jet_foundation::TestingHistory::HISTORY_ENGINE,
            program.facts.target_dossier.compiler_identity
        ),
        target: jet_foundation::SHA256::sha256_hex(
            &program.facts.target_dossier.cache_bytes(
                program
                    .facts
                    .target_dossier
                    .machine
                    .as_deref()
                    .map(|machine| machine.triple.as_str())
                    .unwrap_or_default(),
            ),
        ),
    })
}

const MIR_DETERMINISTIC_WORLD_HANDLE: jet_foundation::MIR::MirHandleId =
    jet_foundation::MIR::MirHandleId(u64::MAX);

thread_local! {
    static MIR_DETERMINISTIC_WORLDS:
        RefCell<BTreeMap<i64, crate::scheduler::JetDeterministicWorld>> =
        const { RefCell::new(BTreeMap::new()) };
    static MIR_NEXT_DETERMINISTIC_WORLD_ID: Cell<i64> = const { Cell::new(1) };
    static MIR_HTTP_HANDLERS: RefCell<BTreeMap<i64, RuntimeValue>> =
        const { RefCell::new(BTreeMap::new()) };
    static MIR_NEXT_HTTP_HANDLER: Cell<i64> = const { Cell::new(1) };
    static MIR_HTTP_LISTENERS: RefCell<BTreeMap<i64, ()>> =
        const { RefCell::new(BTreeMap::new()) };
}

fn register_deterministic_world(world: crate::scheduler::JetDeterministicWorld) -> i64 {
    let raw = MIR_NEXT_DETERMINISTIC_WORLD_ID.with(|next| {
        let raw = next.get();
        next.set(raw.saturating_add(1).max(1));
        raw
    });
    MIR_DETERMINISTIC_WORLDS.with(|worlds| {
        worlds.borrow_mut().insert(raw, world);
    });
    raw
}

fn with_deterministic_world<R>(
    raw: i64,
    callback: impl FnOnce(&crate::scheduler::JetDeterministicWorld) -> R,
) -> Option<R> {
    MIR_DETERMINISTIC_WORLDS.with(|worlds| worlds.borrow().get(&raw).map(callback))
}

fn unregister_deterministic_world(raw: i64) {
    MIR_DETERMINISTIC_WORLDS.with(|worlds| {
        worlds.borrow_mut().remove(&raw);
    });
}

#[allow(dead_code)]
#[allow(dead_code, unused_imports)]
mod fixed_float_reduction_prelude {
    include!("../Prelude/Core/SimdLanes.rs");
    include!("../Prelude/Core/ParallelKernel.rs");
}

mod mir_sort_prelude {
    include!("../Prelude/Core/SortKernel.rs");
}

// Shared Prelude protocol source is compiled by multiple hosts; this adapter
// uses only a subset, so scope dead-code allowance to this module.
#[allow(dead_code)]
mod shared_protocol {
    include!("../Prelude/SharedProtocol.rs");
}
#[allow(dead_code)]
mod mir_typed_text_prelude {
    include!("../Prelude/TypedText.rs");
}
#[allow(dead_code)]
mod mir_csv_prelude {
    use jet_foundation::Outcome::*;
    include!("../Prelude/CoreLib/JetStd/EncodingTypes.rs");
    include!("../Prelude/Core/FieldError.rs");
}
#[allow(dead_code)]
mod mir_time_prelude {
    // The interpreter owns the Scheduler scope used by testing.world. Outside
    // that scope the provider returns None, preserving the explicit ambient
    // fallback; inside it, civil time reads the controlled world origin.
    fn jet_scheduler_world_now_ms() -> Option<i64> {
        crate::scheduler::jet_scheduler_world_now_ms()
    }
    include!("../Prelude/Core/Duration.rs");
    include!("../Prelude/Core/Time.rs");

    pub(crate) fn datetime_parts(value: &str) -> Result<(i64, i64, bool), String> {
        let value = value.to_string();
        let datetime = jet_time_parse_rfc3339(&value)?;
        Ok((
            datetime.unix_seconds_anchor(),
            i64::from(datetime.nanosecond()),
            datetime.is_leap_second(),
        ))
    }
}

#[allow(dead_code, unused_imports)]
mod mir_args_prelude {
    use super::{
        MirCliDefault, MirCliEntry, MirCliInput, MirCliInputShape, MirCliValueKind, MirConstant,
        MirEvalValue, MirType,
    };

    pub use jet_foundation::Outcome::*;

    trait JetShow {
        fn jet_show(&self) -> String;
    }

    fn jet_args_program_name(prog: &str) -> String {
        if prog.is_empty() {
            return "program".to_string();
        }
        std::path::Path::new(prog)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(prog)
            .to_string()
    }

    fn jet_args_source_program_name(prog: &str) -> String {
        let (program, suffix) = prog.split_once(' ').unwrap_or((prog, ""));
        let name = jet_args_program_name(program);
        let source = name.strip_suffix(".jet").unwrap_or(&name);
        if suffix.is_empty() {
            source.to_string()
        } else {
            format!("{source} {suffix}")
        }
    }

    include!("../Prelude/CoreLib/Top/Args.rs");
    include!("../Prelude/Core/ArgsProjectionCore.rs");

    fn cli_constant_text(value: &MirConstant) -> String {
        match value {
            MirConstant::Int { value, .. } => value.to_string(),
            MirConstant::Float { value, .. } => format!("{value:?}"),
            MirConstant::Bool(value) => value.to_string(),
            MirConstant::Char(value) => value.to_string(),
            MirConstant::String(value) => value.clone(),
            MirConstant::BigInt(value) => value.clone(),
            MirConstant::Bytes(value) => String::from_utf8_lossy(value).into_owned(),
            MirConstant::Unit => "()".to_string(),
            _ => format!("{value:?}"),
        }
    }

    fn cli_default_text(default: &Option<MirCliDefault>, ty: &MirType) -> Option<String> {
        match default {
            None => None,
            Some(MirCliDefault::TypeDefault) => Some(if ty.is_bool() {
                "false".to_string()
            } else if ty.is_integer() {
                "0".to_string()
            } else if ty.is_float() {
                "0.0".to_string()
            } else if ty.is_string() {
                String::new()
            } else {
                "()".to_string()
            }),
            Some(MirCliDefault::Value(value)) => Some(cli_constant_text(value)),
        }
    }

    fn cli_spec(entry: &MirCliEntry, program: &str) -> JetArgsSpec {
        fn add_inputs(mut spec: JetArgsSpec, inputs: &[MirCliInput]) -> JetArgsSpec {
            for input in inputs {
                match &input.shape {
                    MirCliInputShape::Flag => {
                        spec = match &input.short {
                            Some(short) => {
                                jet_args_flag_short(spec, &input.name, short, &input.help)
                            }
                            None => jet_args_flag(spec, &input.name, &input.help),
                        };
                    }
                    MirCliInputShape::Value {
                        kind,
                        optional,
                        default,
                    } => {
                        let metavar = input.metavar.clone().unwrap_or_else(|| "VALUE".to_string());
                        let default = cli_default_text(default, &input.ty);
                        let required = !optional && default.is_none() && input.positional.is_none();
                        let value = match kind {
                            MirCliValueKind::Bool => JetArgValueKind::String,
                            MirCliValueKind::Int => JetArgValueKind::Int,
                            MirCliValueKind::Float => JetArgValueKind::Float,
                            MirCliValueKind::String | MirCliValueKind::Path => {
                                JetArgValueKind::String
                            }
                        };
                        spec = jet_args_option_base(
                            spec,
                            &input.name,
                            input.short.clone(),
                            &input.help,
                            &metavar,
                            default,
                            input.env.clone(),
                            required,
                            input.variadic,
                            value,
                        );
                        if input.positional.is_some() {
                            spec = jet_args_positional(spec, &input.name, &input.help);
                        }
                    }
                }
            }
            spec
        }

        let mut spec = jet_args_program(jet_args_spec(), &jet_args_source_program_name(program));
        if let Some(description) = &entry.description {
            spec = jet_args_description(spec, description);
        }
        spec = add_inputs(spec, &entry.inputs);
        if entry.standard {
            spec = jet_args_flag_short(
                spec,
                &"verbose".to_string(),
                &"v".to_string(),
                &"print extra detail".to_string(),
            );
            spec = jet_args_flag_short(
                spec,
                &"quiet".to_string(),
                &"q".to_string(),
                &"suppress normal output".to_string(),
            );
            spec = jet_args_option_choice(
                spec,
                &"color".to_string(),
                &"control terminal color".to_string(),
                &"MODE".to_string(),
                &"auto,always,never".to_string(),
            );
            if let Some(version) = &entry.version {
                spec = jet_args_version(spec, version);
            }
        }
        for command in &entry.commands {
            let mut command_spec =
                jet_args_program(jet_args_spec(), &jet_args_source_program_name(program));
            if let Some(description) = &command.description {
                command_spec = jet_args_description(command_spec, description);
            }
            command_spec = add_inputs(command_spec, &command.inputs);
            spec = jet_args_subcommand(
                spec,
                &command.name,
                &command.description.clone().unwrap_or_default(),
                command_spec,
            );
        }
        spec
    }

    pub(crate) fn shape(
        entry: &MirCliEntry,
        program: &str,
        argv: &[String],
        names: &[(String, String)],
    ) -> Result<Vec<(String, JetArgsShapeValue)>, Vec<JetArgsShapeError>> {
        let spec = cli_spec(entry, program);
        let projection = names
            .iter()
            .map(|(source, target)| (source.as_str(), target.as_str()))
            .collect::<Vec<_>>();
        let argv = argv.to_vec();
        let parsed = jet_args_parse(&spec, &argv).map_err(|reason| {
            vec![JetArgsShapeError {
                path: String::new(),
                reason,
            }]
        })?;
        jet_args_shape_tree_values(&spec, &parsed, &projection)
    }

    pub(crate) fn merge(
        entry: &MirCliEntry,
        program: &str,
        argv: &[String],
        names: &[(String, String)],
        flags: Vec<(String, MirEvalValue)>,
        settings: Vec<(String, MirEvalValue)>,
    ) -> Result<Vec<(String, MirEvalValue)>, Vec<JetArgsShapeError>> {
        let spec = cli_spec(entry, program);
        let projection = names
            .iter()
            .map(|(source, target)| (source.as_str(), target.as_str()))
            .collect::<Vec<_>>();
        let argv = argv.to_vec();
        jet_args_merge_fields(&spec, &argv, &projection, flags, settings)
    }
}

#[allow(dead_code, unused_imports)]
mod mir_env_prelude {
    use jet_foundation::Shape::{ShapeProjection, ShapeProjectionKind};

    include!("../Prelude/Core/EnvConfig.rs");
    include!("../Prelude/Core/EnvProjection.rs");

    pub(crate) fn entries(
        prefix: &str,
        file: &str,
        allow: &[String],
        names: &[(String, String)],
    ) -> Result<Vec<JetEnvConfigEntry>, String> {
        if !jet_env_config_file_is_project_relative(file) {
            return Err("E2416: Dotenv.file must be project-relative".to_string());
        }
        let dotenv = if file.is_empty() {
            None
        } else {
            match std::fs::read_to_string(file) {
                Ok(text) => Some(text),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(format!("E2416: cannot read Dotenv.file `{file}`: {error}"));
                }
            }
        };
        let projection = names
            .iter()
            .map(|(source_name, decode_name)| (source_name.as_str(), decode_name.as_str()))
            .collect::<Vec<_>>();
        jet_env_config_entries_for_names(
            prefix,
            dotenv.as_deref(),
            allow,
            projection,
            std::env::vars(),
        )
    }
}

#[allow(dead_code)]
mod mir_atomic_prelude {
    // Exact `Int` and fixed-width `I64` use the same i64 ABI but distinct
    // marker types. Exact values use the Foundation's owned immutable nodes;
    // no bounded machine carrier or tier-local numeric store is involved.

    pub(crate) mod jet_std {
        use jet_foundation::Numeric::JetInt;
        use jet_foundation::Outcome::AllocError;

        pub(crate) fn jet_int_from_i64(value: i64) -> JetInt {
            JetInt::from_i64(value)
        }

        pub(crate) fn jet_int_from_str(value: &str) -> Result<JetInt, String> {
            jet_foundation::Numeric::CtBigInt::from_str(value).map(JetInt::from_big)
        }

        pub(crate) fn jet_int_to_string(value: &JetInt) -> String {
            value.to_string_rep()
        }

        pub(crate) fn jet_int_compare(left: &JetInt, right: &JetInt) -> i64 {
            match left.compare(right) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        }

        pub(crate) fn jet_int_add(left: &JetInt, right: &JetInt) -> Result<JetInt, AllocError> {
            left.add(right)
        }

        pub(crate) fn jet_int_try_new(value: JetInt) -> Result<JetInt, AllocError> {
            Ok(value)
        }

        pub(crate) fn jet_int_try_add(left: &JetInt, right: &JetInt) -> Result<JetInt, AllocError> {
            left.try_add(right)
        }
    }

    include!("../Prelude/Core/Atomic.rs");
}
/// The interpreter and resident JIT use this module as their canonical game
/// development protocol.  The transition/effect implementation is the exact
/// Prelude source emitted by AOT; the host tiers only construct sessions and
/// marshal their callback/relay values into it.
#[allow(dead_code, private_interfaces, unused_imports)]
pub mod game_dev_protocol {

    use jet_foundation::Devtools::*;
    use jet_foundation::DevtoolsControl::*;
    use jet_foundation::Game::*;

    /// Adapter-local rendering uses the canonical Foundation DataTree directly.
    mod jet_std {
        pub(crate) use crate::DataTree::DataTree;
        pub(crate) use jet_foundation::JSON::parse_json as parse_json_strict;
        pub fn render_json(value: &DataTree, _pretty: bool, _depth: usize) -> String {
            match value {
                DataTree::Null => "null".to_string(),
                DataTree::Bool(value) => value.to_string(),
                DataTree::Int(value) => value.to_string(),
                DataTree::Float(value) => format!("{value:?}"),
                DataTree::Number(value) => value.clone(),
                DataTree::TypedText(value) | DataTree::Text(value) => quote(value),
                DataTree::Array(values) => format!(
                    "[{}]",
                    values
                        .iter()
                        .map(|value| render_json(value, false, 0))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                DataTree::Object(entries) => format!(
                    "{{{}}}",
                    entries
                        .iter()
                        .map(|(key, value)| {
                            format!("{}:{}", quote(key), render_json(value, false, 0))
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                DataTree::Bytes(_) => {
                    unreachable!("game protocol JSON values cannot contain byte nodes")
                }
            }
        }

        fn quote(value: &str) -> String {
            let mut out = String::with_capacity(value.len() + 2);
            out.push('"');
            for character in value.chars() {
                match character {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\u{08}' => out.push_str("\\b"),
                    '\u{0c}' => out.push_str("\\f"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    character if character.is_control() => {
                        out.push_str(&format!("\\u{:04x}", character as u32));
                    }
                    character => out.push(character),
                }
            }
            out.push('"');
            out
        }
    }

    include!("../Prelude/Core/GameAssetPipeline.rs");
    include!("../Prelude/Core/GameFrameProfiler.rs");
    include!("../Prelude/Core/GameHotSwap.rs");
    include!("../Prelude/Core/GameWorldInspector.rs");
    include!("../Prelude/Core/GameOverlay.rs");
    include!("../Prelude/CoreLib/Top/GameAssetsImport.rs");
    include!("../Prelude/CoreLib/Top/GameAssetsRuntime.rs");
    include!("../Prelude/CoreLib/Top/GameDevProtocol.rs");

    pub fn new_session_with_policy(
        scene_name: &str,
        target: &str,
        debug_data_enabled: bool,
    ) -> Result<GameDevSession, String> {
        let fallback_source = if scene_name.is_empty() {
            "game"
        } else {
            scene_name
        };
        let session_id = std::env::var("JET_DEVTOOLS_RELAY_SESSION_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("game:{scene_name}"));
        let source = std::env::var("JET_DEVTOOLS_RELAY_SOURCE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| fallback_source.to_string());
        let build = std::env::var("JET_DEVTOOLS_RELAY_BUILD_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "runtime".to_string());
        let revision = std::env::var("JET_DEVTOOLS_RELAY_REVISION")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "runtime".to_string());
        let identity = GameDevRunIdentity::new(session_id, source, build, revision);
        let mut session =
            GameDevSession::new_with_policy(identity, 0, GameDevPolicy::new(debug_data_enabled))
                .map_err(|error| error.to_string())?;
        session.launch_profile.target = target.to_string();
        jet_game_install_devtools_callback();
        Ok(session)
    }

    pub fn game_debug_data_enabled(policy: &jet_pkg_model::Package::ReleaseDevtoolsPolicy) -> bool {
        !policy.is_release()
    }

    pub fn enqueue_control_payload(session_id: &str, payload: &str) -> Result<(), String> {
        let request = jet_game_decode_control_request(session_id, payload)?;
        jet_devtools_enqueue_game_control(request)
    }

    pub fn ingest_command_frame(frame: &str, session_id: &str) -> Result<usize, String> {
        jet_game_ingest_devtools_command_frame(frame, session_id)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MirFloat {
    F32(f32),
    F64(f64),
}

impl MirFloat {
    pub fn literal(value: f64, f32: bool) -> Self {
        if f32 {
            Self::F32(value as f32)
        } else {
            Self::F64(value)
        }
    }

    pub fn f32(value: f32) -> Self {
        Self::F32(value)
    }

    pub fn f64(value: f64) -> Self {
        Self::F64(value)
    }

    pub fn as_f64(self) -> f64 {
        match self {
            Self::F32(value) => value as f64,
            Self::F64(value) => value,
        }
    }

    pub fn as_f32(self) -> f32 {
        match self {
            Self::F32(value) => value,
            Self::F64(value) => value as f32,
        }
    }

    pub fn is_f32(self) -> bool {
        matches!(self, Self::F32(_))
    }

    fn neg(self) -> Self {
        match self {
            Self::F32(value) => Self::F32(-value),
            Self::F64(value) => Self::F64(-value),
        }
    }

    fn binop(self, op: MirBinaryOp, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::F32(left), Self::F32(right)) => Some(Self::F32(match op {
                MirBinaryOp::Add => left + right,
                MirBinaryOp::Sub => left - right,
                MirBinaryOp::Mul => left * right,
                MirBinaryOp::Div => left / right,
                MirBinaryOp::Pow => left.powf(right),
                MirBinaryOp::FloorDiv
                | MirBinaryOp::Mod
                | MirBinaryOp::Rem
                | MirBinaryOp::BitAnd
                | MirBinaryOp::BitOr
                | MirBinaryOp::BitXor
                | MirBinaryOp::Shl
                | MirBinaryOp::Shr
                | MirBinaryOp::Eq
                | MirBinaryOp::Ne
                | MirBinaryOp::Lt
                | MirBinaryOp::Gt
                | MirBinaryOp::Le
                | MirBinaryOp::Ge
                | MirBinaryOp::Compare
                | MirBinaryOp::And
                | MirBinaryOp::Or => return None,
            })),
            (Self::F64(left), Self::F64(right)) => Some(Self::F64(match op {
                MirBinaryOp::Add => left + right,
                MirBinaryOp::Sub => left - right,
                MirBinaryOp::Mul => left * right,
                MirBinaryOp::Div => left / right,
                MirBinaryOp::Pow => left.powf(right),
                MirBinaryOp::FloorDiv
                | MirBinaryOp::Mod
                | MirBinaryOp::Rem
                | MirBinaryOp::BitAnd
                | MirBinaryOp::BitOr
                | MirBinaryOp::BitXor
                | MirBinaryOp::Shl
                | MirBinaryOp::Shr
                | MirBinaryOp::Eq
                | MirBinaryOp::Ne
                | MirBinaryOp::Lt
                | MirBinaryOp::Gt
                | MirBinaryOp::Le
                | MirBinaryOp::Ge
                | MirBinaryOp::Compare
                | MirBinaryOp::And
                | MirBinaryOp::Or => return None,
            })),
            _ => None,
        }
    }

    fn partial_cmp(self, other: Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::F32(left), Self::F32(right)) => left.partial_cmp(&right),
            (Self::F64(left), Self::F64(right)) => left.partial_cmp(&right),
            _ => None,
        }
    }
}
fn mir_float_data(value: MirFloat) -> MirEvalValue {
    MirEvalValue::Float {
        value: value.as_f64(),
        f32: value.is_f32(),
    }
}

fn mir_float_from_data(value: &MirEvalValue) -> Option<MirFloat> {
    match value {
        MirEvalValue::Float { value, f32 } => Some(MirFloat::literal(*value, *f32)),
        _ => None,
    }
}

fn mir_bigint(value: &str, span: Span) -> Result<CtBigInt, Diagnostic> {
    CtBigInt::from_str(value).map_err(|message| mir_error_at(&message, span))
}
fn mir_sql_binding(
    value: &MirEvalValue,
    ty: &MirType,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    let db_value = |variant: &str, payload: Option<MirEvalValue>| MirEvalValue::Enum {
        type_name: crate::Syntax::TYPE_DB_VALUE.to_string(),
        variant: variant.to_string(),
        args: payload.map_or_else(Vec::new, |value| vec![(None, value)]),
    };
    if mir_value_contains_moved(value) {
        return Err(mir_error_at("MIR value was moved", span));
    }
    match value {
        MirEvalValue::Enum { type_name, .. } if type_name == crate::Syntax::TYPE_DB_VALUE => {
            Ok(value.clone())
        }
        MirEvalValue::Int(value) => Ok(db_value("Int", Some(MirEvalValue::Int(*value)))),
        MirEvalValue::Float { value, f32 } => Ok(db_value(
            "Float",
            Some(MirEvalValue::Float {
                value: *value,
                f32: *f32,
            }),
        )),
        MirEvalValue::Bool(value) => Ok(db_value("Bool", Some(MirEvalValue::Bool(*value)))),
        MirEvalValue::String(value) => {
            Ok(db_value("Text", Some(MirEvalValue::String(value.clone()))))
        }
        MirEvalValue::List(values)
            if matches!(
                ty.kind(),
                MirTypeKind::List(inner)
                    if matches!(
                        inner.kind(),
                        MirTypeKind::IntN {
                            signed: false,
                            bits: 8
                        }
                    )
            ) =>
        {
            let bytes = values
                .iter()
                .map(|value| match value {
                    MirEvalValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
                    _ => Err(mir_error_at(
                        "MIR SQL U8 list binding contains a non-byte value",
                        span,
                    )),
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            Ok(db_value("Blob", Some(MirEvalValue::Bytes(bytes))))
        }
        _ => Ok(db_value(
            "Text",
            Some(MirEvalValue::String(mir_show(value))),
        )),
    }
}

/// Canonical constant-key shape shared with MIR constants and all adapters.
pub type MirEvalKey = jet_foundation::MIR::MirConstKey;

/// Canonical target-neutral runtime carrier.  Adapter-owned handles stay in
/// the private `RuntimeValue` below and never cross this boundary.
pub use jet_foundation::MIR::{
    MirRuntimeClosure as MirClosureValue, MirRuntimeValue as MirEvalValue,
};

const MIR_HTTP_HANDLER_TOKEN_TYPE: &str = "__JetHttpHandler";

fn mir_http_handler_token(value: RuntimeValue, span: Span) -> Result<MirEvalValue, Diagnostic> {
    if !matches!(
        &value,
        RuntimeValue::Closure(_) | RuntimeValue::Data(MirEvalValue::Closure(_))
    ) {
        return Err(mir_error_at(
            "MIR HTTP route handler is not a checked closure",
            span,
        ));
    }
    let raw = MIR_NEXT_HTTP_HANDLER.with(|next| {
        let raw = next.get().max(1);
        next.set(raw.saturating_add(1).max(1));
        raw
    });
    MIR_HTTP_HANDLERS.with(|handlers| {
        handlers.borrow_mut().insert(raw, value);
    });
    Ok(MirEvalValue::Struct {
        type_name: MIR_HTTP_HANDLER_TOKEN_TYPE.to_string(),
        fields: vec![("id".to_string(), MirEvalValue::Int(raw))],
    })
}

fn mir_http_handler_value(value: &MirEvalValue) -> Option<RuntimeValue> {
    let MirEvalValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != MIR_HTTP_HANDLER_TOKEN_TYPE {
        return None;
    }
    let raw = fields.iter().find_map(|(name, value)| {
        (name == "id")
            .then_some(value)
            .and_then(|value| match value {
                MirEvalValue::Int(raw) => Some(*raw),
                _ => None,
            })
    })?;
    MIR_HTTP_HANDLERS.with(|handlers| handlers.borrow().get(&raw).cloned())
}

/// Byte order used by the explicit target layout supplied to an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirEndian {
    Little,
    Big,
}

impl Default for MirEndian {
    fn default() -> Self {
        Self::Little
    }
}

/// ABI facts selected by the caller.  The interpreter does not derive these
/// from a host target; it consumes the layout already attached to the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirTargetLayout {
    pub pointer_bits: u16,
    pub endian: MirEndian,
    pub scalar_widths: BTreeMap<String, u16>,
}

impl Default for MirTargetLayout {
    fn default() -> Self {
        Self {
            pointer_bits: 64,
            endian: MirEndian::Little,
            scalar_widths: BTreeMap::new(),
        }
    }
}

/// Capability facts supplied by the host boundary.  MIR effects remain
/// authoritative; these fields only describe which host services can marshal a
/// request when a canonical Core row selects an ambient route.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MirCapabilities {
    pub effects: BTreeSet<String>,
    pub foreign: bool,
    pub unsafe_ops: bool,
}

/// Configuration for one MIR execution.
#[derive(Debug, Clone)]
pub struct MirEvalConfig {
    pub base_dir: PathBuf,
    pub target: MirTargetLayout,
    pub capabilities: MirCapabilities,
    pub runtime_execution: bool,
    pub try_anyway: bool,
    pub globals: BTreeMap<String, MirEvalValue>,
    pub fuel: u64,
    /// The already-folded package release policy for this invocation. Runtime
    /// adapters consume this carrier; they never inspect compiler cfg flags.
    pub release_devtools_policy: jet_pkg_model::Package::ReleaseDevtoolsPolicy,
}

impl Default for MirEvalConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::new(),
            target: MirTargetLayout::default(),
            capabilities: MirCapabilities::default(),
            runtime_execution: false,
            try_anyway: false,
            globals: BTreeMap::new(),
            fuel: 10_000_000,
            release_devtools_policy:
                jet_pkg_model::Package::ReleaseDevtoolsPolicy::from_manifest_profile(
                    jet_pkg_model::Package::ReleaseInspect::None,
                ),
        }
    }
}

/// A value-bearing, serializable view of a paused MIR machine.  Stable MIR IDs
/// are used instead of host addresses or vector positions.
#[derive(Debug, Clone)]
pub struct MirFrameState {
    pub function: MirFunctionId,
    pub block: MirBlockId,
    pub ip: usize,
    pub predecessor: Option<MirBlockId>,
    /// Closure environment values, in canonical capture-parameter slot order.
    pub captures: Vec<MirEvalValue>,
    pub values: BTreeMap<u64, MirEvalValue>,
    pub places: BTreeMap<u64, MirEvalValue>,
    pub locals: BTreeMap<u64, MirEvalValue>,
    pub scopes: Vec<u64>,
    pub call_stack: Vec<MirFrameState>,
    pub identity: Option<MirFrameIdentity>,
}

impl MirFrameState {
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        write_frame_state(&mut out, self);
        out
    }
}

/// A resumable MIR frame.  The runtime snapshot is retained only for
/// in-process suspension; serialized scalar state remains a public boundary.
#[derive(Debug, Clone)]
pub struct MirFrame {
    pub state: MirFrameState,
    runtime: Option<RuntimeSnapshot>,
}

impl MirFrame {
    pub fn new(function: MirFunctionId, block: MirBlockId) -> Self {
        Self {
            state: MirFrameState {
                function,
                block,
                captures: Vec::new(),
                ip: 0,
                predecessor: None,
                values: BTreeMap::new(),
                places: BTreeMap::new(),
                locals: BTreeMap::new(),
                scopes: Vec::new(),
                call_stack: Vec::new(),
                identity: None,
            },
            runtime: None,
        }
    }

    pub fn serialize(&self) -> String {
        self.state.serialize()
    }
}

/// A value accepted by the MIR entry seam. MIR values are already typed by the
/// canonical checker; the interpreter only transports them.
pub type MirValue = MirEvalValue;
/// Result status for one MIR execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirExecutionStatus {
    Completed,
    Suspended,
}

/// The artifact facts selected for a program-level run.  These are copied from
/// the canonical artifact/entry/harness/job rows so the host can honor
/// initialization and service-lifetime requirements without re-discovering
/// package semantics.
#[derive(Debug, Clone)]
pub struct MirArtifactFacts {
    pub id: MirArtifactId,
    pub entry_function: MirFunctionId,
    pub entry_kind: MirEntryKind,
    pub output: MirEntryOutput,
    pub initialize_environment: bool,
    pub initialize_gc: bool,
    pub serves_until_stopped: bool,
    pub harness: Option<MirHarnessId>,
    pub jobs: Vec<MirJobId>,
    pub links: Vec<MirLinkUnitId>,
}

pub struct MirEvalResult {
    pub value: MirValue,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub status: MirExecutionStatus,
    pub frame: Option<MirFrame>,
    pub artifact: Option<MirArtifactFacts>,
}

#[derive(Debug, Clone)]
pub struct MirEvalError {
    pub diagnostic: Diagnostic,
}

impl MirEvalError {
    pub fn into_diagnostic(self) -> Diagnostic {
        self.diagnostic
    }
}

impl From<Diagnostic> for MirEvalError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}
/// Install the one comptime execution bridge. The hook surface is owned by
/// `jet-comptime`; this module supplies only canonical MIR lowering/execution.
pub fn install_mir_bridge() {
    crate::Comptime::MirBridge::install(crate::Comptime::MirBridge::Hooks {
        run_bundle: run_bundle_bridge,
        run_bundle_at_stage: run_bundle_at_stage_bridge,
        eval_expr: eval_expr_bridge,
        eval_block: eval_block_bridge,
    });
}

fn run_bundle_bridge(
    bundle: &crate::AST::ProgramBundle,
    sink: &mut crate::Comptime::DevSink,
    gates: jet_foundation::Policy::GateSet,
) -> Result<CtValue, Diagnostic> {
    run_bundle_at_stage_bridge(bundle, sink, gates, crate::Comptime::PurityStage::BuildTime)
}

fn run_bundle_at_stage_bridge(
    bundle: &crate::AST::ProgramBundle,
    sink: &mut crate::Comptime::DevSink,
    _gates: jet_foundation::Policy::GateSet,
    _stage: crate::Comptime::PurityStage,
) -> Result<CtValue, Diagnostic> {
    let request = jet_foundation::MIR::MirArtifactRequest::new(
        MirArtifactTarget::Interpreter,
        jet_foundation::MIR::MirArtifactKind::NativeExecutable,
        jet_foundation::MIR::MirArtifactBuildMode::Dev,
    );
    let (program, artifact) = crate::Codegen::TIR::lower_checked_mir_program_for(bundle, request)
        .map_err(|error| mir_error_at(&error.message, error.span))?;
    let config = MirEvalConfig {
        base_dir: PathBuf::from(program.facts.project_root.clone()),
        ..MirEvalConfig::default()
    };
    let result = evaluate_mir_program_with_config(&program, artifact, &config)
        .map_err(MirEvalError::into_diagnostic)?;
    sink.stdout.push_str(&result.stdout);
    sink.stderr.push_str(&result.stderr);
    sink.exit_code = Some(result.exit_code);
    crate::Comptime::MirBridge::mir_to_ct_value(result.value, Span::new(0, 0))
}

fn fragment_context<'a>(
    funcs: &'a HashMap<String, &'a crate::AST::Func>,
    binding_types: &'a HashMap<String, crate::AST::Type>,
    error_conversions: &'a [crate::AST::ErrorConvDef],
    method_traits: &'a HashMap<(String, String), String>,
    methods: &'a HashMap<(String, String), &'a crate::AST::Func>,
    extern_names: &'a std::collections::HashSet<String>,
    globals: &'a HashMap<String, CtValue>,
    core_imports: &'a HashMap<String, String>,
    structs: &'a HashMap<String, &'a crate::AST::StructDef>,
    checked_nominals: Option<&'a crate::Comptime::MirBridge::MirFragmentNominalFacts>,
    computed_fields: &'a HashMap<(String, String), &'a crate::AST::Expr>,
    distinct_ranges: &'a HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &'a HashMap<String, crate::AST::Type>,
    unit_families: &'a [crate::AST::UnitFamilyDef],
) -> crate::Codegen::TIR::MirFragmentContext<'a> {
    crate::Codegen::TIR::MirFragmentContext {
        funcs,
        binding_types,
        error_conversions,
        method_traits,
        methods,
        extern_names,
        globals,
        core_imports,
        structs,
        checked_nominals,
        computed_fields,
        distinct_ranges,
        distinct_bases,
        unit_families,
    }
}

fn fragment_config(
    req_fuel: u64,
    base_dir: &std::path::Path,
    globals: &HashMap<String, CtValue>,
    runtime_execution: bool,
) -> Result<MirEvalConfig, Diagnostic> {
    let mut converted = BTreeMap::new();
    for (name, value) in globals {
        converted.insert(
            name.clone(),
            crate::Comptime::MirBridge::ct_to_mir_value(value.clone(), Span::new(0, 0))?,
        );
    }
    Ok(MirEvalConfig {
        base_dir: base_dir.to_path_buf(),
        fuel: req_fuel,
        runtime_execution,
        globals: converted,
        ..MirEvalConfig::default()
    })
}

fn fragment_args(
    names: &[String],
    values: &HashMap<String, CtValue>,
) -> Result<Vec<MirEvalValue>, Diagnostic> {
    names
        .iter()
        .map(|name| {
            let value = values.get(name).cloned().ok_or_else(|| {
                mir_error_at(
                    &format!("MIR fragment binding `{name}` has no runtime value"),
                    Span::new(0, 0),
                )
            })?;
            crate::Comptime::MirBridge::ct_to_mir_value(value, Span::new(0, 0))
        })
        .collect()
}

fn fragment_fields(value: MirEvalValue, span: Span) -> Result<Vec<(String, CtValue)>, Diagnostic> {
    let value = crate::Comptime::MirBridge::mir_to_ct_value(value, span)?;
    match value {
        CtValue::Struct { fields, .. } => Ok(fields),
        _ => Err(mir_error_at(
            "MIR fragment returned a non-aggregate result",
            span,
        )),
    }
}

fn merge_fragment_output(result: &MirEvalResult, sink: Option<&mut crate::Comptime::DevSink>) {
    if let Some(sink) = sink {
        sink.stdout.push_str(&result.stdout);
        sink.stderr.push_str(&result.stderr);
        sink.exit_code = Some(result.exit_code);
    }
}

fn try_eval_build_time_io(
    request: &mut crate::Comptime::MirBridge::ExprEvalRequest<'_>,
) -> Result<Option<CtValue>, Diagnostic> {
    let crate::AST::Expr::Call(call) = request.expr else {
        return Ok(None);
    };
    if !matches!(
        call.name.as_str(),
        crate::Syntax::BUILTIN_EMBED_FILE
            | crate::Syntax::BUILTIN_EMBED_BYTES
            | crate::Syntax::BUILTIN_FIND
    ) {
        return Ok(None);
    }
    let literal = call.args.first().and_then(|arg| match &arg.expr {
        crate::AST::Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            crate::AST::StrPart::Lit(text) => Some(text.as_str()),
            _ => None,
        },
        _ => None,
    });
    Ok(Some(crate::Comptime::eval_build_time_io(
        &call.name,
        request.base_dir,
        literal,
        request.embed_inputs.as_deref_mut(),
        call.name_span,
    )?))
}

fn eval_expr_bridge(
    request: &mut crate::Comptime::MirBridge::ExprEvalRequest<'_>,
) -> Result<CtValue, Diagnostic> {
    if let Some(value) = try_eval_build_time_io(request)? {
        return Ok(value);
    }
    let mut names = request.binding_types.keys().cloned().collect::<Vec<_>>();
    names.sort();
    let mut values = request.globals.clone();
    if let Some(mutated) = request.mutated.as_ref() {
        for (name, value) in mutated.iter() {
            values.insert(name.clone(), value.clone());
        }
    }
    let args = fragment_args(&names, &values)?;
    let context = fragment_context(
        request.funcs,
        request.binding_types,
        request.error_conversions,
        request.method_traits,
        request.methods,
        request.extern_names,
        request.globals,
        request.core_imports,
        request.structs,
        request.checked_nominals.as_ref(),
        request.computed_fields,
        request.distinct_ranges,
        request.distinct_bases,
        request.unit_families,
    );
    let (program, function) = crate::Codegen::TIR::lower_mir_fragment_expr(request.expr, &context)
        .map_err(|error| mir_error_at(&error.message, error.span))?;
    let config = fragment_config(
        request.fuel,
        request.base_dir,
        &values,
        request.runtime_execution,
    )?;
    let result = evaluate_function_with_config_and_state(
        &program,
        function,
        &args,
        &config,
        request.data_pipeline,
    )
    .map_err(MirEvalError::into_diagnostic)?;
    let mut fields = fragment_fields(result.value, request.expr.span())?;
    let (_, value) = fields.first().cloned().ok_or_else(|| {
        mir_error_at(
            "MIR fragment result has no value field",
            request.expr.span(),
        )
    })?;
    fields.remove(0);
    if let Some(mutated) = request.mutated.as_deref_mut() {
        for (name, (_, field)) in names.iter().zip(fields) {
            mutated.insert(name.clone(), field);
        }
    }
    Ok(value)
}

fn eval_block_bridge<'a, 'debug>(
    request: &mut crate::Comptime::MirBridge::BlockEvalRequest<'a, 'debug>,
) -> Result<crate::Comptime::MirBridge::StmtOutcome, Diagnostic> {
    let context = fragment_context(
        request.funcs,
        request.binding_types,
        request.error_conversions,
        request.method_traits,
        request.methods,
        request.extern_names,
        request.globals,
        request.core_imports,
        request.structs,
        request.checked_nominals.as_ref(),
        request.computed_fields,
        request.distinct_ranges,
        request.distinct_bases,
        request.unit_families,
    );
    let (program, function) =
        crate::Codegen::TIR::lower_mir_fragment_block(request.stmts, &context)
            .map_err(|error| mir_error_at(&error.message, error.span))?;
    let mut names = request.binding_types.keys().cloned().collect::<Vec<_>>();
    names.sort();
    let args = fragment_args(&names, request.globals)?;
    let config = fragment_config(
        request.fuel,
        request.base_dir,
        request.globals,
        request.runtime_execution,
    )?;
    let debugger = request.debugger.take();
    let debug_function = request.debug_function.clone();
    let debug_depth = request.debug_depth;
    let (result, debugger) = evaluate_function_with_config_and_state_and_debugger(
        &program,
        function,
        &args,
        &config,
        request.data_pipeline,
        debugger,
        debug_function,
        debug_depth,
    )
    .map_err(MirEvalError::into_diagnostic)?;
    request.debugger = debugger;
    merge_fragment_output(&result, request.sink.as_deref_mut());
    let value = crate::Comptime::MirBridge::mir_to_ct_value(result.value, Span::new(0, 0))?;
    match value {
        CtValue::Struct { fields, .. } => {
            let scope = names
                .into_iter()
                .zip(fields.into_iter().map(|(_, value)| value))
                .collect();
            Ok(crate::Comptime::MirBridge::StmtOutcome::Done(scope))
        }
        value => Ok(crate::Comptime::MirBridge::StmtOutcome::Returned {
            value,
            scope: HashMap::new(),
        }),
    }
}

/// target configuration.  The artifact ID is required: program order and the
/// legacy `MirProgram::entry` field are never used as selection fallbacks.
pub fn evaluate_mir_program(
    program: &jet_foundation::MIR::MirProgram,
    artifact: MirArtifactId,
) -> Result<MirEvalResult, MirEvalError> {
    evaluate_mir_program_with_config(program, artifact, &MirEvalConfig::default())
}

/// Execute the selected artifact's canonical entry function.
///
/// The artifact row supplies the entry, output policy, initialization facts,
/// service lifetime, harness, and module closure.  Explicit function
/// execution remains separate below and does not require an artifact.
pub fn evaluate_mir_program_with_config(
    program: &jet_foundation::MIR::MirProgram,
    artifact: MirArtifactId,
    config: &MirEvalConfig,
) -> Result<MirEvalResult, MirEvalError> {
    program.cffi.validate_boundaries().map_err(|error| {
        MirEvalError::from(mir_error(
            &format!("invalid foreign boundary facts: {error}"),
            None,
        ))
    })?;
    jet_foundation::MIR::require_canonical_mir_optimization(program).map_err(|error| {
        MirEvalError::from(mir_error(
            &format!("MIR optimization precondition failed: {error}"),
            None,
        ))
    })?;
    let selection = select_artifact_for_eval(program, artifact)?;
    let _arrow_provider =
        ArrowProviderBridgeGuard::prepare(program, &selection).map_err(MirEvalError::from)?;
    let execution = program
        .execution_identity(Some(artifact))
        .map_err(|error| {
            MirEvalError::from(mir_error(
                &format!("MIR execution identity unavailable: {error}"),
                None,
            ))
        })?;
    let frame = Frame::new(
        program_function(program, selection.entry_function)?,
        Vec::new(),
        Vec::new(),
    )?;
    let mut data_pipeline = crate::Comptime::DataPipelineState::default();
    Machine::for_artifact(
        program,
        config,
        selection,
        vec![frame],
        execution,
        Some(&mut data_pipeline),
    )
    .run()
    .map_err(MirEvalError::from)
}

fn select_artifact_for_eval(
    program: &jet_foundation::MIR::MirProgram,
    artifact_id: MirArtifactId,
) -> Result<MirArtifactExecution, Diagnostic> {
    let plan = program
        .artifacts
        .iter()
        .find(|plan| plan.id == artifact_id)
        .ok_or_else(|| {
            mir_error(
                &format!("MIR artifact ID {:?} has no plan row", artifact_id),
                None,
            )
        })?;
    if !matches!(plan.target, MirArtifactTarget::Interpreter) {
        return Err(mir_error(
            &format!(
                "MIR artifact {:?} is not applicable to the interpreter target",
                artifact_id
            ),
            None,
        ));
    }
    if plan.modules.is_empty() {
        return Err(mir_error(
            &format!("MIR artifact {:?} has no selected module rows", artifact_id),
            None,
        ));
    }
    for module_id in &plan.modules {
        if !program.modules.iter().any(|module| module.id == *module_id) {
            return Err(mir_error(
                &format!(
                    "MIR artifact {:?} selects missing module ID {:?}",
                    artifact_id, module_id
                ),
                None,
            ));
        }
    }
    for link_id in &plan.links {
        if !program.links.iter().any(|link| link.id == *link_id) {
            return Err(mir_error(
                &format!(
                    "MIR artifact {:?} selects missing link ID {:?}",
                    artifact_id, link_id
                ),
                None,
            ));
        }
    }

    let harness = match plan.harness {
        Some(harness_id) => Some(
            program
                .harnesses
                .iter()
                .find(|candidate| candidate.id == harness_id)
                .ok_or_else(|| {
                    mir_error(
                        &format!(
                            "MIR artifact {:?} selects missing harness ID {:?}",
                            artifact_id, harness_id
                        ),
                        None,
                    )
                })?,
        ),
        None => None,
    };

    let harness_function = if let Some(harness) = harness {
        for test_id in &harness.tests {
            if !program.tests.iter().any(|test| test.id == *test_id) {
                return Err(mir_error(
                    &format!(
                        "MIR artifact {:?} harness {:?} references missing test ID {:?}",
                        artifact_id, harness.id, test_id
                    ),
                    None,
                ));
            }
        }
        for check in &harness.output_checks {
            if !program
                .functions
                .iter()
                .any(|function| function.id == check.function)
            {
                return Err(mir_error(
                    &format!(
                        "MIR artifact {:?} harness {:?} references missing output-check function {:?}",
                        artifact_id, harness.id, check.function
                    ),
                    Some(Span::new(0, 0)),
                ));
            }
        }
        for point in &harness.coverage_points {
            if !program
                .functions
                .iter()
                .any(|function| function.id == point.function)
            {
                return Err(mir_error(
                    &format!(
                        "MIR artifact {:?} harness {:?} references missing coverage function {:?}",
                        artifact_id, harness.id, point.function
                    ),
                    Some(point.span),
                ));
            }
        }
        harness.selected_test.map(|selected_test| {
            program
                .tests
                .iter()
                .find(|test| test.id == selected_test)
                .map(|test| test.function)
                .ok_or_else(|| {
                    mir_error(
                        &format!(
                            "MIR artifact {:?} harness {:?} references missing selected test ID {:?}",
                            artifact_id, harness.id, selected_test
                        ),
                        None,
                    )
                })
        }).transpose()?
    } else {
        None
    };

    let entry = plan.entry.as_ref().ok_or_else(|| {
        mir_error(
            &format!("MIR artifact {:?} has no entry plan", artifact_id),
            None,
        )
    })?;
    let entry_function = match (entry.function, harness_function) {
        (Some(function), Some(test_function)) if function != test_function => {
            return Err(mir_error(
                &format!(
                    "MIR artifact {:?} entry function {:?} disagrees with harness test function {:?}",
                    artifact_id, function, test_function
                ),
                None,
            ));
        }
        (Some(function), _) => function,
        (None, Some(function)) => function,
        (None, None) => {
            return Err(mir_error(
                &format!(
                    "MIR artifact {:?} entry has no function and no selected harness function",
                    artifact_id
                ),
                None,
            ));
        }
    };
    let entry_row = program_function(program, entry_function)?;
    if !entry_row.target_applicability.interpreter {
        return Err(mir_error_at(
            "MIR artifact entry function is not applicable to the interpreter target",
            entry_row.span,
        ));
    }
    if !plan
        .modules
        .iter()
        .any(|module_id| *module_id == entry_row.module_id)
    {
        return Err(mir_error(
            &format!(
                "MIR artifact {:?} entry function {:?} is outside its module closure",
                artifact_id, entry_function
            ),
            entry_row.span.into(),
        ));
    }

    let mut jobs = Vec::with_capacity(plan.jobs.len());
    for job_id in &plan.jobs {
        let job = program
            .jobs
            .iter()
            .find(|candidate| candidate.id == *job_id)
            .ok_or_else(|| {
                mir_error(
                    &format!(
                        "MIR artifact {:?} selects missing job ID {:?}",
                        artifact_id, job_id
                    ),
                    None,
                )
            })?;
        let job_function = program_function(program, job.function)?;
        if !job_function.target_applicability.interpreter {
            return Err(mir_error_at(
                "MIR artifact job function is not applicable to the interpreter target",
                job_function.span,
            ));
        }
        if !plan
            .modules
            .iter()
            .any(|module_id| *module_id == job_function.module_id)
        {
            return Err(mir_error(
                &format!(
                    "MIR artifact {:?} job ID {:?} is outside its module closure",
                    artifact_id, job.id
                ),
                job_function.span.into(),
            ));
        }
        jobs.push(job.id);
    }

    Ok(MirArtifactExecution {
        id: artifact_id,
        entry_function,

        entry_kind: entry.kind,
        output: entry.output,
        initialize_environment: entry.initialize_environment,
        initialize_gc: entry.initialize_gc,
        serves_until_stopped: entry.serves_until_stopped,
        harness: plan.harness,
        jobs,
        links: plan.links.clone(),
    })
}

type MirArtifactExecution = MirArtifactFacts;
/// A scoped provider/library pair for one interpreter execution.
///
/// Foundation owns the registration slot and lease count.  This guard owns
/// the matching dynamic-library reference; its explicit Drop order releases
/// the Foundation lease before `dlclose`, while any escaped shared Arrow batch
/// keeps the library owner Arc alive through its release callbacks.
struct ArrowProviderBridgeGuard {
    #[cfg(unix)]
    lease: Option<jet_foundation::ArrowFileReader::ProviderLease>,
    #[cfg(unix)]
    owner: Option<Arc<ArrowProviderLibrary>>,
}

#[cfg(unix)]
struct ArrowProviderLibrary {
    handle: *mut c_void,
}

#[cfg(unix)]
unsafe impl Send for ArrowProviderLibrary {}

#[cfg(unix)]
unsafe impl Sync for ArrowProviderLibrary {}

#[cfg(unix)]
impl Drop for ArrowProviderLibrary {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: Foundation's ProviderLease is dropped before the owner
            // Arc, so no registered callback can target this handle.
            unsafe {
                dlclose(self.handle);
            }
        }
    }
}

impl Drop for ArrowProviderBridgeGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let lease = self.lease.take();
            drop(lease);
            let owner = self.owner.take();
            drop(owner);
        }
    }
}

#[cfg(unix)]
fn try_acquire_arrow_provider(
    path: &Path,
) -> Result<
    Option<(
        jet_foundation::ArrowFileReader::ProviderLease,
        Arc<ArrowProviderLibrary>,
    )>,
    String,
> {
    let c_path = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| {
        format!(
            "MIR data provider bridge path contains NUL: {}",
            path.display()
        )
    })?;
    // SAFETY: the selected MIR link artifact is a compiler-produced dynamic
    // bridge. The owner Arc below controls this handle's lifetime.
    let handle = unsafe { dlopen(c_path.as_ptr(), 2) };
    if handle.is_null() {
        return Ok(None);
    }
    let symbol = CString::new("jet_data_read").expect("static symbol has no NUL");
    // SAFETY: `dlsym` only inspects this live bridge handle.
    let pointer = unsafe { dlsym(handle, symbol.as_ptr()) };
    if pointer.is_null() {
        // SAFETY: this candidate does not export the provider.
        unsafe {
            dlclose(handle);
        }
        return Ok(None);
    }
    // SAFETY: Parquet.rs exports exactly Foundation's Provider ABI.
    let provider: jet_foundation::ArrowFileReader::Provider =
        unsafe { std::mem::transmute(pointer) };
    let owner = Arc::new(ArrowProviderLibrary { handle });
    let owner_erased: Arc<dyn std::any::Any + Send + Sync> = owner.clone();
    let lease = match jet_foundation::ArrowFileReader::register_owned(provider, owner_erased) {
        Ok(lease) => lease,
        Err(error) => {
            drop(owner);
            return Err(format!(
                "MIR data provider registration failed for `{}`: {error}",
                path.display()
            ));
        }
    };
    Ok(Some((lease, owner)))
}

impl ArrowProviderBridgeGuard {
    fn prepare(
        program: &jet_foundation::MIR::MirProgram,
        artifact: &MirArtifactFacts,
    ) -> Result<Self, Diagnostic> {
        #[cfg(unix)]
        {
            for link_id in &artifact.links {
                let Some(link) = program.links.iter().find(|link| link.id == *link_id) else {
                    continue;
                };
                for candidate in &link.artifacts {
                    if !matches!(
                        candidate.kind,
                        jet_foundation::MIR::MirLinkArtifactKind::DynamicLibrary
                    ) {
                        continue;
                    }
                    let path = Path::new(&candidate.path);
                    match try_acquire_arrow_provider(path) {
                        Ok(Some((lease, owner))) => {
                            return Ok(Self {
                                lease: Some(lease),
                                owner: Some(owner),
                            });
                        }
                        Ok(None) => {}
                        Err(error) => return Err(mir_error(&error, None)),
                    }
                }
            }
        }
        Ok(Self {
            #[cfg(unix)]
            lease: None,
            #[cfg(unix)]
            owner: None,
        })
    }
}

/// Execute one stable MIR function ID with the default target configuration.
pub fn evaluate_mir_function(
    program: &jet_foundation::MIR::MirProgram,
    function: MirFunctionId,
    args: &[MirValue],
) -> Result<MirEvalResult, MirEvalError> {
    evaluate_mir_function_with_config(program, function, args, &MirEvalConfig::default())
}

/// Execute one stable MIR function ID after the caller has selected target and
/// host capability facts.
pub fn evaluate_mir_function_with_config(
    program: &jet_foundation::MIR::MirProgram,
    function: MirFunctionId,
    args: &[MirValue],
    config: &MirEvalConfig,
) -> Result<MirEvalResult, MirEvalError> {
    jet_foundation::MIR::require_canonical_mir_optimization(program).map_err(|error| {
        MirEvalError::from(mir_error(
            &format!("MIR optimization precondition failed: {error}"),
            None,
        ))
    })?;
    evaluate_function_with_config(program, function, args, config)
}

/// Execute one stable MIR function ID with a caller-selected artifact.
///
/// The artifact supplies the scoped provider/library lease used by callbacks
/// that perform data reads. The guard drops before this function returns, so
/// an existing resident provider remains installed and owned by its caller.
pub fn evaluate_mir_function_with_artifact_config(
    program: &jet_foundation::MIR::MirProgram,
    artifact: MirArtifactId,
    function: MirFunctionId,
    args: &[MirValue],
    config: &MirEvalConfig,
) -> Result<MirEvalResult, MirEvalError> {
    jet_foundation::MIR::require_canonical_mir_optimization(program).map_err(|error| {
        MirEvalError::from(mir_error(
            &format!("MIR optimization precondition failed: {error}"),
            None,
        ))
    })?;
    let selection = select_artifact_for_eval(program, artifact)?;
    let _arrow_provider =
        ArrowProviderBridgeGuard::prepare(program, &selection).map_err(MirEvalError::from)?;
    evaluate_function_with_config(program, function, args, config)
}

fn evaluate_function_with_config(
    program: &jet_foundation::MIR::MirProgram,
    function: MirFunctionId,
    args: &[MirValue],
    config: &MirEvalConfig,
) -> Result<MirEvalResult, MirEvalError> {
    let mut data_pipeline = crate::Comptime::DataPipelineState::default();
    evaluate_function_with_config_and_state(program, function, args, config, &mut data_pipeline)
}

fn evaluate_function_with_config_and_state(
    program: &jet_foundation::MIR::MirProgram,
    function: MirFunctionId,
    args: &[MirValue],
    config: &MirEvalConfig,
    data_pipeline: &mut crate::Comptime::DataPipelineState,
) -> Result<MirEvalResult, MirEvalError> {
    let _job_payload_scope = crate::Comptime::ServicesLite::job_payload_scope();
    let function_row = program_function(program, function)?;
    if !function_row.target_applicability.interpreter {
        return Err(MirEvalError::from(mir_error_at(
            "MIR function is not applicable to the interpreter target",
            function_row.span,
        )));
    }
    let frame = Frame::new(
        function_row,
        args.iter().cloned().map(RuntimeValue::Data).collect(),
        Vec::new(),
    )?;
    Machine::new(program, config, vec![frame], None, Some(data_pipeline))
        .run()
        .map_err(MirEvalError::from)
}
fn evaluate_function_with_config_and_state_and_debugger<'debug>(
    program: &jet_foundation::MIR::MirProgram,
    function: MirFunctionId,
    args: &[MirValue],
    config: &MirEvalConfig,
    data_pipeline: &mut crate::Comptime::DataPipelineState,
    debugger: Option<&'debug mut dyn crate::Comptime::DebugHook>,
    debug_function: String,
    debug_depth: usize,
) -> Result<
    (
        MirEvalResult,
        Option<&'debug mut dyn crate::Comptime::DebugHook>,
    ),
    MirEvalError,
> {
    let _job_payload_scope = crate::Comptime::ServicesLite::job_payload_scope();
    let function_row = program_function(program, function)?;
    if !function_row.target_applicability.interpreter {
        return Err(MirEvalError::from(mir_error_at(
            "MIR function is not applicable to the interpreter target",
            function_row.span,
        )));
    }
    let frame = Frame::new(
        function_row,
        args.iter().cloned().map(RuntimeValue::Data).collect(),
        Vec::new(),
    )?;
    let mut machine = Machine::new(program, config, vec![frame], None, Some(data_pipeline))
        .with_debugger(debugger, debug_function, debug_depth);
    let result = machine.run().map_err(MirEvalError::from)?;
    Ok((result, machine.take_debugger()))
}

#[derive(Clone)]
struct MirClosureHost {
    program: Arc<jet_foundation::MIR::MirProgram>,
    function: MirFunctionId,
    captures: Vec<CtValue>,
    config: MirEvalConfig,
}

impl MirClosureHost {
    fn run(
        &self,
        args: Vec<CtValue>,
        capture_parameters: bool,
        span: Span,
    ) -> Result<(CtValue, Vec<CtValue>), Diagnostic> {
        jet_foundation::MIR::require_canonical_mir_optimization(&self.program).map_err(
            |error| {
                mir_error_at(
                    &format!("MIR optimization precondition failed: {error}"),
                    span,
                )
            },
        )?;
        let function = program_function(&self.program, self.function)?;
        if !function.target_applicability.interpreter {
            return Err(mir_error_at(
                "MIR callback is not applicable to the interpreter target",
                function.span,
            ));
        }
        let frame = Frame::new(
            function,
            args.into_iter()
                .map(|value| runtime_from_ct(value, span))
                .collect::<Result<_, _>>()?,
            self.captures
                .iter()
                .cloned()
                .map(|value| runtime_from_ct(value, span))
                .collect::<Result<_, _>>()?,
        )?;
        let mut data_pipeline = crate::Comptime::DataPipelineState::default();
        let mut machine = Machine::new(
            &self.program,
            &self.config,
            vec![frame],
            None,
            Some(&mut data_pipeline),
        );
        let (value, status, _) = machine.run_runtime()?;
        if status != MirExecutionStatus::Completed {
            return Err(mir_error_at(
                "MIR native callback suspended before returning",
                span,
            ));
        }
        let value = machine.materialize_runtime(value, span)?;
        let result = machine.runtime_to_ct(value, span)?;
        let mut parameters = Vec::new();
        if capture_parameters {
            for value in machine.take_completed_parameters(span)? {
                let value = machine.materialize_runtime(value, span)?;
                parameters.push(machine.runtime_to_ct(value, span)?);
            }
        }
        Ok((result, parameters))
    }
}

impl crate::Comptime::StandaloneClosureHost for MirClosureHost {
    fn history_callback_identity(&self) -> Result<String, String> {
        let function = program_function(&self.program, self.function)
            .map_err(|_| "history callback has no checked function identity".to_string())?;
        let identity = format!(
            "function:{}:{}-{}",
            function.key, function.span.start, function.span.end,
        );
        crate::Comptime::history_callback_fingerprint(&identity, &self.captures)
    }

    fn invoke(&self, args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
        self.run(args, false, span).map(|(value, _)| value)
    }

    fn invoke_mut(&self, args: &mut Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
        let count = args.len();
        let (result, parameters) = self.run(std::mem::take(args), true, span)?;
        if count != parameters.len() {
            return Err(mir_error_at(
                "MIR callback changed its checked parameter count",
                span,
            ));
        }
        *args = parameters;
        Ok(result)
    }
}

/// Resume a frame captured by this machine or by a serialized scalar state.
pub fn resume_frame(
    program: &jet_foundation::MIR::MirProgram,
    frame: MirFrame,
    config: &MirEvalConfig,
) -> Result<MirValue, Diagnostic> {
    let frames = if let Some(runtime) = frame.runtime {
        runtime.frames
    } else {
        let mut frames = Vec::with_capacity(frame.state.call_stack.len() + 1);
        for state in &frame.state.call_stack {
            frames.push(frame_from_state(program, state)?);
        }
        frames.push(frame_from_state(program, &frame.state)?);
        frames
    };
    let execution = frame
        .state
        .identity
        .as_ref()
        .map(|identity| identity.execution.clone())
        .or_else(|| {
            frame
                .state
                .call_stack
                .iter()
                .find_map(|state| state.identity.as_ref().map(|identity| identity.execution.clone()))
        })
        .ok_or_else(|| {
            mir_error(
                "MIR frame identity unavailable: resume_frame(program, frame, config) requires a canonical frame identity",
                None,
            )
        })?;
    let mut machine = Machine::new(program, config, frames, Some(execution), None);
    machine.run().map(|result| result.value)
}

/// Execute the typed fixed-tree reduction routes directly in MIR evaluation.
/// TIR has selected these rows only for Float/F32 addition policy, so
/// interpreter execution must not fall back to sequential scalar accumulation.
fn eval_fixed_float_reduction(
    module: &str,
    member: &str,
    values: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let (f32, has_seed, para) = match (module, member) {
        ("core.list" | "core.builtin", "sum_fixed_f32") => (true, false, false),
        ("core.list" | "core.builtin", "sum_fixed_f64") => (false, false, false),
        ("core.list", "fold_add_fixed_f32") => (true, true, false),
        ("core.list", "fold_add_fixed_f64") => (false, true, false),
        ("core.list", "para_fold_add_fixed_f32") => (true, true, true),
        ("core.list", "para_fold_add_fixed_f64") => (false, true, true),
        _ => return None,
    };
    let expected_values = if has_seed { 2 } else { 1 };
    if values.len() != expected_values {
        return Some(Err(mir_error_at(
            "MIR fixed Float reduction received the wrong argument count",
            span,
        )));
    }
    let CtValue::List(items) = &values[0] else {
        return Some(Err(mir_error_at(
            "MIR fixed Float reduction receiver is not a list",
            span,
        )));
    };
    if f32 {
        let seed = if has_seed {
            let Some(CtValue::Float(CtFloat::F32(seed))) = values.get(1) else {
                return Some(Err(mir_error_at(
                    "MIR fixed F32 reduction seed has the wrong type",
                    span,
                )));
            };
            *seed
        } else {
            0.0
        };
        let mut inputs = Vec::with_capacity(items.len());
        for item in items {
            let CtValue::Float(CtFloat::F32(value)) = item else {
                return Some(Err(mir_error_at(
                    "MIR fixed F32 reduction input has the wrong type",
                    span,
                )));
            };
            inputs.push(*value);
        }
        let reduced = if para {
            fixed_float_reduction_prelude::jet_list_para_fold_add_fixed_f32(inputs, seed)
        } else {
            fixed_float_reduction_prelude::jet_simd_reduce_fixed(&inputs, seed)
        };
        Some(Ok(CtValue::Float(CtFloat::F32(reduced))))
    } else {
        let seed = if has_seed {
            let Some(CtValue::Float(CtFloat::F64(seed))) = values.get(1) else {
                return Some(Err(mir_error_at(
                    "MIR fixed Float reduction seed has the wrong type",
                    span,
                )));
            };
            *seed
        } else {
            0.0
        };
        let mut inputs = Vec::with_capacity(items.len());
        for item in items {
            let CtValue::Float(CtFloat::F64(value)) = item else {
                return Some(Err(mir_error_at(
                    "MIR fixed Float reduction input has the wrong type",
                    span,
                )));
            };
            inputs.push(*value);
        }
        let reduced = if para {
            fixed_float_reduction_prelude::jet_list_para_fold_add_fixed_f64(inputs, seed)
        } else {
            fixed_float_reduction_prelude::jet_simd_reduce_fixed(&inputs, seed)
        };
        Some(Ok(CtValue::Float(CtFloat::F64(reduced))))
    }
}

/// Dispatch one MIR Core/Prelude row through the installed ambient adapter,
/// then the phase-appropriate shared Core dispatcher. Runtime evaluation uses
/// the impure adapter so checked Tier-2 effects keep their runtime meaning;
/// comptime evaluation keeps the pure dispatcher and its effect gate.
fn eval_core_call_binding(
    mut data_pipeline: Option<&mut crate::Comptime::DataPipelineState>,
    module: &str,
    member: &str,
    values: Vec<CtValue>,
    type_args: &[crate::AST::Type],
    resolved_ret: Option<&crate::AST::Type>,
    data_plan: Option<&MirDataPlan>,
    span: Span,
    runtime: bool,
    base_dir: &std::path::Path,
    mut sink: Option<&mut crate::Comptime::DevSink>,
    history_schema: Option<&crate::Comptime::HistoryCommandSchema>,
) -> Result<CtValue, Diagnostic> {
    if let Some(result) = eval_fixed_float_reduction(module, member, &values, span) {
        return result;
    }
    if module == "core.ui" {
        match member {
            "null_backend" => {
                return Ok(mir_ui_backend_value(MirUiBackend::Null(
                    mir_ui_kernel::jet_ui_null(),
                )))
            }
            "tui_backend" => {
                return Ok(mir_ui_backend_value(MirUiBackend::Tui(
                    mir_ui_kernel::jet_ui_tui(),
                )))
            }
            _ => {}
        }
    }
    if module == "core.reactive" && member == "signal" {
        return crate::Comptime::AppLite::apply_suite(module, member, &values, span, resolved_ret);
    }
    if module == "core.collections" && member == "entries_to_map" {
        let [payload] = values.as_slice() else {
            return Err(mir_error_at(
                "core.collections.entries_to_map expects one object",
                span,
            ));
        };
        let entries = match payload {
            CtValue::Map(entries) => entries.clone(),
            CtValue::Struct { type_name, fields } if type_name == "JSONObject" => fields
                .iter()
                .map(|(key, value)| (crate::AST::CtKey::Str(key.clone()), value.clone()))
                .collect(),
            _ => {
                return Err(mir_error_at(
                    "core.collections.entries_to_map expects an object",
                    span,
                ))
            }
        };
        return Ok(CtValue::Map(entries));
    }
    if module == "core.encoding.json" && matches!(member, "to_string" | "to_string_pretty") {
        let [value] = values.as_slice() else {
            return Err(mir_error_at(
                "core.encoding.json renderer expects one DataTree value",
                span,
            ));
        };
        let rendered = if member == "to_string_pretty" {
            crate::Comptime::render_datatree_pretty_for_tir(value)
        } else {
            crate::Comptime::render_datatree_for_tir(value)
        };
        return Ok(CtValue::Str(rendered));
    }
    if let Some(state) = data_pipeline.as_deref_mut() {
        if let Some(result) = crate::Comptime::DataPipeline::eval_data_call_in_state(
            state,
            module,
            member,
            &values,
            type_args,
            resolved_ret,
            data_plan,
            span,
        ) {
            return result;
        }
    }
    if module == "core.testing" && member == "histories" {
        return crate::Comptime::apply_core_call_without_ambient_with_type_args_and_history_schema(
            module,
            member,
            values,
            span,
            false,
            type_args,
            resolved_ret,
            history_schema,
        );
    }
    if runtime {
        // The impure adapter owns runtime Core effects and performs the one
        // canonical ambient attempt before its shared fallback. Do not call
        // the ambient hook here as well: a declining adapter must be tried
        // exactly once and must never recurse through this binding.
        return crate::Comptime::apply_impure_core_call_with_type_args(
            module,
            member,
            values,
            span,
            base_dir,
            sink.as_deref_mut(),
            false,
            None,
            None,
            type_args,
            resolved_ret,
        );
    }
    if let Some(result) = crate::Comptime::try_core_call_typed_with_sink(
        module,
        member,
        values.clone(),
        span,
        resolved_ret.cloned(),
        sink.as_deref_mut(),
    ) {
        return result;
    }
    crate::Comptime::apply_core_call_without_ambient_with_type_args(
        module,
        member,
        values,
        span,
        false,
        type_args,
        resolved_ret,
    )
}

#[derive(Debug)]
struct MirPendingStreamWindow {
    key_index: usize,
    start_ns: i128,
    events: Vec<MirEvalValue>,
}

fn mir_stream_field(value: &MirEvalValue, name: &str) -> Option<MirEvalValue> {
    match value {
        MirEvalValue::Struct { fields, .. } => fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone()),
        _ => None,
    }
}

fn mir_stream_int(value: &MirEvalValue) -> Option<i64> {
    match value {
        MirEvalValue::Int(value) => Some(*value),
        MirEvalValue::BigInt(value) => value.parse::<i64>().ok(),
        MirEvalValue::Present(value) => mir_stream_int(value),
        _ => None,
    }
}

fn mir_stream_duration_ns(value: &MirEvalValue) -> Option<i128> {
    match value {
        MirEvalValue::Int(value) => Some(i128::from(*value)),
        MirEvalValue::BigInt(value) => value.parse::<i128>().ok(),
        MirEvalValue::Present(value) => mir_stream_duration_ns(value),
        MirEvalValue::Struct { type_name, .. } if type_name == "Duration" => {
            mir_stream_int(&mir_stream_field(value, "ns")?).map(i128::from)
        }
        _ => None,
    }
}

fn mir_stream_datetime_ns(value: &MirEvalValue) -> Option<i128> {
    let MirEvalValue::Struct { type_name, .. } = value else {
        return mir_stream_int(value).map(|seconds| i128::from(seconds) * 1_000_000_000);
    };
    if type_name != "DateTime" {
        return None;
    }
    let seconds = mir_stream_int(&mir_stream_field(value, "secs")?)?;
    let nanos = mir_stream_int(&mir_stream_field(value, "nanos")?)?;
    Some(i128::from(seconds) * 1_000_000_000 + i128::from(nanos))
}

fn mir_stream_datetime_value(total_ns: i128) -> MirEvalValue {
    let seconds = total_ns.div_euclid(1_000_000_000);
    let nanos = total_ns.rem_euclid(1_000_000_000);
    let seconds = seconds.clamp(i128::from(i64::MIN), i128::from(i64::MAX));
    MirEvalValue::Struct {
        type_name: "DateTime".to_string(),
        fields: vec![
            ("secs".to_string(), MirEvalValue::Int(seconds as i64)),
            ("nanos".to_string(), MirEvalValue::Int(nanos as i64)),
            ("leap_second".to_string(), MirEvalValue::Bool(false)),
        ],
    }
}

fn mir_stream_take_ready_windows(
    active: &mut Vec<MirPendingStreamWindow>,
    keys: &[MirEvalValue],
    width_ns: i128,
    watermark_ns: Option<i128>,
) -> Vec<MirEvalValue> {
    let mut ready = Vec::new();
    let mut retained = Vec::with_capacity(active.len());
    for window in active.drain(..) {
        let is_ready = watermark_ns
            .map(|watermark| window.start_ns.saturating_add(width_ns) <= watermark)
            .unwrap_or(false);
        if is_ready {
            ready.push(MirEvalValue::Struct {
                type_name: "Window".to_string(),
                fields: vec![
                    ("key".to_string(), keys[window.key_index].clone()),
                    (
                        "start".to_string(),
                        mir_stream_datetime_value(window.start_ns),
                    ),
                    (
                        "end".to_string(),
                        mir_stream_datetime_value(window.start_ns.saturating_add(width_ns)),
                    ),
                    ("events".to_string(), MirEvalValue::List(window.events)),
                ],
            });
        } else {
            retained.push(window);
        }
    }
    *active = retained;
    ready
}

type MirStreamHandle = Rc<RefCell<MirInterpreterStream>>;
#[derive(Debug)]
enum MirInterpreterZipMode {
    Short,
    Strict,
    Pad {
        left_fill: MirEvalValue,
        right_fill: MirEvalValue,
    },
}

#[derive(Debug)]
enum MirInterpreterGeneratorState {
    Pending {
        function: MirFunctionId,
        args: Vec<RuntimeValue>,
        captures: Vec<RuntimeValue>,
        capture_cells: Vec<Option<Rc<RefCell<RuntimeValue>>>>,
    },
    Suspended {
        frames: Vec<Frame>,
    },
    Done,
}

struct MirInterpreterTask {
    handle: Mutex<Option<crate::scheduler::JetSchedulerJoin<CtValue>>>,
    control: Arc<crate::scheduler::JetTaskControl>,
}

impl MirInterpreterTask {
    fn cancel(&self) {
        if self.handle.lock().unwrap().is_some() {
            self.control.cancel();
        }
    }

    fn drain(&self) {
        if let Some(handle) = self.handle.lock().unwrap().take() {
            handle.drain();
        }
    }

    fn take_entry(
        &self,
        span: Span,
    ) -> Result<
        (
            crate::scheduler::JetSchedulerJoin<CtValue>,
            Arc<crate::scheduler::JetTaskControl>,
        ),
        Diagnostic,
    > {
        let handle = self
            .handle
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| mir_error_at("MIR task was already joined", span))?;
        Ok((handle, self.control.clone()))
    }

    fn join(&self) -> Result<CtValue, crate::scheduler::JetTaskFailure> {
        let mut slot = self.handle.lock().unwrap();
        let result = {
            let Some(handle) = slot.as_mut() else {
                return Err(crate::scheduler::JetTaskFailure::Panicked(
                    "task already joined".to_string(),
                ));
            };
            handle.join()
        };
        let _ = slot.take();
        result
    }
}

struct MirInterpreterTaskGroup {
    children: Arc<crate::task_group::JetTaskGroupRuntime<Arc<MirInterpreterTask>>>,
}

impl MirInterpreterTaskGroup {
    fn close(&self) {
        if crate::task_group::jet_task_deadline_pending() {
            self.children
                .close_with_cancel(|child| child.cancel(), |child| child.drain());
        } else {
            self.children.close_with(|child| child.drain());
        }
    }
}

impl Drop for MirInterpreterTaskGroup {
    fn drop(&mut self) {
        self.close();
    }
}

fn mir_interpreter_task_carrier(task: Arc<MirInterpreterTask>) -> CtValue {
    CtValue::Struct {
        type_name: "__JetTirTask".to_string(),
        fields: vec![("value".to_string(), mir_runtime_owner_value(task))],
    }
}

fn mir_interpreter_task_from_ct(
    value: &CtValue,
    span: Span,
) -> Result<Arc<MirInterpreterTask>, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(mir_error_at(
            "MIR task value is not a checked task carrier",
            span,
        ));
    };
    if type_name != "__JetTirTask" {
        return Err(mir_error_at(
            "MIR task value is not a checked task carrier",
            span,
        ));
    }
    let [(name, value)] = fields.as_slice() else {
        return Err(mir_error_at(
            "MIR task carrier must contain exactly one value field",
            span,
        ));
    };
    if name != "value" {
        return Err(mir_error_at("MIR task carrier has no value field", span));
    }
    mir_runtime_owner::<Arc<MirInterpreterTask>>(value)
        .cloned()
        .ok_or_else(|| mir_error_at("MIR task carrier has no scheduler owner", span))
}

#[derive(Debug)]
struct MirRealtimeTask {
    scheduler_id: u64,
    requested_rate_hz: i64,
    requested_frames: i64,
    start_identity: i64,
    next_callback: u128,
    next_deadline_identity: i64,
    completed_callbacks: i64,
    completed_frames: i64,
    missed: i64,
    max_lateness_ns: i64,
    end_identity: i64,
    cancelled: bool,
    callback: RuntimeValue,
    sample_ring: Arc<crate::scheduler::JetRealtimeSampleRing>,
}

impl Drop for MirRealtimeTask {
    fn drop(&mut self) {
        crate::scheduler::jet_scheduler_owner_deadline_cancel(self.scheduler_id);
    }
}

type MirRealtimeTasks = Rc<RefCell<HashMap<u64, Rc<RefCell<MirRealtimeTask>>>>>;
struct PinnedDmaTransfer {
    value: RuntimeValue,
    buffer_ty: MirType,
    bytes: Box<[u8]>,
    completion_handle: Option<jet_foundation::ResourceSchedule::JetFrameCompletionHandle>,
}

#[derive(Debug)]
enum MirInterpreterStream {
    Source {
        values: VecDeque<MirEvalValue>,
    },
    Progress {
        source: MirStreamHandle,
        state: mir_human_output_semantics::JetOutputProgressIterState,
    },
    Take {
        source: MirStreamHandle,
        remaining: usize,
    },
    Range {
        cursor: crate::Comptime::CollectionEval::LoopRangeCursor,
    },
    List {
        cursor: crate::Comptime::CollectionEval::LoopListCursor<MirEvalValue>,
    },
    Map {
        source: MirStreamHandle,
        callback: RuntimeValue,
    },
    FilterMap {
        source: MirStreamHandle,
        callback: RuntimeValue,
    },
    Filter {
        source: MirStreamHandle,
        callback: RuntimeValue,
    },
    TakeWhile {
        source: MirStreamHandle,
        callback: RuntimeValue,
        done: bool,
    },
    SkipWhile {
        source: MirStreamHandle,
        callback: RuntimeValue,
        skipping: bool,
    },
    FlatMap {
        source: MirStreamHandle,
        callback: RuntimeValue,
        pending: VecDeque<MirEvalValue>,
    },
    Scan {
        source: MirStreamHandle,
        callback: RuntimeValue,
        accumulator: MirEvalValue,
    },
    Zip {
        left: MirStreamHandle,
        right: MirStreamHandle,
        callback: RuntimeValue,
        mode: MirInterpreterZipMode,
    },

    AmbientLines {
        handle: i64,
        closed: bool,
    },
    AmbientFileLines {
        receiver: crate::AST::CtValue,
        closed: bool,
    },

    Generator {
        state: MirInterpreterGeneratorState,
    },
    EventTime {
        source: MirStreamHandle,
        callback: RuntimeValue,
        datetime: bool,
    },
    Keyed {
        source: MirStreamHandle,
        callback: RuntimeValue,
    },
    Window {
        source: MirStreamHandle,
        window_ns: i128,
        lateness_ns: i128,
        side_output: bool,
        keys: Vec<MirEvalValue>,
        active: Vec<MirPendingStreamWindow>,
        side_output_events: Vec<MirEvalValue>,
        max_event_ns: Option<i128>,
        watermark_ns: Option<i128>,
        ready: VecDeque<MirEvalValue>,
        finished: bool,
    },
    Realtime {
        state: Rc<RefCell<MirRealtimeTask>>,
    },
}

#[derive(Debug)]
struct MirInterpreterStreamCursor {
    source: MirStreamHandle,
    current: Option<MirEvalValue>,
    step: usize,
}

fn close_ambient_line_reader(handle: i64, span: Span) -> Result<(), Diagnostic> {
    let result =
        crate::Comptime::try_ambient_mir_handle("loop.lines.close", Some(handle), Vec::new(), span)
            .ok_or_else(|| {
                mir_error_at(
                    "interpreter line iterator has no ambient close host binding",
                    span,
                )
            })??;
    match result {
        crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Unit) => Ok(()),
        crate::Comptime::AmbientMirHandleResult::Value(_)
        | crate::Comptime::AmbientMirHandleResult::Handle(_) => Err(mir_error_at(
            "interpreter line iterator close returned the wrong typed carrier",
            span,
        )),
    }
}

fn pull_ambient_line(
    handle: &mut i64,
    closed: &mut bool,
    span: Span,
) -> Result<Option<MirEvalValue>, Diagnostic> {
    if *closed {
        return Ok(None);
    }
    let result =
        crate::Comptime::try_ambient_mir_handle("loop.lines.next", Some(*handle), Vec::new(), span)
            .ok_or_else(|| {
                mir_error_at(
                    "interpreter line iterator has no ambient next host binding",
                    span,
                )
            })?;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = close_ambient_line_reader(*handle, span);
            *closed = true;
            return Err(error);
        }
    };
    match result {
        crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Present(value)) => {
            match *value {
                MirEvalValue::String(line) => Ok(Some(MirEvalValue::String(line))),
                _ => {
                    let _ = close_ambient_line_reader(*handle, span);
                    *closed = true;
                    Err(mir_error_at(
                        "interpreter line iterator next returned a non-String Present value",
                        span,
                    ))
                }
            }
        }
        crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Absent { .. }) => {
            let close = close_ambient_line_reader(*handle, span);
            *closed = true;
            close.map(|()| None)
        }
        crate::Comptime::AmbientMirHandleResult::Value(_)
        | crate::Comptime::AmbientMirHandleResult::Handle(_) => Err(mir_error_at(
            "interpreter line iterator next returned the wrong typed carrier",
            span,
        )),
    }
}
fn pull_ambient_file_line(
    receiver: &crate::AST::CtValue,
    closed: &mut bool,
    span: Span,
) -> Result<Option<MirEvalValue>, Diagnostic> {
    if *closed {
        return Ok(None);
    }
    let line = match crate::Comptime::file_reader_next_line(receiver) {
        Ok(line) => line,
        Err(error) => {
            *closed = true;
            return Err(mir_error_at(
                &format!("interpreter file line iterator read failed: {error}"),
                span,
            ));
        }
    };
    match line {
        Some(line) => Ok(Some(MirEvalValue::String(line))),
        None => {
            *closed = true;
            Ok(None)
        }
    }
}

fn mir_stream_exact_len(handle: &MirStreamHandle) -> Option<usize> {
    let stream = handle.try_borrow().ok()?;
    match &*stream {
        MirInterpreterStream::Source { values } => Some(values.len()),
        MirInterpreterStream::Take { source, remaining } => {
            if *remaining == 0 {
                Some(0)
            } else {
                mir_stream_exact_len(source).map(|length| length.min(*remaining))
            }
        }
        MirInterpreterStream::Map { source, .. }
        | MirInterpreterStream::Scan { source, .. }
        | MirInterpreterStream::Progress { source, .. } => mir_stream_exact_len(source),
        _ => None,
    }
}

fn mir_progress_style_enabled() -> bool {
    crate::terminal_runtime::jet_term_style_enabled(
        std::env::var_os("NO_COLOR").is_some(),
        std::env::var_os("FORCE_COLOR").is_some(),
        crate::terminal_runtime::jet_term_stdout_is_terminal(),
    )
}

fn mir_progress_emit(text: &str, span: Span) -> Result<(), Diagnostic> {
    if !crate::terminal_runtime::jet_term_progress_enabled() {
        return Ok(());
    }
    let frame = crate::terminal_runtime::jet_term_progress_frame(
        crate::terminal_runtime::jet_term_stderr_is_terminal(),
        text,
    );
    crate::terminal_runtime::jet_term_write_stderr(&frame, true)
        .map_err(|error| mir_error_at(&format!("MIR progress output failed: {error}"), span))
}

fn mir_progress_finish(span: Span) -> Result<(), Diagnostic> {
    if !crate::terminal_runtime::jet_term_progress_enabled() {
        return Ok(());
    }
    let frame = crate::terminal_runtime::jet_term_progress_finish(
        crate::terminal_runtime::jet_term_stderr_is_terminal(),
    );
    if frame.is_empty() {
        return Ok(());
    }
    crate::terminal_runtime::jet_term_write_stderr(frame, true)
        .map_err(|error| mir_error_at(&format!("MIR progress output failed: {error}"), span))
}

impl Drop for MirInterpreterStream {
    fn drop(&mut self) {
        match self {
            Self::Progress { state, .. } => {
                if let Err(error) = state.finish(|| mir_progress_finish(Span::new(0, 0))) {
                    panic!("MIR progress finish failed: {error:?}");
                }
            }
            Self::AmbientLines { handle, closed } => {
                if !*closed {
                    let _ = close_ambient_line_reader(*handle, Span::new(0, 0));
                    *closed = true;
                }
            }
            Self::AmbientFileLines { closed, .. } => {
                *closed = true;
            }
            _ => {}
        }
    }
}

impl MirInterpreterStream {
    fn pull(
        &mut self,
        machine: &mut Machine<'_, '_, '_>,
        span: Span,
    ) -> Result<Option<MirEvalValue>, Diagnostic> {
        match self {
            Self::Source { values } => Ok(values.pop_front()),
            Self::Progress { source, state } => {
                let source = source.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    state.finish(|| mir_progress_finish(span))?;
                    return Ok(None);
                };
                state.advance(
                    mir_progress_style_enabled(),
                    |text| mir_progress_emit(text, span),
                    || mir_progress_finish(span),
                )?;
                Ok(Some(value))
            }
            Self::Take { source, remaining } => {
                if *remaining == 0 {
                    return Ok(None);
                }
                let source = source.clone();
                let value = mir_stream_pull_handle(&source, machine, span)?;
                if value.is_some() {
                    *remaining -= 1;
                }
                Ok(value)
            }
            Self::Range { cursor } => Ok(cursor.next().map(MirEvalValue::Int)),
            Self::List { cursor } => Ok(cursor.next()),
            Self::Map { source, callback } => {
                let source = source.clone();
                let callback = callback.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    return Ok(None);
                };
                let value = machine.invoke_callback(callback, RuntimeValue::Data(value), span)?;
                Ok(Some(Machine::closure_callback_data(value, span)?))
            }
            Self::FilterMap { source, callback } => loop {
                let source = source.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    return Ok(None);
                };
                let mapped =
                    machine.invoke_callback(callback.clone(), RuntimeValue::Data(value), span)?;
                if let Ok(value) = Machine::closure_callback_outcome(mapped)? {
                    return runtime_to_data(value, span).map(Some);
                }
            },
            Self::Filter { source, callback } => loop {
                let source = source.clone();
                let callback = callback.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    return Ok(None);
                };
                let predicate =
                    machine.invoke_callback(callback, RuntimeValue::Data(value.clone()), span)?;
                let predicate = Machine::closure_callback_data(predicate, span)?;
                let MirEvalValue::Bool(keep) = predicate else {
                    return Err(mir_error_at(
                        "MIR lazy filter callback must return Bool",
                        span,
                    ));
                };
                if keep {
                    return Ok(Some(value));
                }
            },
            Self::TakeWhile {
                source,
                callback,
                done,
            } => {
                if *done {
                    return Ok(None);
                }
                let source = source.clone();
                let callback = callback.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    *done = true;
                    return Ok(None);
                };
                let keep =
                    machine.invoke_callback(callback, RuntimeValue::Data(value.clone()), span)?;
                let keep = Machine::closure_callback_data(keep, span)?;
                let MirEvalValue::Bool(keep) = keep else {
                    return Err(mir_error_at(
                        "MIR lazy take_while callback must return Bool",
                        span,
                    ));
                };
                if keep {
                    Ok(Some(value))
                } else {
                    *done = true;
                    Ok(None)
                }
            }
            Self::SkipWhile {
                source,
                callback,
                skipping,
            } => loop {
                let source = source.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    return Ok(None);
                };
                if !*skipping {
                    return Ok(Some(value));
                }
                let keep = machine.invoke_callback(
                    callback.clone(),
                    RuntimeValue::Data(value.clone()),
                    span,
                )?;
                let keep = Machine::closure_callback_data(keep, span)?;
                let MirEvalValue::Bool(keep) = keep else {
                    return Err(mir_error_at(
                        "MIR lazy skip_while callback must return Bool",
                        span,
                    ));
                };
                if !keep {
                    *skipping = false;
                    return Ok(Some(value));
                }
            },
            Self::FlatMap {
                source,
                callback,
                pending,
            } => loop {
                if let Some(value) = pending.pop_front() {
                    return Ok(Some(value));
                }
                let source = source.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    return Ok(None);
                };
                let value =
                    machine.invoke_callback(callback.clone(), RuntimeValue::Data(value), span)?;
                let value = Machine::closure_callback_data(value, span)?;
                let MirEvalValue::List(values) = value else {
                    return Err(mir_error_at(
                        "MIR lazy flat_map callback must return a List",
                        span,
                    ));
                };
                pending.extend(values);
            },
            Self::Scan {
                source,
                callback,
                accumulator,
            } => {
                let source = source.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    return Ok(None);
                };
                let next = machine.invoke_callback_args(
                    callback.clone(),
                    vec![
                        RuntimeValue::Data(accumulator.clone()),
                        RuntimeValue::Data(value),
                    ],
                    span,
                )?;
                let next = Machine::closure_callback_data(next, span)?;
                *accumulator = next.clone();
                Ok(Some(next))
            }
            Self::Zip {
                left,
                right,
                callback,
                mode,
            } => {
                let left = left.clone();
                let right = right.clone();
                let callback = callback.clone();
                let left = mir_stream_pull_handle(&left, machine, span)?;
                let pair = match mode {
                    MirInterpreterZipMode::Short => {
                        let mut pull_error = None;
                        let pair =
                            crate::Comptime::CollectionEval::jet_zip_short_step(left, || {
                                match mir_stream_pull_handle(&right, machine, span) {
                                    Ok(value) => value,
                                    Err(error) => {
                                        pull_error = Some(error);
                                        None
                                    }
                                }
                            });
                        if let Some(error) = pull_error {
                            return Err(error);
                        }
                        pair
                    }
                    MirInterpreterZipMode::Strict => {
                        let right = mir_stream_pull_handle(&right, machine, span)?;
                        match crate::Comptime::CollectionEval::jet_zip_strict_step(left, right) {
                            Ok(pair) => pair,
                            Err(()) => {
                                let message = crate::Comptime::CollectionEval::jet_zip_length_mismatch_message();
                                return Err(if machine.config.runtime_execution {
                                    machine.located_runtime_stop(
                                        "E3001",
                                        "<core.collections>",
                                        0,
                                        message,
                                        span,
                                    )
                                } else {
                                    crate::Comptime::comptime_panic(message, span)
                                });
                            }
                        }
                    }
                    MirInterpreterZipMode::Pad {
                        left_fill,
                        right_fill,
                    } => {
                        let right = mir_stream_pull_handle(&right, machine, span)?;
                        crate::Comptime::CollectionEval::jet_zip_pad_step(
                            left,
                            right,
                            left_fill.clone(),
                            right_fill.clone(),
                        )
                    }
                };
                let Some((left, right)) = pair else {
                    return Ok(None);
                };
                let value = machine.invoke_callback_args(
                    callback,
                    vec![RuntimeValue::Data(left), RuntimeValue::Data(right)],
                    span,
                )?;
                Ok(Some(Machine::closure_callback_data(value, span)?))
            }
            Self::AmbientLines { handle, closed } => pull_ambient_line(handle, closed, span),
            Self::AmbientFileLines { receiver, closed } => {
                pull_ambient_file_line(receiver, closed, span)
            }

            Self::Generator { state } => machine.pull_generator(state, span),
            Self::EventTime {
                source,
                callback,
                datetime,
            } => {
                let source = source.clone();
                let callback = callback.clone();
                let Some(value) = mir_stream_pull_handle(&source, machine, span)? else {
                    return Ok(None);
                };
                let timestamp =
                    machine.invoke_callback(callback, RuntimeValue::Data(value.clone()), span)?;
                let timestamp = runtime_to_data(timestamp, span)?;
                let timestamp = if *datetime {
                    if !matches!(
                        &timestamp,
                        MirEvalValue::Struct { type_name, .. } if type_name == "DateTime"
                    ) {
                        return Err(mir_error_at(
                            "stream.with_event_time DateTime callback returned a non-DateTime value",
                            span,
                        ));
                    }
                    timestamp
                } else {
                    let seconds = mir_stream_int(&timestamp).ok_or_else(|| {
                        mir_error_at(
                            "stream.with_event_time callback must return Int seconds",
                            span,
                        )
                    })?;
                    mir_stream_datetime_value(i128::from(seconds) * 1_000_000_000)
                };
                Ok(Some(MirEvalValue::Struct {
                    type_name: "StreamEventTime".to_string(),
                    fields: vec![
                        ("value".to_string(), value),
                        ("event_time".to_string(), timestamp),
                    ],
                }))
            }
            Self::Keyed { source, callback } => {
                let source = source.clone();
                let callback = callback.clone();
                let Some(event) = mir_stream_pull_handle(&source, machine, span)? else {
                    return Ok(None);
                };
                let value = mir_stream_field(&event, "value").ok_or_else(|| {
                    mir_error_at("stream.key_by received an event without a value", span)
                })?;
                let key = machine.invoke_callback(callback, RuntimeValue::Data(value), span)?;
                let key = runtime_to_data(key, span)?;
                Ok(Some(MirEvalValue::Struct {
                    type_name: "KeyedStream".to_string(),
                    fields: vec![("key".to_string(), key), ("event".to_string(), event)],
                }))
            }
            Self::Window {
                source,
                window_ns,
                lateness_ns,
                side_output,
                keys,
                active,
                side_output_events,
                max_event_ns,
                watermark_ns,
                ready,
                finished,
            } => {
                if let Some(window) = ready.pop_front() {
                    return Ok(Some(window));
                }
                if *finished {
                    return Ok(None);
                }
                loop {
                    let source = source.clone();
                    let Some(keyed) = mir_stream_pull_handle(&source, machine, span)? else {
                        ready.extend(mir_stream_take_ready_windows(
                            active,
                            keys,
                            *window_ns,
                            Some(i128::MAX),
                        ));
                        *finished = true;
                        return Ok(ready.pop_front());
                    };
                    let key = mir_stream_field(&keyed, "key").ok_or_else(|| {
                        mir_error_at("stream.window received a malformed keyed event", span)
                    })?;
                    let event = mir_stream_field(&keyed, "event").ok_or_else(|| {
                        mir_error_at(
                            "stream.window received a keyed event without an event",
                            span,
                        )
                    })?;
                    let event_ns = mir_stream_datetime_ns(
                        &mir_stream_field(&event, "event_time").ok_or_else(|| {
                            mir_error_at("stream.window received an event without event_time", span)
                        })?,
                    )
                    .ok_or_else(|| {
                        mir_error_at("stream.window received an invalid event_time", span)
                    })?;
                    let was_late = watermark_ns.is_some_and(|watermark| event_ns < watermark);
                    if max_event_ns.is_none_or(|max| event_ns > max) {
                        *max_event_ns = Some(event_ns);
                        let candidate = event_ns.saturating_sub(*lateness_ns);
                        if watermark_ns.is_none_or(|watermark| candidate > watermark) {
                            *watermark_ns = Some(candidate);
                        }
                    }
                    if was_late {
                        if *side_output {
                            side_output_events.push(event);
                        }
                        continue;
                    }
                    let key_index =
                        keys.iter()
                            .position(|known| known == &key)
                            .unwrap_or_else(|| {
                                keys.push(key.clone());
                                keys.len() - 1
                            });
                    let start_ns = event_ns.div_euclid(*window_ns) * *window_ns;
                    if let Some(window) = active
                        .iter_mut()
                        .find(|window| window.key_index == key_index && window.start_ns == start_ns)
                    {
                        window.events.push(event);
                    } else {
                        let position = active
                            .iter()
                            .position(|window| {
                                window.key_index > key_index
                                    || (window.key_index == key_index && window.start_ns > start_ns)
                            })
                            .unwrap_or(active.len());
                        active.insert(
                            position,
                            MirPendingStreamWindow {
                                key_index,
                                start_ns,
                                events: vec![event],
                            },
                        );
                    }
                    ready.extend(mir_stream_take_ready_windows(
                        active,
                        keys,
                        *window_ns,
                        *watermark_ns,
                    ));
                    if let Some(window) = ready.pop_front() {
                        return Ok(Some(window));
                    }
                }
            }
            Self::Realtime { .. } => Err(mir_error_at(
                "real-time callback stream cannot be iterated",
                span,
            )),
        }
    }
}

fn mir_stream_pull_handle(
    handle: &MirStreamHandle,
    machine: &mut Machine<'_, '_, '_>,
    span: Span,
) -> Result<Option<MirEvalValue>, Diagnostic> {
    let mut stream = handle
        .try_borrow_mut()
        .map_err(|_| mir_error_at("interpreter stream is already being pulled", span))?;
    stream.pull(machine, span)
}

struct Machine<'a, 'state, 'debug> {
    program: &'a jet_foundation::MIR::MirProgram,
    config: &'a MirEvalConfig,
    data_pipeline: Option<&'state mut crate::Comptime::DataPipelineState>,
    frames: Vec<Frame>,
    continuations: Vec<Option<Continuation>>,
    debugger: Option<&'debug mut dyn crate::Comptime::DebugHook>,
    debug_function: String,
    debug_depth: usize,
    debug_statements: Vec<Option<DebugStatement>>,
    steps: u64,
    stdout: String,
    stderr: String,
    exit_code: i32,
    execution: Option<MirExecutionIdentity>,
    sequence: u64,
    artifact: Option<MirArtifactFacts>,
    static_values: Rc<RefCell<BTreeMap<String, RuntimeValue>>>,
    service_callbacks: Rc<RefCell<BTreeMap<String, RuntimeValue>>>,
    hardware_setup_pending: bool,
    realtime_tasks: MirRealtimeTasks,
    process_stdin_tokens: Rc<RefCell<BTreeMap<i64, MirHandleToken>>>,
    dma_transfers: Rc<RefCell<HashMap<u64, PinnedDmaTransfer>>>,
    next_dma_transfer_id: Rc<RefCell<u64>>,
    completed_parameters: Option<Vec<RuntimeValue>>,
    last_runtime_stop: Option<String>,
    /// Held for the machine's lifetime so the scoped Web runtime policy is
    /// restored on drop; never read directly.
    _web_runtime_policy: crate::Comptime::AppLite::WebRuntimePolicyGuard,
}
#[derive(Debug, Clone)]
struct Continuation {
    result: Option<MirValueId>,
}
#[derive(Clone)]
struct DebugStatement {
    function: String,
    depth: usize,
    span: Span,
}

impl<'a, 'state, 'debug> Machine<'a, 'state, 'debug> {
    fn new(
        program: &'a jet_foundation::MIR::MirProgram,
        config: &'a MirEvalConfig,
        frames: Vec<Frame>,
        execution: Option<MirExecutionIdentity>,
        data_pipeline: Option<&'state mut crate::Comptime::DataPipelineState>,
    ) -> Self {
        Self::new_with_static_values(
            program,
            config,
            frames,
            execution,
            Rc::new(RefCell::new(BTreeMap::new())),
            true,
            data_pipeline,
        )
    }
    fn with_debugger(
        mut self,
        debugger: Option<&'debug mut dyn crate::Comptime::DebugHook>,
        debug_function: String,
        debug_depth: usize,
    ) -> Self {
        self.debugger = debugger;
        self.debug_function = debug_function;
        self.debug_depth = debug_depth;
        self.debug_statements = if self.debugger.is_some() {
            (0..self.frames.len()).map(|_| None).collect()
        } else {
            Vec::new()
        };
        self
    }

    fn take_debugger(&mut self) -> Option<&'debug mut dyn crate::Comptime::DebugHook> {
        self.debugger.take()
    }

    fn new_with_static_values(
        program: &'a jet_foundation::MIR::MirProgram,
        config: &'a MirEvalConfig,
        frames: Vec<Frame>,
        execution: Option<MirExecutionIdentity>,
        static_values: Rc<RefCell<BTreeMap<String, RuntimeValue>>>,
        hardware_setup_pending: bool,
        data_pipeline: Option<&'state mut crate::Comptime::DataPipelineState>,
    ) -> Self {
        Self::new_with_realtime_tasks(
            program,
            config,
            frames,
            execution,
            static_values,
            Rc::new(RefCell::new(BTreeMap::new())),
            hardware_setup_pending,
            Rc::new(RefCell::new(HashMap::new())),
            Rc::new(RefCell::new(HashMap::new())),
            Rc::new(RefCell::new(BTreeMap::new())),
            Rc::new(RefCell::new(1)),
            data_pipeline,
        )
    }
    fn new_with_realtime_tasks(
        program: &'a jet_foundation::MIR::MirProgram,
        config: &'a MirEvalConfig,
        frames: Vec<Frame>,
        execution: Option<MirExecutionIdentity>,
        static_values: Rc<RefCell<BTreeMap<String, RuntimeValue>>>,
        service_callbacks: Rc<RefCell<BTreeMap<String, RuntimeValue>>>,
        hardware_setup_pending: bool,
        dma_transfers: Rc<RefCell<HashMap<u64, PinnedDmaTransfer>>>,
        realtime_tasks: MirRealtimeTasks,
        process_stdin_tokens: Rc<RefCell<BTreeMap<i64, MirHandleToken>>>,
        next_dma_transfer_id: Rc<RefCell<u64>>,
        data_pipeline: Option<&'state mut crate::Comptime::DataPipelineState>,
    ) -> Self {
        let web_runtime_policy = crate::Comptime::AppLite::set_web_runtime_policy(
            !config.release_devtools_policy.is_release()
                || config.release_devtools_policy.stream_code,
            !config.release_devtools_policy.is_release()
                || config.release_devtools_policy.local_rail,
        );
        let continuations = vec![None; frames.len()];
        Self {
            program,
            config,
            data_pipeline,
            frames,
            continuations,
            debugger: None,
            debug_function: String::new(),
            debug_depth: 0,
            debug_statements: Vec::new(),
            steps: 0,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            execution,
            sequence: 0,
            artifact: None,
            static_values,
            service_callbacks,
            hardware_setup_pending,
            process_stdin_tokens,
            dma_transfers,
            next_dma_transfer_id,
            realtime_tasks,
            completed_parameters: None,
            last_runtime_stop: None,
            _web_runtime_policy: web_runtime_policy,
        }
    }

    fn pull_generator(
        &mut self,
        state: &mut MirInterpreterGeneratorState,
        span: Span,
    ) -> Result<Option<MirEvalValue>, Diagnostic> {
        let (frames, execution) = match state {
            MirInterpreterGeneratorState::Pending {
                function,
                args,
                captures,
                capture_cells,
            } => {
                let function_row = program_function(self.program, *function)?;
                let frame = Frame::with_capture_cells(
                    function_row,
                    std::mem::take(args),
                    std::mem::take(captures),
                    std::mem::take(capture_cells),
                )?;
                (vec![frame], self.execution.clone())
            }
            MirInterpreterGeneratorState::Suspended { frames } => {
                (std::mem::take(frames), self.execution.clone())
            }
            MirInterpreterGeneratorState::Done => return Ok(None),
        };
        if execution.is_none() {
            return Err(mir_error_at(
                "interpreter Stream generator requires a resumable MIR execution identity",
                span,
            ));
        }
        let nested_function = frames
            .first()
            .and_then(|frame| program_function(self.program, frame.function).ok())
            .map(|function| function.name.clone())
            .unwrap_or_else(|| self.debug_function.clone());
        let nested_debugger = self.debugger.take();
        let nested_debug_depth = self.debug_depth.saturating_add(self.frames.len());
        let mut nested = Machine::new_with_realtime_tasks(
            self.program,
            self.config,
            frames,
            execution,
            self.static_values.clone(),
            self.service_callbacks.clone(),
            false,
            self.dma_transfers.clone(),
            self.realtime_tasks.clone(),
            self.process_stdin_tokens.clone(),
            self.next_dma_transfer_id.clone(),
            self.data_pipeline.as_deref_mut(),
        )
        .with_debugger(nested_debugger, nested_function, nested_debug_depth);
        let result = nested.run()?;
        self.debugger = nested.take_debugger();
        self.stdout.push_str(&nested.stdout);
        self.stderr.push_str(&nested.stderr);
        self.exit_code = self.exit_code.max(nested.exit_code);
        match result.status {
            MirExecutionStatus::Suspended => {
                let frame = result.frame.ok_or_else(|| {
                    mir_error_at(
                        "interpreter Stream generator suspension has no resumable frame",
                        span,
                    )
                })?;
                let snapshot = frame.runtime.ok_or_else(|| {
                    mir_error_at(
                        "interpreter Stream generator suspension lost its runtime frame",
                        span,
                    )
                })?;
                *state = MirInterpreterGeneratorState::Suspended {
                    frames: snapshot.frames,
                };
                Ok(Some(result.value))
            }
            MirExecutionStatus::Completed => {
                *state = MirInterpreterGeneratorState::Done;
                Ok(None)
            }
        }
    }
    fn for_artifact(
        program: &'a jet_foundation::MIR::MirProgram,
        config: &'a MirEvalConfig,
        artifact: MirArtifactFacts,
        frames: Vec<Frame>,
        execution: MirExecutionIdentity,
        data_pipeline: Option<&'state mut crate::Comptime::DataPipelineState>,
    ) -> Self {
        let mut machine = Self::new(program, config, frames, Some(execution), data_pipeline);
        machine.artifact = Some(artifact);
        machine
    }
    fn completed_parameter_values(
        &self,
        frame_index: usize,
        span: Span,
    ) -> Result<Vec<RuntimeValue>, Diagnostic> {
        let frame = self
            .frames
            .get(frame_index)
            .ok_or_else(|| mir_error_at("MIR completed frame is unavailable", span))?;
        let function = program_function(self.program, frame.function)?;
        let mut parameters = frame.params.clone();
        for block in &function.blocks {
            for instruction in &block.instructions {
                let MirOperation::Parameter { index, .. } = &instruction.operation else {
                    continue;
                };
                let Some(result) = instruction.result else {
                    continue;
                };
                if let (Some(parameter), Some(value)) =
                    (parameters.get_mut(*index), frame.values.get(&result))
                {
                    *parameter = value.clone();
                }
            }
        }
        Ok(parameters)
    }

    fn take_completed_parameters(&mut self, span: Span) -> Result<Vec<RuntimeValue>, Diagnostic> {
        self.completed_parameters
            .take()
            .ok_or_else(|| mir_error_at("MIR completed parameter state is unavailable", span))
    }

    fn is_runtime_invocation(&self) -> bool {
        self.config.runtime_execution
    }

    fn merge_runtime_sink(&mut self, sink: Option<crate::Comptime::DevSink>) {
        let Some(sink) = sink else {
            return;
        };
        self.stdout.push_str(&sink.stdout);
        self.stderr.push_str(&sink.stderr);
        if let Some(exit_code) = sink.exit_code {
            self.exit_code = self.exit_code.max(exit_code);
        }
    }

    fn run(&mut self) -> Result<MirEvalResult, Diagnostic> {
        let (value, status, frame) = match self.run_runtime() {
            Ok(result) => result,
            Err(error) if error.code == "SOFT_EXIT" => {
                return Ok(MirEvalResult {
                    value: MirEvalValue::Unit,
                    stdout: std::mem::take(&mut self.stdout),
                    stderr: std::mem::take(&mut self.stderr),
                    exit_code: self.exit_code,
                    status: MirExecutionStatus::Completed,
                    frame: None,
                    artifact: self.artifact.clone(),
                });
            }
            Err(error) => return Err(error),
        };
        let value = self.serve_entry_app(value, status)?;
        self.result(value, status, frame)
    }

    /// D-ENTRY-VALUE1=B / R12: an entry that returns an `App` serves that value
    /// at the runtime edge. AOT emits `.serve()` and the resident JIT calls
    /// `serve_app` at the same boundary; the interpreter marshals the handle
    /// into the identical Prelude `App.serve` method, which reads the
    /// `JET_APP_PORT` / `JET_APP_DEV` contract itself.
    fn serve_entry_app(
        &mut self,
        value: RuntimeValue,
        status: MirExecutionStatus,
    ) -> Result<RuntimeValue, Diagnostic> {
        let serves = self
            .artifact
            .as_ref()
            .is_some_and(|artifact| artifact.serves_until_stopped);
        if !serves || status != MirExecutionStatus::Completed {
            return Ok(value);
        }
        let value = match value {
            RuntimeValue::Result { ok: true, value } => *value,
            value => value,
        };
        let RuntimeValue::App(handle) = value else {
            return Ok(value);
        };
        let span = Span::new(0, 0);
        match crate::Comptime::AppLite::app_method_runtime(&handle, "serve", &[], &[], None, span)?
        {
            crate::Comptime::AppLite::AppRuntimeResult::Value(_) => {
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            crate::Comptime::AppLite::AppRuntimeResult::App(_) => Err(mir_error_at(
                "MIR entry App serve returned a builder instead of completing",
                span,
            )),
        }
    }

    fn abort_frame_transactions(&mut self, frame_index: usize) {
        let transactions = std::mem::take(&mut self.frames[frame_index].shared_transactions);
        for transaction in transactions.values() {
            transaction.abort();
        }
    }

    fn begin_shared_transaction(
        &mut self,
        frame_index: usize,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let (scope, parent_errors) = {
            let frame = &self.frames[frame_index];
            let scope = *frame
                .scopes
                .last()
                .ok_or_else(|| mir_error_at("MIR Shared STM has no active scope", span))?;
            let function = program_function(self.program, frame.function)?;
            let scope_row = function
                .scopes
                .iter()
                .find(|candidate| candidate.id == scope)
                .ok_or_else(|| mir_error_at("MIR Shared STM scope is unavailable", span))?;
            if scope_row.kind != MirScopeKind::Transaction {
                return Err(mir_error_at(
                    "MIR Shared STM global is outside a transaction scope",
                    span,
                ));
            }
            let parent_errors = frame
                .scopes
                .iter()
                .rev()
                .skip(1)
                .find_map(|scope| frame.shared_transactions.get(scope))
                .map(|transaction| transaction.errors.clone());
            (scope, parent_errors)
        };
        let transaction = MirSharedTransaction::begin(parent_errors);
        if self.frames[frame_index]
            .shared_transactions
            .insert(scope, transaction.clone())
            .is_some()
        {
            return Err(mir_error_at(
                "MIR Shared STM scope was initialized twice",
                span,
            ));
        }
        Ok(RuntimeValue::SharedTransaction(transaction))
    }
    fn debug_instruction_span(&self, frame_index: usize) -> Option<Span> {
        let frame = self.frames.get(frame_index)?;
        let function = program_function(self.program, frame.function).ok()?;
        let block = function
            .blocks
            .iter()
            .find(|block| block.id == frame.block)?;
        let span = block.instructions.get(frame.ip)?.span;
        (span.start < span.end).then_some(span)
    }

    fn debug_scope(
        &mut self,
        frame_index: usize,
        span: Span,
    ) -> Result<HashMap<String, CtValue>, Diagnostic> {
        let entries = {
            let frame = self
                .frames
                .get(frame_index)
                .ok_or_else(|| mir_error_at("MIR debugger frame is unavailable", span))?;
            let function = program_function(self.program, frame.function)?;
            let mut entries = Vec::new();
            for parameter in &function.params {
                if let Some(value) = frame.params.get(parameter.index) {
                    entries.push((parameter.name.clone(), value.clone()));
                }
            }
            for local in &function.locals {
                if let Some(value) = frame.locals.get(&local.id) {
                    entries.push((local.name.clone(), value.clone()));
                }
            }
            entries
        };
        let mut scope = HashMap::new();
        for (name, value) in entries {
            let Ok(value) = self.runtime_to_ct(value, span) else {
                continue;
            };
            scope.entry(name).or_insert(value);
        }
        Ok(scope)
    }

    fn debug_begin_statement(&mut self, frame_index: usize, span: Span) -> Result<(), Diagnostic> {
        if self.debugger.is_none() {
            return Ok(());
        }
        let (function, depth) = if frame_index == 0 {
            (self.debug_function.clone(), self.debug_depth)
        } else {
            let name = self
                .frames
                .get(frame_index)
                .and_then(|frame| program_function(self.program, frame.function).ok())
                .map(|function| function.name.clone())
                .unwrap_or_else(|| self.debug_function.clone());
            (name, self.debug_depth.saturating_add(frame_index))
        };
        let scope = self.debug_scope(frame_index, span)?;
        let result = match self.debugger.take() {
            Some(debugger) => {
                let result = debugger.at_stmt(&function, depth, span, &scope);
                self.debugger = Some(debugger);
                result
            }
            None => Ok(()),
        };
        result?;
        if let Some(statement) = self.debug_statements.get_mut(frame_index) {
            *statement = Some(DebugStatement {
                function,
                depth,
                span,
            });
        }
        Ok(())
    }

    fn debug_finish_statement(&mut self, frame_index: usize) -> Result<(), Diagnostic> {
        let Some(statement) = self
            .debug_statements
            .get_mut(frame_index)
            .and_then(Option::take)
        else {
            return Ok(());
        };
        let scope = self.debug_scope(frame_index, statement.span)?;
        match self.debugger.take() {
            Some(debugger) => {
                let result = debugger.after_stmt(
                    &statement.function,
                    statement.depth,
                    statement.span,
                    &scope,
                );
                self.debugger = Some(debugger);
                result
            }
            None => Ok(()),
        }
    }

    fn debug_sync(&mut self, frame_index: usize) -> Result<(), Diagnostic> {
        if self.debugger.is_none() {
            return Ok(());
        }
        let next = self.debug_instruction_span(frame_index);
        let current = self
            .debug_statements
            .get(frame_index)
            .and_then(|statement| statement.as_ref())
            .map(|statement| statement.span);
        if current == next {
            return Ok(());
        }
        if current.is_some() {
            self.debug_finish_statement(frame_index)?;
        }
        if let Some(span) = next {
            self.debug_begin_statement(frame_index, span)?;
        }
        Ok(())
    }

    fn run_runtime(
        &mut self,
    ) -> Result<(RuntimeValue, MirExecutionStatus, Option<MirFrame>), Diagnostic> {
        if self.hardware_setup_pending {
            self.install_hardware_setups()?;
            self.hardware_setup_pending = false;
        }
        loop {
            if self.steps >= self.config.fuel {
                return Err(mir_error("MIR execution exhausted its fuel", None));
            }
            self.steps = self.steps.saturating_add(1);
            let Some(index) = self.frames.len().checked_sub(1) else {
                return Err(mir_error("MIR execution ended without a value", None));
            };
            self.pump_realtime_tasks(Span::new(0, 0))?;
            self.debug_sync(index)?;
            let stderr_len = self.stderr.len();
            let step = match self.step(index) {
                Ok(step) => step,
                Err(error) if error.code == "SOFT_EXIT" => {
                    let Some(code) = self.last_runtime_stop.clone() else {
                        return Err(error);
                    };
                    if self.catch_expected_runtime_stop(index, &code)? {
                        self.last_runtime_stop = None;
                        self.stderr.truncate(stderr_len);
                        self.exit_code = 0;
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            match step {
                Action::Continue => {}
                Action::Call {
                    function,
                    args,
                    captures,
                    capture_cells,
                    result,
                } => {
                    let function_row = program_function(self.program, function)?;
                    if function_row.generator.is_some() {
                        let result = result.ok_or_else(|| {
                            mir_error_at(
                                "interpreter Stream generator call has no result value",
                                function_row.span,
                            )
                        })?;
                        let stream = RuntimeValue::Stream(Rc::new(RefCell::new(
                            MirInterpreterStream::Generator {
                                state: MirInterpreterGeneratorState::Pending {
                                    function,
                                    args,
                                    captures,
                                    capture_cells,
                                },
                            },
                        )));
                        let caller = self.frames.last_mut().ok_or_else(|| {
                            mir_error_at(
                                "interpreter Stream generator call has no caller frame",
                                function_row.span,
                            )
                        })?;
                        caller.values.insert(result, stream);
                        continue;
                    }
                    let callee =
                        Frame::with_capture_cells(function_row, args, captures, capture_cells)?;
                    self.frames.push(callee);
                    self.debug_statements.push(None);
                    self.continuations.push(Some(Continuation { result }));
                }

                Action::Return(value) => {
                    self.debug_finish_statement(index)?;
                    let continuation = self.continuations.pop().flatten();
                    let completed_parameters = if continuation.is_none() {
                        Some(self.completed_parameter_values(index, Span::new(0, 0))?)
                    } else {
                        None
                    };
                    self.abort_frame_transactions(index);
                    self.frames.pop();
                    self.debug_statements.pop();
                    if let Some(continuation) = continuation {
                        let caller = self
                            .frames
                            .last_mut()
                            .ok_or_else(|| mir_error("MIR continuation has no caller", None))?;
                        if let Some(result) = continuation.result {
                            caller.values.insert(result, value);
                        }
                    } else {
                        self.completed_parameters = completed_parameters;
                        return Ok((value, MirExecutionStatus::Completed, None));
                    }
                }
                Action::Suspend(value) => {
                    self.debug_finish_statement(index)?;
                    let frame = self.suspended_frame()?;
                    return Ok((value, MirExecutionStatus::Suspended, Some(frame)));
                }
            }
        }
    }

    fn catch_expected_runtime_stop(
        &mut self,
        frame_index: usize,
        code: &str,
    ) -> Result<bool, Diagnostic> {
        let mut target = None;
        let upper = frame_index.min(self.frames.len().saturating_sub(1));
        'frames: for index in (0..=upper).rev() {
            let function = program_function(self.program, self.frames[index].function)?;
            for scope_id in self.frames[index].scopes.iter().rev().copied() {
                let Some(scope) = function.scopes.iter().find(|scope| scope.id == scope_id) else {
                    continue;
                };
                if scope.kind != MirScopeKind::ScopeMember
                    || !matches!(
                        function.test_scope_member(scope_id),
                        Some(MirTestScopeMember::ExpectFail { expected_code })
                            if expected_code.as_deref().is_none_or(|expected| expected == code)
                    )
                {
                    continue;
                }
                let Some(exit) = function.blocks.iter().find_map(|block| {
                    block.instructions.iter().any(|instruction| {
                        matches!(
                            &instruction.operation,
                            MirOperation::ScopeExit { scope } if *scope == scope_id
                        )
                    })
                    .then_some(block.id)
                }) else {
                    continue;
                };
                target = Some((index, scope_id, exit));
                break 'frames;
            }
        }
        let Some((target_frame, target_scope, exit)) = target else {
            return Ok(false);
        };
        while self.frames.len() > target_frame + 1 {
            let index = self.frames.len() - 1;
            self.abort_frame_transactions(index);
            self.frames.pop();
            self.continuations.pop();
            if !self.debug_statements.is_empty() {
                self.debug_statements.pop();
            }
        }
        let scope_position = self.frames[target_frame]
            .scopes
            .iter()
            .rposition(|scope| *scope == target_scope)
            .ok_or_else(|| mir_error("MIR expected-failure scope disappeared during unwind", None))?;
        let removed_scopes = self.frames[target_frame]
            .scopes
            .split_off(scope_position + 1);
        for scope in removed_scopes {
            if let Some(transaction) = self.frames[target_frame]
                .shared_transactions
                .remove(&scope)
            {
                transaction.abort();
            }
            self.frames[target_frame].completed_scope_stops.remove(&scope);
            self.frames[target_frame].timeout_starts.remove(&scope);
        }
        self.frames[target_frame]
            .completed_scope_stops
            .insert(target_scope, code.to_string());
        self.frames[target_frame].block = exit;
        self.frames[target_frame].ip = 0;
        self.frames[target_frame].predecessor = None;
        Ok(true)
    }

    fn drain_pending_hardware_interrupts(&mut self, span: Span) -> Result<(), Diagnostic> {
        for vector in crate::embedded_hardware::jet_hardware_drain_pending() {
            let mut handler = None;
            for setup in &self.program.facts.hardware_setups {
                match setup {
                    MirHardwareSetup::DmaConfigure { .. } => {}
                    MirHardwareSetup::InterruptBind {
                        vector: bound_vector,
                        handler_symbol,
                        ..
                    } if *bound_vector == vector => {
                        if handler.is_some() {
                            return Err(mir_error_at(
                                "MIR interrupt vector has duplicate bindings",
                                span,
                            ));
                        }
                        let mut functions = self
                            .program
                            .functions
                            .iter()
                            .filter(|function| function.key == *handler_symbol);
                        let function = functions.next().ok_or_else(|| {
                            mir_error_at(
                                &format!(
                                    "MIR interrupt handler `{handler_symbol}` has no function row"
                                ),
                                span,
                            )
                        })?;
                        if functions.next().is_some() {
                            return Err(mir_error_at(
                                &format!(
                                    "MIR interrupt handler `{handler_symbol}` resolves to multiple function rows"
                                ),
                                span,
                            ));
                        }
                        if !function.params.is_empty() || !function.capture_params.is_empty() {
                            return Err(mir_error_at(
                                &format!(
                                    "MIR interrupt handler `{handler_symbol}` must have a zero-argument ABI"
                                ),
                                span,
                            ));
                        }
                        handler = Some(function.id);
                    }
                    MirHardwareSetup::InterruptBind { .. } => {}
                }
            }
            let Some(function) = handler else {
                continue;
            };
            let _ = self.invoke_function(function, Vec::new(), Vec::new(), span)?;
        }
        Ok(())
    }

    fn pump_realtime_tasks(&mut self, span: Span) -> Result<(), Diagnostic> {
        loop {
            let now = jet_foundation::Monotonic::jet_time_monotonic_now_ns();
            let Some(scheduler_id) = crate::scheduler::jet_scheduler_owner_deadline_pop_due(now)
            else {
                return Ok(());
            };
            let task = self.realtime_tasks.borrow_mut().remove(&scheduler_id);
            let Some(task) = task else {
                continue;
            };
            let task_info = {
                let state = task.borrow();
                if state.cancelled {
                    None
                } else {
                    Some((
                        state.next_deadline_identity,
                        state.requested_rate_hz,
                        state.requested_frames,
                        state.callback.clone(),
                        state.sample_ring.clone(),
                    ))
                }
            };
            let Some((deadline_identity, rate_hz, frames, callback, sample_ring)) = task_info
            else {
                continue;
            };
            if !sample_ring.publish_zeroed() {
                let mut state = task.borrow_mut();
                state.cancelled = true;
                if state.end_identity == 0 {
                    state.end_identity = jet_foundation::Monotonic::jet_time_monotonic_now_ns();
                }
                return Err(mir_error_at("MIR realtime sample ring is full", span));
            }
            let buffer = sample_ring
                .pop()
                .ok_or_else(|| mir_error_at("MIR realtime sample ring lost a sample", span))?
                .into_iter()
                .map(|value| MirEvalValue::Float { value, f32: false })
                .collect();
            let started_identity = jet_foundation::Monotonic::jet_time_monotonic_now_ns();
            let lateness = started_identity.saturating_sub(deadline_identity).max(0);
            {
                let mut state = task.borrow_mut();
                if lateness > 0 {
                    state.missed = state.missed.saturating_add(1);
                    state.max_lateness_ns = state.max_lateness_ns.max(lateness);
                }
            }
            let callback_result = self.invoke_callback(
                callback,
                RuntimeValue::Data(MirEvalValue::List(buffer)),
                span,
            );
            if let Err(error) = callback_result {
                let mut state = task.borrow_mut();
                state.cancelled = true;
                if state.end_identity == 0 {
                    state.end_identity = jet_foundation::Monotonic::jet_time_monotonic_now_ns();
                }
                return Err(error);
            }
            let mut state = task.borrow_mut();
            if state.cancelled {
                if state.end_identity == 0 {
                    state.end_identity = jet_foundation::Monotonic::jet_time_monotonic_now_ns();
                }
                continue;
            }
            state.completed_callbacks = state.completed_callbacks.saturating_add(1);
            state.completed_frames = state.completed_frames.saturating_add(frames);
            state.next_callback = state.next_callback.saturating_add(1);
            let offset =
                crate::scheduler::jet_rt_deadline_offset_ns(rate_hz, frames, state.next_callback);
            state.next_deadline_identity = state.start_identity.saturating_add(offset);
            let next_scheduler_id = crate::scheduler::jet_scheduler_owner_deadline_register(
                state.next_deadline_identity,
            );
            state.scheduler_id = next_scheduler_id;
            drop(state);
            self.realtime_tasks
                .borrow_mut()
                .insert(next_scheduler_id, task);
        }
    }

    fn realtime_state(value: &RuntimeValue) -> Option<Rc<RefCell<MirRealtimeTask>>> {
        let RuntimeValue::Stream(handle) = value else {
            return None;
        };
        let stream = handle.try_borrow().ok()?;
        match &*stream {
            MirInterpreterStream::Realtime { state } => Some(state.clone()),
            _ => None,
        }
    }

    fn cancel_realtime_state(&mut self, state: &Rc<RefCell<MirRealtimeTask>>) {
        let scheduler_id = {
            let mut state = state.borrow_mut();
            state.cancelled = true;
            if state.end_identity == 0 {
                state.end_identity = jet_foundation::Monotonic::jet_time_monotonic_now_ns();
            }
            state.scheduler_id
        };
        crate::scheduler::jet_scheduler_owner_deadline_cancel(scheduler_id);
        self.realtime_tasks.borrow_mut().remove(&scheduler_id);
    }

    fn realtime_instant(deadline_ns: i64) -> MirEvalValue {
        MirEvalValue::Struct {
            type_name: "Instant".to_string(),
            fields: vec![("start_ns".to_string(), MirEvalValue::Int(deadline_ns))],
        }
    }

    fn realtime_receipt(state: &MirRealtimeTask) -> MirEvalValue {
        MirEvalValue::Struct {
            type_name: "RealtimeReceipt".to_string(),
            fields: vec![
                (
                    "requested_rate_hz".to_string(),
                    MirEvalValue::Int(state.requested_rate_hz),
                ),
                (
                    "requested_frames".to_string(),
                    MirEvalValue::Int(state.requested_frames),
                ),
                (
                    "completed_callbacks".to_string(),
                    MirEvalValue::Int(state.completed_callbacks),
                ),
                (
                    "completed_frames".to_string(),
                    MirEvalValue::Int(state.completed_frames),
                ),
                ("missed".to_string(), MirEvalValue::Int(state.missed)),
                (
                    "max_lateness_ns".to_string(),
                    MirEvalValue::Int(state.max_lateness_ns),
                ),
                (
                    "start_identity".to_string(),
                    MirEvalValue::Int(state.start_identity),
                ),
                (
                    "end_identity".to_string(),
                    MirEvalValue::Int(state.end_identity),
                ),
            ],
        }
    }

    fn eval_realtime_handle(
        &mut self,
        member: &str,
        state: Rc<RefCell<MirRealtimeTask>>,
        arg_count: usize,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if arg_count != 0 {
            return Err(mir_error_at(
                "real-time stream lifecycle methods take no arguments",
                span,
            ));
        }
        match member {
            "realtime.next_deadline" => {
                let deadline = state.borrow().next_deadline_identity;
                Ok(RuntimeValue::Data(Self::realtime_instant(deadline)))
            }
            "realtime.receipt" => {
                let receipt = Self::realtime_receipt(&state.borrow());
                Ok(RuntimeValue::Data(receipt))
            }
            "realtime.cancel" => {
                self.cancel_realtime_state(&state);
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            "realtime.is_cancelled" => Ok(RuntimeValue::Data(MirEvalValue::Bool(
                state.borrow().cancelled,
            ))),
            _ => Err(mir_error_at(
                "unknown real-time stream lifecycle method",
                span,
            )),
        }
    }
    fn task_group_owner(
        &mut self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<Arc<MirInterpreterTaskGroup>, Diagnostic> {
        let value = match value {
            RuntimeValue::Address(address) => {
                require_address_access(&address, MirAccess::Read, span)?;
                self.read_place(address.frame, address.place, span)?
            }
            value => value,
        };
        let RuntimeValue::Ambient(value) = value else {
            return Err(mir_error_at(
                "MIR task group value is not a scheduler owner",
                span,
            ));
        };
        mir_runtime_owner::<Arc<MirInterpreterTaskGroup>>(&value)
            .cloned()
            .ok_or_else(|| mir_error_at("MIR task group value has no scheduler owner", span))
    }

    fn eval_task_join(
        &mut self,
        receiver: RuntimeValue,
        flatten_result: bool,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let receiver = match receiver {
            RuntimeValue::Address(address) => {
                require_address_access(&address, MirAccess::Read, span)?;
                self.read_place(address.frame, address.place, span)?
            }
            receiver => receiver,
        };
        if let RuntimeValue::Ambient(value) = &receiver {
            if matches!(
                value,
                CtValue::Struct { type_name, .. } if type_name == "__JetTirTask"
            ) {
                let has_scheduler_owner = match value {
                    CtValue::Struct { fields, .. } => fields.iter().any(|(_, value)| {
                        mir_runtime_owner::<Arc<MirInterpreterTask>>(value).is_some()
                    }),
                    _ => false,
                };
                if has_scheduler_owner {
                    let task = mir_interpreter_task_from_ct(value, span)?;
                    let payload = match task.join() {
                        Ok(value) => runtime_from_ct(value, span)?,
                        Err(failure) => return Ok(task_failure_result(failure)),
                    };
                    return Self::eval_task_payload(payload, flatten_result, span);
                }
            }
        }
        Self::eval_task_join_legacy(receiver, flatten_result, span)
    }

    fn eval_task_join_legacy(
        receiver: RuntimeValue,
        flatten_result: bool,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let payload = match receiver {
            RuntimeValue::Data(MirEvalValue::Struct { type_name, fields })
                if type_name == "__JetTirTask" =>
            {
                let [(name, value)] = fields.as_slice() else {
                    return Err(mir_error_at(
                        "MIR task carrier must contain exactly one value field",
                        span,
                    ));
                };
                if name != "value" {
                    return Err(mir_error_at("MIR task carrier has no value field", span));
                }
                RuntimeValue::Data(value.clone())
            }
            RuntimeValue::Ambient(CtValue::Struct { type_name, fields })
                if type_name == "__JetTirTask" =>
            {
                let [(name, value)] = fields.as_slice() else {
                    return Err(mir_error_at(
                        "MIR task carrier must contain exactly one value field",
                        span,
                    ));
                };
                if name != "value" {
                    return Err(mir_error_at("MIR task carrier has no value field", span));
                }
                runtime_from_ct(value.clone(), span)?
            }
            _ => {
                return Err(mir_error_at(
                    "MIR task join receiver is not a checked task carrier",
                    span,
                ))
            }
        };
        Self::eval_task_payload(payload, flatten_result, span)
    }

    fn eval_task_payload(
        payload: RuntimeValue,
        flatten_result: bool,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if !flatten_result {
            return Ok(RuntimeValue::Result {
                ok: true,
                value: Box::new(payload),
            });
        }
        match payload {
            RuntimeValue::Result { ok: true, value } => {
                Ok(RuntimeValue::Result { ok: true, value })
            }
            RuntimeValue::Result { ok: false, value } => Ok(RuntimeValue::Result {
                ok: false,
                value: Box::new(RuntimeValue::Data(task_failure_value(format!(
                    "task body returned an error: {value:?}"
                )))),
            }),
            RuntimeValue::Data(MirEvalValue::Present(value)) => Ok(RuntimeValue::Result {
                ok: true,
                value: Box::new(RuntimeValue::Data(*value)),
            }),
            RuntimeValue::Data(MirEvalValue::FailedTold(value)) => Ok(RuntimeValue::Result {
                ok: false,
                value: Box::new(RuntimeValue::Data(task_failure_value(format!(
                    "task body returned an error: {value:?}"
                )))),
            }),
            RuntimeValue::Ambient(CtValue::Present(value)) => Ok(RuntimeValue::Result {
                ok: true,
                value: Box::new(runtime_from_ct(*value, span)?),
            }),
            RuntimeValue::Ambient(value) => {
                let report = value.told_report().ok_or_else(|| {
                    mir_error_at("MIR task join result carrier is not a told result", span)
                })?;
                Ok(RuntimeValue::Result {
                    ok: false,
                    value: Box::new(RuntimeValue::Data(task_failure_value(format!(
                        "task body returned an error: {report:?}",
                    )))),
                })
            }
            _ => Err(mir_error_at(
                "MIR task join result carrier is not a result",
                span,
            )),
        }
    }

    fn eval_task_group(
        &mut self,
        call: MirPreludeCallId,
        kind: MirTaskGroupKind,
        values: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let (family, module, member, symbol, abi) = {
            let row = self.prelude_row(call, span)?;
            (
                row.family,
                row.module.clone(),
                row.member.clone(),
                row.symbol.name().to_string(),
                row.abi,
            )
        };
        if family != jet_foundation::MIR::MirPreludeFamily::StaticPrelude
            || module != "core.tasks"
            || abi != jet_foundation::MIR::MirPreludeAbi::Value
        {
            return Err(mir_error_at(
                "MIR task-group route has unsupported ABI or family",
                span,
            ));
        }
        let (expected_member, expected_symbol, flatten_result) = match kind {
            MirTaskGroupKind::All => {
                if member == "all_result" {
                    ("all_result", "jet_std::jet_task_all_result", true)
                } else {
                    ("all", "jet_std::jet_task_all", false)
                }
            }
            MirTaskGroupKind::Race => {
                if member == "race_result" {
                    ("race_result", "jet_std::jet_task_race_result", true)
                } else {
                    ("race", "jet_std::jet_task_race", false)
                }
            }
            MirTaskGroupKind::Any => {
                if member == "any_result" {
                    ("any_result", "jet_std::jet_task_any_result", true)
                } else {
                    ("any", "jet_std::jet_task_any", false)
                }
            }
        };
        if member != expected_member || symbol != expected_symbol || values.len() != 1 {
            return Err(mir_error_at(
                "MIR task-group route has invalid member, symbol, or arity",
                span,
            ));
        }
        let tasks = self.materialize_runtime(
            values.into_iter().next().expect("checked task-group arity"),
            span,
        )?;
        let tasks = self.runtime_to_ct(tasks, span)?;
        let CtValue::List(tasks) = tasks else {
            return Err(mir_error_at(
                "MIR task-group operation requires a List of tasks",
                span,
            ));
        };
        if tasks.is_empty() && !matches!(kind, MirTaskGroupKind::All) {
            return Err(mir_error_at(
                "MIR task-group race/any requires at least one task",
                span,
            ));
        }
        let mut entries: Vec<(
            crate::scheduler::JetSchedulerJoin<CtValue>,
            Arc<crate::scheduler::JetTaskControl>,
        )> = Vec::with_capacity(tasks.len());
        for task in tasks {
            let task = match mir_interpreter_task_from_ct(&task, span) {
                Ok(task) => task,
                Err(error) => {
                    for (handle, _) in entries {
                        handle.drain();
                    }
                    return Err(error);
                }
            };
            let entry = match task.take_entry(span) {
                Ok(entry) => entry,
                Err(error) => {
                    for (handle, _) in entries {
                        handle.drain();
                    }
                    return Err(error);
                }
            };
            entries.push(entry);
        }
        let result = match kind {
            MirTaskGroupKind::All => {
                let completions = match crate::scheduler::jet_scheduler_all(entries) {
                    Ok(completions) => completions,
                    Err(failure) => return Ok(task_failure_result(failure)),
                };
                if !flatten_result {
                    let payload = runtime_from_ct(CtValue::List(completions), span)?;
                    RuntimeValue::Result {
                        ok: true,
                        value: Box::new(payload),
                    }
                } else {
                    let mut payloads = Vec::with_capacity(completions.len());
                    for completion in completions {
                        let joined = Self::eval_task_payload(
                            runtime_from_ct(completion, span)?,
                            true,
                            span,
                        )?;
                        match joined {
                            RuntimeValue::Result { ok: true, value } => {
                                payloads.push(self.runtime_to_ct(*value, span)?);
                            }
                            RuntimeValue::Result { ok: false, value } => {
                                return Ok(RuntimeValue::Result { ok: false, value });
                            }
                            _ => {
                                return Err(mir_error_at(
                                    "MIR task-group result adapter returned a non-result",
                                    span,
                                ));
                            }
                        }
                    }
                    let payload = runtime_from_ct(CtValue::List(payloads), span)?;
                    RuntimeValue::Result {
                        ok: true,
                        value: Box::new(payload),
                    }
                }
            }
            MirTaskGroupKind::Race => {
                let completion = match crate::scheduler::jet_scheduler_race(entries) {
                    Ok(completion) => completion,
                    Err(failure) => return Ok(task_failure_result(failure)),
                };
                Self::eval_task_payload(runtime_from_ct(completion, span)?, flatten_result, span)?
            }
            MirTaskGroupKind::Any => {
                let completion = match crate::scheduler::jet_scheduler_any(entries) {
                    Ok(completion) => completion,
                    Err(failure) => return Ok(task_failure_result(failure)),
                };
                Self::eval_task_payload(runtime_from_ct(completion, span)?, flatten_result, span)?
            }
        };
        let _ = result_ty;
        Ok(result)
    }

    fn materialize_runtime(
        &mut self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match value {
            RuntimeValue::Stream(handle) => {
                let mut values = Vec::new();
                while let Some(value) = mir_stream_pull_handle(&handle, self, span)? {
                    values.push(value);
                }
                Ok(RuntimeValue::Data(MirEvalValue::List(values)))
            }
            RuntimeValue::StreamCursor(_) => Err(mir_error_at(
                "interpreter stream iterator cursor cannot be materialized as a Stream",
                span,
            )),
            value => Ok(value),
        }
    }

    fn stream_cursor_has_next(
        &self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let RuntimeValue::StreamCursor(cursor) = value else {
            return Err(mir_error_at(
                "MIR stream cursor value is not a cursor",
                span,
            ));
        };
        let cursor = cursor
            .try_borrow()
            .map_err(|_| mir_error_at("interpreter stream cursor is already borrowed", span))?;
        Ok(RuntimeValue::Data(MirEvalValue::Bool(
            cursor.current.is_some(),
        )))
    }

    fn stream_cursor_value(
        &self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let RuntimeValue::StreamCursor(cursor) = value else {
            return Err(mir_error_at(
                "MIR stream cursor value is not a cursor",
                span,
            ));
        };
        let mut cursor = cursor
            .try_borrow_mut()
            .map_err(|_| mir_error_at("interpreter stream cursor is already borrowed", span))?;
        let value = cursor
            .current
            .take()
            .ok_or_else(|| mir_error_at("iterator loop value requested after exhaustion", span))?;
        Ok(RuntimeValue::Data(value))
    }

    fn stream_cursor_advance(
        &mut self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let RuntimeValue::StreamCursor(cursor) = value else {
            return Err(mir_error_at(
                "MIR stream cursor value is not a cursor",
                span,
            ));
        };
        let (source, step) = {
            let cursor = cursor
                .try_borrow()
                .map_err(|_| mir_error_at("interpreter stream cursor is already borrowed", span))?;
            (cursor.source.clone(), cursor.step)
        };
        let mut current = None;
        let mut exhausted = false;
        for _ in 1..step {
            if mir_stream_pull_handle(&source, self, span)?.is_none() {
                exhausted = true;
                break;
            }
        }
        if !exhausted {
            current = mir_stream_pull_handle(&source, self, span)?;
        }
        let mut cursor = cursor
            .try_borrow_mut()
            .map_err(|_| mir_error_at("interpreter stream cursor is already borrowed", span))?;
        cursor.current = current;
        Ok(RuntimeValue::Data(MirEvalValue::Unit))
    }

    fn result(
        &mut self,
        value: RuntimeValue,
        status: MirExecutionStatus,
        frame: Option<MirFrame>,
    ) -> Result<MirEvalResult, Diagnostic> {
        let mut value = runtime_to_data(
            self.materialize_runtime(value, Span::new(0, 0))?,
            Span::new(0, 0),
        )?;
        if let Some(artifact) = &self.artifact {
            value = match artifact.output {
                MirEntryOutput::None => MirEvalValue::Unit,
                MirEntryOutput::ReturnValue => value,
                MirEntryOutput::StandardOutput => {
                    self.stdout.push_str(&mir_show(&value));
                    self.stdout.push('\n');
                    MirEvalValue::Unit
                }
                MirEntryOutput::ExitStatus => MirEvalValue::Int(i64::from(self.exit_code)),
            };
        }
        Ok(MirEvalResult {
            value,
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
            exit_code: self.exit_code,
            status,
            frame,
            artifact: self.artifact.clone(),
        })
    }

    fn suspended_frame(&mut self) -> Result<MirFrame, Diagnostic> {
        let execution = self.execution.clone().ok_or_else(|| {
            mir_error(
                "MIR frame identity unavailable: evaluate_mir_function_with_config(program, function, args, config) has no artifact parameter",
                None,
            )
        })?;
        let mut states = Vec::with_capacity(self.frames.len());
        for frame in &self.frames {
            let sequence = self.sequence;
            self.sequence = self.sequence.wrapping_add(1);
            let identity = self
                .program
                .frame_identity(
                    Some(execution.artifact.artifact),
                    frame.function,
                    Some(frame.block),
                    sequence,
                )
                .map_err(|error| {
                    mir_error(&format!("MIR frame identity unavailable: {error}"), None)
                })?;
            states.push(frame_state(frame, self.program, Some(identity))?);
        }
        let Some(mut state) = states.pop() else {
            return Err(mir_error("MIR suspension has no frame", None));
        };
        state.call_stack = states;
        Ok(MirFrame {
            state,
            runtime: Some(RuntimeSnapshot {
                frames: self.frames.clone(),
            }),
        })
    }

    fn step(&mut self, frame_index: usize) -> Result<Action, Diagnostic> {
        let function_id = self.frames[frame_index].function;
        let (function_span, interpreter_applicable) = {
            let function = program_function(self.program, function_id)?;
            (function.span, function.target_applicability.interpreter)
        };
        if !interpreter_applicable {
            return Err(mir_error_at(
                "MIR function is not applicable to the interpreter target",
                function_span,
            ));
        }
        self.drain_pending_hardware_interrupts(function_span)?;
        let function = program_function(self.program, function_id)?;
        let block_id = self.frames[frame_index].block;
        let block = function
            .blocks
            .iter()
            .find(|candidate| candidate.id == block_id)
            .ok_or_else(|| mir_error_at("MIR frame targets a missing block", function.span))?;
        if self.frames[frame_index].ip < block.instructions.len() {
            let instruction = block.instructions[self.frames[frame_index].ip].clone();
            self.frames[frame_index].ip += 1;
            return self.execute_instruction(frame_index, &instruction);
        }
        self.execute_terminator(frame_index, &block.terminator)
    }

    fn execute_instruction(
        &mut self,
        frame_index: usize,
        instruction: &MirInstruction,
    ) -> Result<Action, Diagnostic> {
        let span = instruction.span;
        let result = match &instruction.operation {
            MirOperation::Call {
                callee: MirCallee::Prelude(call),
                args,
                ..
            } => {
                let values = self.call_args(frame_index, args, span)?;
                self.eval_prelude_runtime_values(*call, values, instruction.ty.as_ref(), span)?
            }
            MirOperation::Call {
                callee: MirCallee::Foreign(call),
                args,
                ..
            } => {
                let values = self.call_args(frame_index, args, span)?;
                self.eval_foreign_call(*call, values, instruction.ty.as_ref(), span)?
            }
            MirOperation::Call {
                callee: MirCallee::Indirect(callee),
                args,
                ..
            } => {
                let closure = self.value(frame_index, *callee, span)?;
                let values = self.call_args(frame_index, args, span)?;
                if let RuntimeValue::Ambient(value) = closure {
                    self.invoke_ambient_callback(value, values, span)?
                } else {
                    let (function, captures, capture_cells) = self.closure_parts(closure, span)?;
                    return Ok(Action::Call {
                        function,
                        args: values,
                        captures,
                        capture_cells,
                        result: instruction.result,
                    });
                }
            }
            MirOperation::Call { callee, args, .. } => {
                let values = self.call_args(frame_index, args, span)?;
                let (function, captures, capture_cells) =
                    self.resolve_callee(frame_index, callee, &values, span)?;
                return Ok(Action::Call {
                    function,
                    args: values,
                    captures,
                    capture_cells,
                    result: instruction.result,
                });
            }
            MirOperation::CoreCall {
                call,
                route,
                args,
                type_args,
                fallibility: _,
                data_plan,
                ..
            } => {
                let values = self.call_args(frame_index, args, span)?;
                self.eval_core_call(
                    *call,
                    *route,
                    values,
                    type_args,
                    instruction.ty.as_ref(),
                    data_plan.as_ref(),
                    span,
                )?
            }
            MirOperation::IndirectCall { callee, args, .. } => {
                let closure = self.value(frame_index, *callee, span)?;
                let values = self.call_args(frame_index, args, span)?;
                if let RuntimeValue::Ambient(value) = closure {
                    self.invoke_ambient_callback(value, values, span)?
                } else {
                    let (function, captures, capture_cells) = self.closure_parts(closure, span)?;
                    return Ok(Action::Call {
                        function,
                        args: values,
                        captures,
                        capture_cells,
                        result: instruction.result,
                    });
                }
            }
            operation => {
                self.eval_operation(frame_index, operation, instruction.ty.as_ref(), span)?
            }
        };
        if let Some(result_id) = instruction.result {
            self.frames[frame_index].values.insert(result_id, result);
            self.record_value_alias(frame_index, result_id, &instruction.operation, span)?;
        }
        Ok(Action::Continue)
    }

    fn record_value_alias(
        &mut self,
        frame_index: usize,
        result_id: MirValueId,
        operation: &MirOperation,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let alias = match operation {
            MirOperation::ReadPlace(place) => self.alias_for_place(frame_index, *place, span)?,
            MirOperation::Field { base, field } => self.frames[frame_index]
                .value_aliases
                .get(base)
                .cloned()
                .map(|mut alias| {
                    let name = field_name(self.program, *field)
                        .ok_or_else(|| mir_error_at("MIR field ID is missing", span))?;
                    alias.steps.push(MoveStep::Field {
                        field: *field,
                        name,
                    });
                    Ok(alias)
                })
                .transpose()?,
            MirOperation::ProjectMembers { base, members } => {
                let mut alias = self.frames[frame_index].value_aliases.get(base).cloned();
                if let Some(alias_value) = alias.as_mut() {
                    for field in members {
                        let name = field_name(self.program, *field)
                            .ok_or_else(|| mir_error_at("MIR field ID is missing", span))?;
                        alias_value.steps.push(MoveStep::Field {
                            field: *field,
                            name,
                        });
                    }
                }
                alias
            }
            MirOperation::Index { kind, .. } if *kind == MirIndexKind::Pool => None,
            MirOperation::Index {
                base, index, kind, ..
            } => self.frames[frame_index]
                .value_aliases
                .get(base)
                .cloned()
                .map(|mut alias| {
                    let index = runtime_to_data(self.value(frame_index, *index, span)?, span)?;
                    alias.steps.push(MoveStep::Index {
                        value: index,
                        kind: *kind,
                    });
                    Ok(alias)
                })
                .transpose()?,
            MirOperation::Deref { value } => match self.frames[frame_index].values.get(value) {
                Some(RuntimeValue::Address(address)) => Some(ValueAlias {
                    frame: address.frame,
                    place: address.place,
                    steps: Vec::new(),
                }),
                _ => self.frames[frame_index].value_aliases.get(value).cloned(),
            },
            _ => None,
        };
        let alias = alias.or_else(|| match self.frames[frame_index].values.get(&result_id) {
            Some(RuntimeValue::Address(address)) => Some(ValueAlias {
                frame: address.frame,
                place: address.place,
                steps: Vec::new(),
            }),
            _ => None,
        });
        if let Some(alias) = alias {
            self.frames[frame_index]
                .value_aliases
                .insert(result_id, alias);
        }
        Ok(())
    }

    fn alias_for_place(
        &self,
        frame_index: usize,
        place_id: MirPlaceId,
        span: Span,
    ) -> Result<Option<ValueAlias>, Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        let place = function
            .places
            .iter()
            .find(|place| place.id == place_id)
            .ok_or_else(|| mir_error_at("MIR place is unavailable", span))?;
        match &place.base {
            MirPlaceBase::Temporary(source) | MirPlaceBase::Parameter(source) => {
                if let Some(mut alias) = self.frames[frame_index].value_aliases.get(source).cloned()
                {
                    alias.steps.extend(self.move_steps_for_frame(
                        frame_index,
                        &place.projections,
                        span,
                    )?);
                    Ok(Some(alias))
                } else {
                    Ok(Some(ValueAlias {
                        frame: frame_index,
                        place: place_id,
                        steps: Vec::new(),
                    }))
                }
            }
            MirPlaceBase::Local(_) | MirPlaceBase::Capture(_) | MirPlaceBase::Static(_) => {
                Ok(Some(ValueAlias {
                    frame: frame_index,
                    place: place_id,
                    steps: Vec::new(),
                }))
            }
        }
    }

    fn execute_terminator(
        &mut self,
        frame_index: usize,
        terminator: &MirTerminator,
    ) -> Result<Action, Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        let span = function.span;
        match terminator {
            MirTerminator::Jump { target } => {
                self.run_drops(frame_index, DropEdge::Normal, span)?;
                self.jump(frame_index, *target, span)?;
                Ok(Action::Continue)
            }
            MirTerminator::Branch {
                condition,
                then_target,
                else_target,
            } => {
                self.run_drops(frame_index, DropEdge::Normal, span)?;
                let condition = self.bool_value(frame_index, *condition, span)?;
                self.jump(
                    frame_index,
                    if condition {
                        *then_target
                    } else {
                        *else_target
                    },
                    span,
                )?;
                Ok(Action::Continue)
            }
            MirTerminator::Switch {
                subject,
                arms,
                otherwise,
            } => {
                self.run_drops(frame_index, DropEdge::Normal, span)?;
                let _ = self.value(frame_index, *subject, span)?;
                let mut target = *otherwise;
                for arm in arms {
                    if self.bool_value(frame_index, arm.condition, arm.span)? {
                        target = arm.target;
                        break;
                    }
                }
                self.jump(frame_index, target, span)?;
                Ok(Action::Continue)
            }
            MirTerminator::Return { value } => {
                let value = value
                    .map(|value| self.value(frame_index, value, span))
                    .transpose()?;
                // A block-bodied fallible Unit function has an implicit
                // successful return: the same Ok(()) carrier the JIT and AOT
                // return, so the caller's result test sees a carrier.
                let value = match value {
                    Some(value) => value,
                    None if matches!(
                        &function.failure,
                        MirFailureCarrier::Result { success, .. } if success.is_unit()
                    ) =>
                    {
                        RuntimeValue::Data(MirEvalValue::Present(Box::new(MirEvalValue::Unit)))
                    }
                    None => RuntimeValue::Data(MirEvalValue::Unit),
                };
                self.run_drops(frame_index, DropEdge::Return, span)?;
                Ok(Action::Return(value))
            }
            MirTerminator::Yield { value, resume } => {
                self.run_drops(frame_index, DropEdge::Normal, span)?;
                let value = self.value(frame_index, *value, span)?;
                self.jump(frame_index, *resume, span)?;
                Ok(Action::Suspend(value))
            }
            MirTerminator::Break { target, value } => {
                self.run_drops(frame_index, DropEdge::Normal, span)?;
                let value = value
                    .map(|value| self.value(frame_index, value, span))
                    .transpose()?
                    .unwrap_or(RuntimeValue::Data(MirEvalValue::Unit));
                self.frames[frame_index].pending_break = Some(value);
                self.jump(frame_index, *target, span)?;
                Ok(Action::Continue)
            }
            MirTerminator::Continue { target } => {
                self.run_drops(frame_index, DropEdge::Normal, span)?;
                self.jump(frame_index, *target, span)?;
                Ok(Action::Continue)
            }
            MirTerminator::Unreachable { reason } => Err(mir_error_at(
                &format!("MIR reached an unreachable terminator: {reason}"),
                span,
            )),
        }
    }
    fn eval_plain_list_cursor(
        &mut self,
        values: Vec<MirEvalValue>,
        step_value: Option<MirEvalValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let step = step_value.map(|value| int_value(value, span)).transpose()?;
        let cursor = crate::Comptime::CollectionEval::LoopListCursor::new(values, step).map_err(
            |message| {
                if self.config.runtime_execution {
                    self.located_runtime_stop("E3001", "<core.prelude>", 0, message, span)
                } else {
                    crate::Comptime::comptime_panic(message, span)
                }
            },
        )?;
        let source = Rc::new(RefCell::new(MirInterpreterStream::List { cursor }));
        let current = mir_stream_pull_handle(&source, self, span)?;
        Ok(RuntimeValue::StreamCursor(Rc::new(RefCell::new(
            MirInterpreterStreamCursor {
                source,
                current,
                step: 1,
            },
        ))))
    }

    fn eval_operation(
        &mut self,
        frame_index: usize,
        operation: &MirOperation,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match operation {
            MirOperation::Parameter { index, .. } => self.frames[frame_index]
                .params
                .get(*index)
                .cloned()
                .ok_or_else(|| mir_error_at("MIR parameter index is outside the call frame", span)),
            MirOperation::Capture { slot } => {
                let cell = self.frames[frame_index]
                    .capture_cells
                    .get(*slot)
                    .and_then(Option::as_ref)
                    .cloned();
                cell.map(|cell| cell.borrow().clone())
                    .or_else(|| self.frames[frame_index].captures.get(*slot).cloned())
                    .ok_or_else(|| {
                        mir_error_at("MIR capture slot is outside the closure environment", span)
                    })
            }
            MirOperation::Global { name } if name == "stm" => {
                self.begin_shared_transaction(frame_index, span)
            }
            MirOperation::Global { name } => self
                .config
                .globals
                .get(name)
                .cloned()
                .map(RuntimeValue::Data)
                .ok_or_else(|| {
                    mir_error_at(
                        &format!("MIR global `{name}` is not provided by the run"),
                        span,
                    )
                }),
            MirOperation::Phi { incoming } => {
                let predecessor = self.frames[frame_index].predecessor;
                let value = incoming
                    .iter()
                    .find(|(block, _)| Some(*block) == predecessor)
                    .map(|(_, value)| *value)
                    .ok_or_else(|| {
                        mir_error_at("MIR phi has no incoming value for predecessor", span)
                    })?;
                self.value(frame_index, value, span)
            }
            MirOperation::ReadPlace(place) => self.read_place(frame_index, *place, span),
            MirOperation::MovePlace { place } => self.move_place(frame_index, *place, span),
            MirOperation::InitializeUninit { place } => {
                let length = {
                    let function = self.function_row(self.frames[frame_index].function)?;
                    let place_row = function
                        .places
                        .iter()
                        .find(|candidate| candidate.id == *place)
                        .ok_or_else(|| mir_error_at("MIR place is missing", span))?;
                    match place_row.ty.kind() {
                        MirTypeKind::FixedList { len, .. } => Some(
                            len.literal_value()
                                .and_then(|len| usize::try_from(len).ok())
                                .ok_or_else(|| {
                                    mir_error_at(
                                        "MIR uninitialized fixed-list local has no static length",
                                        span,
                                    )
                                })?,
                        ),
                        _ => None,
                    }
                };
                let value = length.map_or(RuntimeValue::Moved, |len| {
                    RuntimeValue::Data(MirEvalValue::List(vec![MirEvalValue::Moved; len]))
                });
                self.write_place(frame_index, *place, value, span)?;
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            MirOperation::WritePlace { place, value } => {
                let value = self.value(frame_index, *value, span)?;
                self.write_place(frame_index, *place, value, span)?;
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            MirOperation::Copy { value } => {
                let mut value = self.value(frame_index, *value, span)?;
                fork_runtime_value_owners(&mut value);
                Ok(value)
            }
            MirOperation::Move { value } => {
                let value = self.frames[frame_index]
                    .values
                    .remove(value)
                    .ok_or_else(|| mir_error_at("MIR move reads an unavailable SSA value", span))?;
                if runtime_contains_moved(&value) {
                    return Err(mir_error_at("MIR value was moved", span));
                }
                Ok(value)
            }
            MirOperation::Constant(constant) => mir_constant_to_runtime(constant, span),
            MirOperation::Unary { op, value } => {
                let value = runtime_to_data(self.value(frame_index, *value, span)?, span)?;
                Ok(RuntimeValue::Data(match op {
                    MirUnaryOp::Neg => match value {
                        MirEvalValue::Int(value) => MirEvalValue::Int(
                            value
                                .checked_neg()
                                .ok_or_else(|| mir_error_at("integer negation overflow", span))?,
                        ),
                        MirEvalValue::BigInt(value) => {
                            MirEvalValue::BigInt(mir_bigint(&value, span)?.neg().to_string_rep())
                        }
                        MirEvalValue::Float { .. } => mir_float_data(
                            mir_float_from_data(&value).expect("float carrier").neg(),
                        ),
                        _ => {
                            return Err(mir_error_at("MIR unary negation requires a number", span))
                        }
                    },
                    MirUnaryOp::Not => match value {
                        MirEvalValue::Bool(value) => MirEvalValue::Bool(!value),
                        _ => return Err(mir_error_at("MIR logical negation requires Bool", span)),
                    },
                }))
            }
            MirOperation::Binary {
                op,
                dispatch,
                left,
                right,
            } => {
                let left = runtime_to_data(self.value(frame_index, *left, span)?, span)?;
                let right = runtime_to_data(self.value(frame_index, *right, span)?, span)?;
                match dispatch {
                    MirBinaryDispatch::Primitive => {
                        Ok(RuntimeValue::Data(eval_mir_binary(*op, left, right, span)?))
                    }
                    MirBinaryDispatch::Prelude { call, location } => {
                        let args = self.binary_prelude_args(*call, left, right, *location, span)?;
                        self.eval_prelude(*call, args, result_ty, span)
                            .map(RuntimeValue::Data)
                    }
                }
            }
            MirOperation::BuildString { parts } => {
                let mut output = String::new();
                for part in parts {
                    match part {
                        MirStringPart::Literal(value) => output.push_str(value),
                        MirStringPart::Value(value) => output.push_str(&mir_show(
                            &runtime_to_data(self.value(frame_index, *value, span)?, span)?,
                        )),
                    }
                }
                Ok(RuntimeValue::Data(MirEvalValue::String(output)))
            }
            MirOperation::BuildList { values } => {
                let values = values
                    .iter()
                    .map(|value| self.value(frame_index, *value, span))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                let data = values
                    .iter()
                    .cloned()
                    .map(|value| runtime_to_data(value, span))
                    .collect::<Result<Vec<_>, Diagnostic>>();
                if let Ok(data) = data {
                    return Ok(RuntimeValue::Data(MirEvalValue::List(data)));
                }
                let values = values
                    .into_iter()
                    .map(|value| self.runtime_to_ct(value, span))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                runtime_from_ct(CtValue::List(values), span)
            }
            MirOperation::BuildMap { entries } => {
                let mut output = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    let key = data_key(
                        &runtime_to_data(self.value(frame_index, *key, span)?, span)?,
                        span,
                    )?;
                    let value = runtime_to_data(self.value(frame_index, *value, span)?, span)?;
                    if let Some((_, existing)) =
                        output.iter_mut().find(|(existing, _)| *existing == key)
                    {
                        *existing = value;
                    } else {
                        output.push((key, value));
                    }
                }
                Ok(RuntimeValue::Data(MirEvalValue::Map(output)))
            }
            MirOperation::ProjectMembers { base, members } => {
                let mut value = self.value(frame_index, *base, span)?;
                for member in members {
                    value = self.project_field_id(value, *member, span)?;
                }
                Ok(value)
            }
            MirOperation::Index {
                call,
                base,
                index,
                kind,
                access: _,
                location,
                context,
            } => {
                if *kind == MirIndexKind::Pool {
                    let base = self.value(frame_index, *base, span)?;
                    let index = self.value(frame_index, *index, span)?;
                    return self.eval_pool_index(base, index, *location, Some(context), span);
                }
                let args = self.index_prelude_args(
                    *call,
                    runtime_to_data(self.value(frame_index, *base, span)?, span)?,
                    runtime_to_data(self.value(frame_index, *index, span)?, span)?,
                    *location,
                    Some(context),
                    span,
                )?;
                self.eval_prelude(*call, args, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirOperation::Slice {
                call,
                base,
                start,
                end,
                range,
                location,
            } => {
                let path = self.source_file_path(location.file, span)?;
                let args = match range {
                    Some(range) => vec![
                        runtime_to_data(self.value(frame_index, *base, span)?, span)?,
                        runtime_to_data(self.value(frame_index, *range, span)?, span)?,
                        path,
                        MirEvalValue::Int(i64::from(location.line)),
                    ],
                    None => vec![
                        runtime_to_data(self.value(frame_index, *base, span)?, span)?,
                        runtime_to_data(self.value(frame_index, *start, span)?, span)?,
                        runtime_to_data(self.value(frame_index, *end, span)?, span)?,
                        path,
                        MirEvalValue::Int(i64::from(location.line)),
                    ],
                };
                self.eval_prelude(*call, args, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirOperation::Range {
                start,
                end,
                exclusive,
            } => {
                let start = runtime_to_data(self.value(frame_index, *start, span)?, span)?;
                let end = runtime_to_data(self.value(frame_index, *end, span)?, span)?;
                Ok(RuntimeValue::Data(MirEvalValue::Struct {
                    type_name: crate::Syntax::TYPE_RANGE.to_string(),
                    fields: vec![
                        ("start".to_string(), start),
                        ("end".to_string(), end),
                        ("exclusive".to_string(), MirEvalValue::Bool(*exclusive)),
                    ],
                }))
            }
            MirOperation::Field { base, field } => {
                self.project_field_id(self.value(frame_index, *base, span)?, *field, span)
            }
            MirOperation::Struct { type_id, fields } => {
                let type_name = type_instance_name(self.program, *type_id, span)?;
                let fields = fields
                    .iter()
                    .map(|(field, value)| Ok((*field, self.value(frame_index, *value, span)?)))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                self.aggregate_or_data(type_name, fields, span)
            }
            MirOperation::Enum {
                type_id,
                variant,
                args,
            } => {
                let type_name = type_instance_name(self.program, *type_id, span)?;
                validate_enum_args(
                    self.program,
                    self.frames[frame_index].function,
                    *type_id,
                    variant,
                    args,
                    span,
                )?;
                let args = args
                    .iter()
                    .map(|arg| {
                        let field = arg
                            .field
                            .map(|field| {
                                field_name(self.program, field).ok_or_else(|| {
                                    mir_error_at("MIR enum argument field ID is missing", span)
                                })
                            })
                            .transpose()?;
                        Ok((
                            field,
                            runtime_to_data(self.value(frame_index, arg.value, span)?, span)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                Ok(RuntimeValue::Data(MirEvalValue::Enum {
                    type_name,
                    variant: variant.clone(),
                    args,
                }))
            }
            MirOperation::EnumIs {
                subject,
                owner,
                variant,
            } => {
                let value = runtime_to_data(self.value(frame_index, *subject, span)?, span)?;
                let owner = type_instance_name(self.program, *owner, span)?;
                let matched = matches!(
                    &value,
                    MirEvalValue::Enum {
                        type_name,
                        variant: actual,
                        ..
                    } if type_name == &owner && variant_name_matches(actual, variant),
                );
                Ok(RuntimeValue::Data(MirEvalValue::Bool(matched)))
            }
            MirOperation::EnumPayload {
                subject,
                owner,
                variant,
                index,
            } => {
                let value = runtime_to_data(self.value(frame_index, *subject, span)?, span)?;
                let owner = type_instance_name(self.program, *owner, span)?;
                let MirEvalValue::Enum {
                    type_name,
                    variant: actual,
                    args,
                } = value
                else {
                    return Err(mir_error_at(
                        "MIR enum payload requires an enum value",
                        span,
                    ));
                };
                if type_name != owner || !variant_name_matches(&actual, variant) {
                    return Err(mir_error_at(
                        "MIR enum payload variant does not match",
                        span,
                    ));
                }
                let (_, value) = args
                    .get(*index)
                    .ok_or_else(|| mir_error_at("MIR enum payload index is out of range", span))?;
                Ok(RuntimeValue::Data(value.clone()))
            }
            MirOperation::OptionIsSome { subject } => {
                let value = self.value(frame_index, *subject, span)?;
                let present = match value {
                    RuntimeValue::Absent { .. }
                    | RuntimeValue::Data(MirEvalValue::Absent { .. })
                    | RuntimeValue::Data(MirEvalValue::FailedTold(_))
                    | RuntimeValue::Ambient(CtValue::Failed(_)) => false,
                    RuntimeValue::Data(MirEvalValue::Present(_))
                    | RuntimeValue::Ambient(CtValue::Present(_)) => true,
                    RuntimeValue::Game(MirGameValue::Optional { present, .. }) => present,
                    RuntimeValue::Result { ok, .. } => ok,
                    _ => {
                        return Err(mir_error_at(
                            "MIR option test requires an option outcome carrier",
                            span,
                        ));
                    }
                };
                Ok(RuntimeValue::Data(MirEvalValue::Bool(present)))
            }
            MirOperation::OptionValue { subject } => {
                let value = self.value(frame_index, *subject, span)?;
                match value {
                    RuntimeValue::Ambient(CtValue::Present(value)) => runtime_from_ct(*value, span),
                    RuntimeValue::Data(MirEvalValue::Present(value)) => {
                        Ok(RuntimeValue::Data(*value))
                    }
                    RuntimeValue::Game(MirGameValue::Optional {
                        present: true,
                        value: Some(value),
                    }) => Ok(*value),
                    RuntimeValue::Result { ok: true, value } => Ok(*value),
                    _ => Err(mir_error_at("MIR option value requires Present", span)),
                }
            }
            MirOperation::ResultIsOk { subject } => {
                let value = self.value(frame_index, *subject, span)?;
                let ok = match value {
                    RuntimeValue::Result { ok, .. } => ok,
                    RuntimeValue::Data(MirEvalValue::Present(_))
                    | RuntimeValue::Ambient(CtValue::Present(_)) => true,
                    RuntimeValue::Data(MirEvalValue::Absent { .. })
                    | RuntimeValue::Data(MirEvalValue::FailedTold(_))
                    | RuntimeValue::Ambient(CtValue::Failed(_))
                    | RuntimeValue::Absent { .. } => false,
                    _ => {
                        return Err(mir_error_at(
                            "MIR result test requires a result outcome carrier",
                            span,
                        ));
                    }
                };
                Ok(RuntimeValue::Data(MirEvalValue::Bool(ok)))
            }
            MirOperation::ResultValue { subject, ok } => {
                let value = self.value(frame_index, *subject, span)?;
                match (*ok, value) {
                    (true, RuntimeValue::Result { ok: true, value })
                    | (false, RuntimeValue::Result { ok: false, value }) => Ok(*value),
                    (true, RuntimeValue::Ambient(CtValue::Present(value))) => {
                        runtime_from_ct(*value, span)
                    }
                    (false, RuntimeValue::Ambient(value)) => {
                        let report = value.told_report().ok_or_else(|| {
                            mir_error_at("MIR result value requires a told report", span)
                        })?;
                        runtime_from_ct(report.clone(), span)
                    }
                    (true, RuntimeValue::Data(MirEvalValue::Present(value))) => {
                        if result_ty.is_some_and(|ty| ty.nominal_name() == Some("ProcessChild")) {
                            let MirEvalValue::Int(raw) = *value else {
                                return Err(mir_error_at(
                                    "MIR process child result payload is not an opaque handle",
                                    span,
                                ));
                            };
                            if raw == 0 {
                                return Err(mir_error_at(
                                    "MIR ProcessChild result has an invalid identity",
                                    span,
                                ));
                            }
                            let handle = result_ty
                                .and_then(|ty| self.handle_id_for_type(ty))
                                .ok_or_else(|| {
                                    mir_error_at(
                                        "MIR ProcessChild result has no canonical lifecycle row",
                                        span,
                                    )
                                })?;
                            Ok(RuntimeValue::ForeignHandle {
                                token: MirHandleToken::new(handle, raw),
                            })
                        } else {
                            Ok(RuntimeValue::Data(*value))
                        }
                    }
                    (false, RuntimeValue::Data(MirEvalValue::FailedTold(value))) => {
                        Ok(RuntimeValue::Data(*value))
                    }
                    (true, _) => Err(mir_error_at("MIR result value requires Ok", span)),
                    (false, _) => Err(mir_error_at("MIR result value requires Err", span)),
                }
            }
            MirOperation::PatternMatched { matched } => {
                let value = self.value(frame_index, *matched, span)?;
                let matched = match value {
                    RuntimeValue::Absent { .. } => false,
                    RuntimeValue::Data(MirEvalValue::Present(value)) => {
                        let MirEvalValue::Struct { type_name, fields } = *value else {
                            return Err(mir_error_at(
                                "MIR pattern match carrier payload is not a typed capture tuple",
                                span,
                            ));
                        };
                        if !type_name.starts_with('(')
                            || fields
                                .iter()
                                .enumerate()
                                .any(|(index, (name, _))| name != &index.to_string())
                        {
                            return Err(mir_error_at(
                                "MIR pattern match carrier payload is not a canonical capture tuple",
                                span,
                            ));
                        }
                        true
                    }
                    _ => {
                        return Err(mir_error_at(
                            "MIR pattern match requires its canonical outcome carrier",
                            span,
                        ));
                    }
                };
                Ok(RuntimeValue::Data(MirEvalValue::Bool(matched)))
            }
            MirOperation::PatternCapture { matched, index } => {
                let value = self.value(frame_index, *matched, span)?;
                let RuntimeValue::Data(MirEvalValue::Present(value)) = value else {
                    return Err(mir_error_at(
                        "MIR pattern capture requires a matched carrier",
                        span,
                    ));
                };
                let MirEvalValue::Struct { type_name, fields } = *value else {
                    return Err(mir_error_at(
                        "MIR pattern match carrier is not a typed capture tuple",
                        span,
                    ));
                };
                if !type_name.starts_with('(')
                    || fields
                        .iter()
                        .enumerate()
                        .any(|(position, (name, _))| name != &position.to_string())
                {
                    return Err(mir_error_at(
                        "MIR pattern match carrier is not a canonical capture tuple",
                        span,
                    ));
                }
                let value = fields
                    .get(*index)
                    .ok_or_else(|| mir_error_at("MIR pattern capture index is out of range", span))?
                    .1
                    .clone();
                let result_ty = result_ty.ok_or_else(|| {
                    mir_error_at("MIR pattern capture has no extraction type", span)
                })?;
                Ok(RuntimeValue::Data(marshal_pattern_capture(
                    value,
                    result_ty.kind(),
                    span,
                )?))
            }
            MirOperation::Tuple { type_id, fields } => {
                let type_name = type_instance_name(self.program, *type_id, span)?;
                let fields = fields
                    .iter()
                    .map(|(field, value)| Ok((*field, self.value(frame_index, *value, span)?)))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                self.aggregate_or_data(type_name, fields, span)
            }
            MirOperation::Semantic(operation) => {
                Ok(self.eval_semantic(frame_index, operation, result_ty, span)?)
            }
            MirOperation::Present { value } => {
                let value = self.value(frame_index, *value, span)?;
                match value {
                    RuntimeValue::Game(value) => Ok(RuntimeValue::Game(MirGameValue::Optional {
                        present: true,
                        value: Some(Box::new(RuntimeValue::Game(value))),
                    })),
                    value @ (RuntimeValue::Ambient(_)
                    | RuntimeValue::Closure(_)
                    | RuntimeValue::Data(MirEvalValue::Closure(_))
                    | RuntimeValue::Aggregate(_)) => {
                        let value = self.runtime_to_ct(value, span)?;
                        runtime_from_ct(CtValue::Present(Box::new(value)), span)
                    }
                    value => Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(
                        runtime_to_data(value, span)?,
                    )))),
                }
            }
            MirOperation::Absent => {
                let result_ty = result_ty.ok_or_else(|| {
                    mir_error_at("MIR absent operation has no result type fact", span)
                })?;
                let element = result_ty
                    .option_inner()
                    .ok_or_else(|| {
                        mir_error_at("MIR absent operation result is not an Option", span)
                    })?
                    .clone();
                Ok(RuntimeValue::Absent { element })
            }
            MirOperation::ResultOk { value } => {
                let value = self.value(frame_index, *value, span)?;
                Ok(RuntimeValue::Result {
                    ok: true,
                    value: Box::new(value),
                })
            }
            MirOperation::ResultErr { value } => {
                let value = self.value(frame_index, *value, span)?;
                Ok(RuntimeValue::Result {
                    ok: false,
                    value: Box::new(value),
                })
            }
            MirOperation::Closure {
                function,
                captures,
                facts,
            } => {
                let target = program_function(self.program, *function)?;
                let source = program_function(self.program, self.frames[frame_index].function)?;
                let mut capture_cells = vec![None; captures.len()];
                let captures = captures
                    .iter()
                    .enumerate()
                    .map(|(slot, operand)| {
                        let parameter = target.capture_params.get(slot).ok_or_else(|| {
                            mir_error_at(
                                "MIR closure capture operand has no declared target slot",
                                span,
                            )
                        })?;
                        match (operand, parameter.access) {
                            (MirCaptureOperand::Value(value), MirAccess::Move) => self.frames
                                [frame_index]
                                .values
                                .remove(value)
                                .ok_or_else(|| {
                                    mir_error_at("MIR moved closure capture is unavailable", span)
                                }),
                            (
                                MirCaptureOperand::Value(value),
                                MirAccess::Read | MirAccess::Write,
                            ) if matches!(parameter.ownership.mode, MirOwnershipMode::Owned) => {
                                let captured = self.frames[frame_index]
                                    .values
                                    .remove(value)
                                    .ok_or_else(|| {
                                        mir_error_at(
                                            "MIR owned closure capture is unavailable",
                                            span,
                                        )
                                    })?;
                                capture_cells[slot] = Some(Rc::new(RefCell::new(captured.clone())));
                                Ok(captured)
                            }
                            (MirCaptureOperand::Place(place), MirAccess::Read)
                            | (MirCaptureOperand::Place(place), MirAccess::Write) => {
                                if !source.places.iter().any(|candidate| candidate.id == *place) {
                                    return Err(mir_error_at(
                                        "MIR captured place ID is missing",
                                        span,
                                    ));
                                }
                                Ok(RuntimeValue::Address(Address {
                                    frame: frame_index,
                                    place: *place,
                                    access: parameter.access,
                                }))
                            }
                            (MirCaptureOperand::Value(_), MirAccess::Read)
                            | (MirCaptureOperand::Value(_), MirAccess::Write)
                            | (MirCaptureOperand::Place(_), MirAccess::Move) => Err(mir_error_at(
                                "MIR closure capture operand does not match target access",
                                parameter.span,
                            )),
                        }
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                validate_captures(target, &captures)?;
                let expected = target.captures.clone().unwrap_or_default();
                if !capture_facts_equal(facts, &expected) {
                    return Err(mir_error_at(
                        "MIR closure operation facts disagree with its target function row",
                        span,
                    ));
                }
                Ok(RuntimeValue::Closure(Rc::new(MirClosure {
                    function: target.id,
                    captures,
                    capture_cells,
                    facts: facts.clone(),
                })))
            }
            MirOperation::PtrFromAddr { addr, .. } => self.value(frame_index, *addr, span),
            MirOperation::Deref { value } => {
                let value = self.value(frame_index, *value, span)?;
                match value {
                    RuntimeValue::SharedGuard(guard) => self.deref_shared_guard(guard, span),
                    RuntimeValue::SharedCell(cell) => {
                        if let Some(storage) = &cell.guard.storage {
                            storage.refresh_scalar_shadow(span)?;
                        }
                        Ok(RuntimeValue::Data(shared_payload_read(
                            &cell.guard.payload,
                            &cell.path,
                            span,
                        )?))
                    }
                    RuntimeValue::CellGuard(guard) => Ok(RuntimeValue::Data(cell_payload_read(
                        self.program,
                        &guard.payload,
                        &guard.path,
                        span,
                    )?)),
                    RuntimeValue::Address(address) => {
                        require_address_access(&address, MirAccess::Read, span)?;
                        self.read_place(address.frame, address.place, span)
                    }
                    RuntimeValue::Moved
                    | RuntimeValue::Data(_)
                    | RuntimeValue::Atomic(_)
                    | RuntimeValue::Stream(_)
                    | RuntimeValue::StreamCursor(_)
                    | RuntimeValue::Absent { .. }
                    | RuntimeValue::Closure(_)
                    | RuntimeValue::App(_)
                    | RuntimeValue::Ambient(_)
                    | RuntimeValue::ForeignHandle { .. }
                    | RuntimeValue::ProcessSpec { .. }
                    | RuntimeValue::DmaTransfer { .. }
                    | RuntimeValue::Game(_)
                    | RuntimeValue::Result { .. }
                    | RuntimeValue::Aggregate(_)
                    | RuntimeValue::SharedSnapshot(_)
                    | RuntimeValue::SharedTransaction(_)
                    | RuntimeValue::GcRoot(_) => Err(mir_error_at(
                        "MIR dereference requires an address or guard",
                        span,
                    )),
                }
            }
            MirOperation::AddressOf { place, access } => match access {
                MirAccess::Read => self.read_place(frame_index, *place, span),
                MirAccess::Write | MirAccess::Move => Ok(RuntimeValue::Address(Address {
                    frame: frame_index,
                    place: *place,
                    access: *access,
                })),
            },
            MirOperation::RawAddressOf { place } => Ok(RuntimeValue::Address(Address {
                frame: frame_index,
                place: *place,
                access: MirAccess::Read,
            })),
            MirOperation::Convert {
                value,
                parameters,
                target,
                conversion,
            } => {
                let runtime_value = self.value(frame_index, *value, span)?;
                match conversion {
                    MirConversion::Transparent => {
                        if !parameters.is_empty() {
                            return Err(mir_error_at(
                                "transparent MIR conversion has parameters",
                                span,
                            ));
                        }
                        Ok(RuntimeValue::Data(runtime_to_data(runtime_value, span)?))
                    }
                    MirConversion::NumericCast => {
                        if !parameters.is_empty() {
                            return Err(mir_error_at("numeric cast has parameters", span));
                        }
                        let source_ty = self.value_type(frame_index, *value, span)?;
                        let value = runtime_to_data(runtime_value, span)?;
                        Ok(RuntimeValue::Data(mir_numeric_cast_value(
                            value, &source_ty, target, span,
                        )?))
                    }
                    MirConversion::SendFn => {
                        let source_ty = self.value_type(frame_index, *value, span)?;
                        if !matches!(source_ty.kind(), MirTypeKind::SendFn { .. })
                            || !parameters.is_empty()
                            || !matches!(target.kind, MirTypeKind::SendFn { .. })
                        {
                            return Err(mir_error_at(
                                "SendFn conversion does not carry a checked SendFn callable",
                                span,
                            ));
                        }
                        match &runtime_value {
                            RuntimeValue::Closure(_)
                            | RuntimeValue::Data(MirEvalValue::Closure(_)) => {
                                let _ = self.closure_handle(runtime_value.clone(), span)?;
                                Ok(runtime_value)
                            }
                            _ => Err(mir_error_at(
                                "SendFn conversion operand is not callable",
                                span,
                            )),
                        }
                    }
                    MirConversion::Prelude {
                        call,
                        location,
                        fallibility: _,
                    } => {
                        let value = runtime_to_data(runtime_value, span)?;
                        let mut args = vec![value];
                        args.extend(
                            parameters
                                .iter()
                                .map(|parameter| self.value(frame_index, *parameter, span))
                                .collect::<Result<Vec<_>, Diagnostic>>()?
                                .into_iter()
                                .map(|value| runtime_to_data(value, span))
                                .collect::<Result<Vec<_>, Diagnostic>>()?,
                        );
                        let arity = self
                            .program
                            .prelude_calls
                            .iter()
                            .find(|row| row.id == *call)
                            .ok_or_else(|| {
                                mir_error_at(
                                    "MIR conversion PreludeCallId has no canonical row",
                                    span,
                                )
                            })?
                            .signature
                            .arity;
                        match arity.checked_sub(args.len()) {
                            Some(0) => {}
                            Some(2) => {
                                args.push(self.source_file_path(location.file, span)?);
                                args.push(MirEvalValue::Int(i64::from(location.line)));
                            }
                            _ => {
                                return Err(mir_error_at(
                                    "MIR conversion arguments do not match the Prelude row",
                                    span,
                                ));
                            }
                        }
                        self.eval_conversion_prelude(*call, args, target, span)
                            .map(RuntimeValue::Data)
                    }
                }
            }
            MirOperation::AttachTag { value, .. } => self.value(frame_index, *value, span),
            MirOperation::Todo {
                call,
                location,
                expected_type,
            } => {
                let expected_type = expected_type
                    .as_ref()
                    .ok_or_else(|| mir_error_at("MIR Todo has no checked expected type", span))?;
                let args = vec![
                    self.source_file_path(location.file, span)?,
                    MirEvalValue::Int(i64::from(location.line)),
                    MirEvalValue::String(expected_type.display_name()),
                ];
                let _ = self.eval_prelude(*call, args, Some(expected_type), span)?;
                Err(mir_error_at("MIR Todo operation was reached", span))
            }
            MirOperation::Never { reason } => Err(mir_error_at(
                &format!("MIR reached a never operation: {reason}"),
                span,
            )),
            MirOperation::LoopRangeInit {
                call: _,
                start,
                end,
                step,
                exclusive,
            } => {
                let start = int_value(
                    runtime_to_data(self.value(frame_index, *start, span)?, span)?,
                    span,
                )?;
                let end = int_value(
                    runtime_to_data(self.value(frame_index, *end, span)?, span)?,
                    span,
                )?;
                let step = step
                    .map(|step| {
                        int_value(
                            runtime_to_data(self.value(frame_index, step, span)?, span)?,
                            span,
                        )
                    })
                    .transpose()?;
                let cursor = crate::Comptime::CollectionEval::LoopRangeCursor::new(
                    start, end, step, *exclusive,
                )
                .map_err(|message| {
                    if self.config.runtime_execution {
                        self.located_runtime_stop("E3001", "<core.prelude>", 0, message, span)
                    } else {
                        crate::Comptime::comptime_panic(message, span)
                    }
                })?;
                let source = Rc::new(RefCell::new(MirInterpreterStream::Range { cursor }));
                let current = mir_stream_pull_handle(&source, self, span)?;
                Ok(RuntimeValue::StreamCursor(Rc::new(RefCell::new(
                    MirInterpreterStreamCursor {
                        source,
                        current,
                        step: 1,
                    },
                ))))
            }
            MirOperation::LoopRangeHasNext { call, cursor }
            | MirOperation::LoopIterHasNext { call, cursor } => {
                let value = self.value(frame_index, *cursor, span)?;
                if matches!(value, RuntimeValue::StreamCursor(_)) {
                    self.stream_cursor_has_next(value, span)
                } else {
                    self.eval_prelude_values(frame_index, *call, &[*cursor], result_ty, span)
                }
            }
            MirOperation::LoopRangeValue { call, cursor }
            | MirOperation::LoopIterValue { call, cursor } => {
                let value = self.value(frame_index, *cursor, span)?;
                if matches!(value, RuntimeValue::StreamCursor(_)) {
                    self.stream_cursor_value(value, span)
                } else {
                    self.eval_prelude_values(frame_index, *call, &[*cursor], result_ty, span)
                }
            }
            MirOperation::LoopRangeAdvance { call, cursor }
            | MirOperation::LoopIterAdvance { call, cursor } => {
                let value = self.value(frame_index, *cursor, span)?;
                if matches!(value, RuntimeValue::StreamCursor(_)) {
                    self.stream_cursor_advance(value, span)
                } else {
                    self.eval_prelude_values(frame_index, *call, &[*cursor], result_ty, span)
                }
            }
            MirOperation::LoopIterInit {
                call,
                collection,
                step,
                source_kind,
                by_value,
            } => {
                let collection_value = self.value(frame_index, *collection, span)?;
                let collection_type = self.value_type(frame_index, *collection, span)?;
                let is_stream = matches!(
                    collection_type.nominal_name(),
                    Some(crate::Syntax::TYPE_STREAM | crate::Syntax::TYPE_ITER | crate::Syntax::TYPE_VIEW_ITER)
                );
                let is_stdin = collection_type.nominal_name() == Some("StdinHandle");
                let step_value = step
                    .map(|step| self.value(frame_index, step, span))
                    .transpose()?
                    .map(|value| runtime_to_data(value, span))
                    .transpose()?;
                let has_step = step_value.is_some();
                let step = step_value.as_ref().and_then(mir_stream_int).unwrap_or(1);
                if matches!(source_kind, MirLoopSourceKind::LinesStdin) {
                    if !is_stdin {
                        return Err(mir_error_at(
                            "interpreter stdin line iterator requires a checked StdinHandle value",
                            span,
                        ));
                    }
                    if step <= 0 {
                        return Err(mir_error_at("iterator loop stride must be positive", span));
                    }
                    let result = crate::Comptime::try_ambient_mir_handle(
                        "loop.lines.init",
                        None,
                        vec![MirEvalValue::String("lines:stdin".to_string())],
                        span,
                    )
                    .ok_or_else(|| {
                        mir_error_at(
                            "interpreter line iterator has no ambient init host binding",
                            span,
                        )
                    })??;
                    let handle = match result {
                        crate::Comptime::AmbientMirHandleResult::Handle(handle) => handle,
                        crate::Comptime::AmbientMirHandleResult::Value(_) => {
                            return Err(mir_error_at(
                                "interpreter line iterator init returned the wrong typed carrier",
                                span,
                            ))
                        }
                    };
                    let source = Rc::new(RefCell::new(MirInterpreterStream::AmbientLines {
                        handle,
                        closed: false,
                    }));
                    let current = mir_stream_pull_handle(&source, self, span)?;
                    return Ok(RuntimeValue::StreamCursor(Rc::new(RefCell::new(
                        MirInterpreterStreamCursor {
                            source,
                            current,
                            step: usize::try_from(step).map_err(|_| {
                                mir_error_at("iterator loop stride is too large", span)
                            })?,
                        },
                    ))));
                }
                if matches!(source_kind, MirLoopSourceKind::LinesFile) {
                    if collection_type.nominal_name() != Some("FileReader") {
                        return Err(mir_error_at(
                            "interpreter file line iterator requires a checked FileReader value",
                            span,
                        ));
                    }
                    if step <= 0 {
                        return Err(mir_error_at("iterator loop stride must be positive", span));
                    }
                    let receiver = match collection_value {
                        RuntimeValue::Ambient(receiver) => receiver,
                        RuntimeValue::Moved => {
                            return Err(mir_error_at(
                                "interpreter FileReader iterator receiver was already moved",
                                span,
                            ));
                        }
                        _ => {
                            return Err(mir_error_at(
                                "interpreter file line iterator requires a checked FileReader value",
                                span,
                            ));
                        }
                    };
                    let source = Rc::new(RefCell::new(MirInterpreterStream::AmbientFileLines {
                        receiver,
                        closed: false,
                    }));
                    let current = mir_stream_pull_handle(&source, self, span)?;
                    return Ok(RuntimeValue::StreamCursor(Rc::new(RefCell::new(
                        MirInterpreterStreamCursor {
                            source,
                            current,
                            step: usize::try_from(step).map_err(|_| {
                                mir_error_at("iterator loop stride is too large", span)
                            })?,
                        },
                    ))));
                }

                if is_stream {
                    let source = match collection_value {
                        RuntimeValue::Stream(handle) => handle,
                        RuntimeValue::Data(MirEvalValue::List(values)) => {
                            Rc::new(RefCell::new(MirInterpreterStream::Source {
                                values: values.into(),
                            }))
                        }
                        RuntimeValue::Moved => {
                            return Err(mir_error_at(
                                "interpreter streaming iterator receiver was already moved",
                                span,
                            ))
                        }
                        _ => {
                            return Err(mir_error_at(
                                "interpreter streaming iterator requires a checked iterator value",
                                span,
                            ))
                        }
                    };
                    if step <= 0 {
                        return Err(mir_error_at("iterator loop stride must be positive", span));
                    }
                    let current = mir_stream_pull_handle(&source, self, span)?;
                    return Ok(RuntimeValue::StreamCursor(Rc::new(RefCell::new(
                        MirInterpreterStreamCursor {
                            source,
                            current,
                            step: usize::try_from(step).map_err(|_| {
                                mir_error_at("iterator loop stride is too large", span)
                            })?,
                        },
                    ))));
                }
                let collection_value = match (source_kind, collection_value) {
                    (MirLoopSourceKind::Plain, RuntimeValue::Data(MirEvalValue::List(values))) => {
                        return self.eval_plain_list_cursor(values, step_value, span);
                    }
                    (MirLoopSourceKind::Plain, RuntimeValue::Data(MirEvalValue::Map(entries))) => {
                        let values = entries
                            .into_iter()
                            .map(|(key, value)| MirEvalValue::Struct {
                                type_name: "Tuple".to_string(),
                                fields: vec![
                                    ("key".to_string(), mir_const_key_value(&key)),
                                    ("value".to_string(), value),
                                ],
                            })
                            .collect();
                        return self.eval_plain_list_cursor(values, step_value, span);
                    }
                    (_, value) => value,
                };
                let values = vec![
                    runtime_to_data(collection_value, span)?,
                    mir_optional_value(step_value),
                    MirEvalValue::Bool(has_step),
                    MirEvalValue::Bool(*by_value),
                    MirEvalValue::String(source_kind.wire()),
                ];
                self.eval_prelude(*call, values, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirOperation::ScopeEnter { scope, test_member } => {
                self.frames[frame_index].scopes.push(*scope);
                if let Some(MirTestScopeMember::Timeout { duration }) = test_member {
                    let duration = runtime_to_data(self.value(frame_index, *duration, span)?, span)?;
                    let limit = mir_stream_duration_ns(&duration)
                        .and_then(|value| i64::try_from(value).ok())
                        .ok_or_else(|| mir_error_at("MIR timeout scope duration is not valid", span))?;
                    let started = jet_foundation::Monotonic::jet_time_monotonic_now_ns();
                    self.frames[frame_index]
                        .timeout_starts
                        .insert(*scope, (started, limit));
                }
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            MirOperation::ScopeExit { scope } => {
                let found = self.frames[frame_index]
                    .scopes
                    .iter()
                    .rposition(|value| value == scope)
                    .ok_or_else(|| mir_error_at("MIR scope exit has no active scope", span))?;
                let transaction = self.frames[frame_index]
                    .shared_transactions
                    .get(scope)
                    .cloned();
                if let Some(transaction) = transaction {
                    transaction.commit(span)?;
                    self.frames[frame_index].shared_transactions.remove(scope);
                }
                let member = program_function(self.program, self.frames[frame_index].function)?
                    .test_scope_member(*scope)
                    .cloned();
                let expected_completed = matches!(
                    &member,
                    Some(MirTestScopeMember::ExpectFail { .. })
                ) && self.frames[frame_index]
                    .completed_scope_stops
                    .remove(scope)
                    .is_some();
                let timeout = matches!(&member, Some(MirTestScopeMember::Timeout { .. }))
                    .then(|| self.frames[frame_index].timeout_starts.remove(scope))
                    .flatten();
                self.frames[frame_index].scopes.remove(found);
                if let Some(MirTestScopeMember::ExpectFail { expected_code }) = member {
                    if !expected_completed {
                        let message = jet_foundation::Outcome::jet_test_expect_fail_message(
                            expected_code.as_deref(),
                        );
                        let error = self.located_runtime_stop("E3001", "", 0, &message, span);
                        return Err(error);
                    }
                }
                if let Some((started, limit)) = timeout {
                    let elapsed = jet_foundation::Monotonic::jet_time_monotonic_now_ns()
                        .saturating_sub(started);
                    if elapsed > limit {
                        let message = jet_foundation::Outcome::jet_test_timeout_message(
                            elapsed, limit,
                        );
                        let error = self.located_runtime_stop("E3001", "", 0, &message, span);
                        return Err(error);
                    }
                }
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            MirOperation::Drop { value, kind } => {
                let is_db_lease =
                    self.value_type(frame_index, *value, span)?.nominal_name() == Some("DbLease");
                let removed = self.frames[frame_index].values.remove(value);
                if let Some(removed) = removed {
                    if let Some(state) = Self::realtime_state(&removed) {
                        self.cancel_realtime_state(&state);
                    }
                    if runtime_contains_moved(&removed) {
                        return Ok(RuntimeValue::Data(MirEvalValue::Unit));
                    }
                    if is_db_lease {
                        let value = crate::Comptime::MirBridge::mir_to_ct_value(
                            runtime_to_data(removed, span)?,
                            span,
                        )?;
                        let runtime = self.is_runtime_invocation();
                        let mut sink = if runtime {
                            Some(crate::Comptime::DevSink::default())
                        } else {
                            None
                        };
                        let result = eval_core_call_binding(
                            None,
                            "core.handle",
                            "db_lease.close",
                            vec![value],
                            &[],
                            None,
                            None,
                            span,
                            runtime,
                            &self.config.base_dir,
                            sink.as_mut(),
                            None,
                        );
                        self.merge_runtime_sink(sink);
                        let _ = result?;
                    } else {
                        match removed {
                            RuntimeValue::DmaTransfer { transfer_id, .. } => {
                                self.dma_transfers.borrow_mut().remove(&transfer_id);
                            }
                            RuntimeValue::ForeignHandle { token }
                                if matches!(kind, MirDropKind::ForeignHandle) =>
                            {
                                self.close_foreign_handle(token, span)?;
                            }
                            RuntimeValue::Ambient(value) => {
                                if let Some(owner) =
                                    mir_runtime_owner::<MirAllocatorOwner>(&value)
                                {
                                    owner
                                        .state
                                        .lock()
                                        .unwrap_or_else(|error| error.into_inner())
                                        .closed = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            MirOperation::Call { .. }
            | MirOperation::CoreCall { .. }
            | MirOperation::IndirectCall { .. } => {
                Err(mir_error_at("MIR operation dispatch fell through", span))
            }
        }
    }

    fn install_hardware_setups(&mut self) -> Result<(), Diagnostic> {
        for setup in &self.program.facts.hardware_setups {
            let mut values = Vec::with_capacity(5);
            match setup {
                MirHardwareSetup::DmaConfigure {
                    profile_id,
                    channel,
                    transfer_width,
                    ownership,
                } => {
                    values.push(MirEvalValue::String(profile_id.clone()));
                    values.push(MirEvalValue::String(
                        crate::embedded_hardware::JET_HARDWARE_SETUP_DMA_CONFIGURE_NAME.to_string(),
                    ));
                    values.push(MirEvalValue::String(channel.clone()));
                    values.push(MirEvalValue::Int(transfer_width.bytes() as i64));
                    values.push(MirEvalValue::String(ownership.as_str().to_string()));
                }
                MirHardwareSetup::InterruptBind {
                    profile_id,
                    interrupt,
                    vector,
                    handler_symbol,
                    ..
                } => {
                    values.push(MirEvalValue::String(profile_id.clone()));
                    values.push(MirEvalValue::String(
                        crate::embedded_hardware::JET_HARDWARE_SETUP_INTERRUPT_BIND_NAME
                            .to_string(),
                    ));
                    values.push(MirEvalValue::String(interrupt.clone()));
                    values.push(MirEvalValue::Int(i64::from(*vector)));
                    values.push(MirEvalValue::String(handler_symbol.clone()));
                }
            }
            let result = crate::Comptime::try_ambient_mir_handle(
                "jet_hardware_setup",
                None,
                values,
                Span::new(0, 0),
            )
            .ok_or_else(|| {
                mir_error_at(
                    "MIR hardware setup has no interpreter ambient host binding",
                    Span::new(0, 0),
                )
            })??;
            match result {
                crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Unit) => {}
                crate::Comptime::AmbientMirHandleResult::Handle(_) => {
                    return Err(mir_error_at(
                        "MIR hardware setup adapter returned an opaque handle",
                        Span::new(0, 0),
                    ));
                }
                crate::Comptime::AmbientMirHandleResult::Value(_) => {
                    return Err(mir_error_at(
                        "MIR hardware setup adapter returned the wrong result carrier",
                        Span::new(0, 0),
                    ));
                }
            }
        }
        Ok(())
    }
    fn eval_dma_start(
        &mut self,
        profile: &str,
        channel: &str,
        buffer: RuntimeValue,
        buffer_ty: &MirType,
        bytes: Box<[u8]>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let completion_handle =
            jet_foundation::ResourceSchedule::current_frame_completion_handle("jet_dma_transfer")
                .map_err(|error| mir_error_at(&error, span))?;
        let transfer_id = {
            let mut next_transfer_id = self.next_dma_transfer_id.borrow_mut();
            let transfer_id = *next_transfer_id;
            *next_transfer_id = transfer_id
                .checked_add(1)
                .ok_or_else(|| mir_error_at("MIR DMA transfer identity exhausted", span))?;
            transfer_id
        };
        self.dma_transfers.borrow_mut().insert(
            transfer_id,
            PinnedDmaTransfer {
                value: buffer,
                buffer_ty: buffer_ty.clone(),
                bytes,
                completion_handle,
            },
        );
        let address_and_length =
            match self
                .dma_transfers
                .borrow()
                .get(&transfer_id)
                .map(|transfer| {
                    let address =
                        i64::try_from(transfer.bytes.as_ptr() as usize).map_err(|_| {
                            mir_error_at("MIR DMA buffer address does not fit Int", span)
                        })?;
                    let length = i64::try_from(transfer.bytes.len()).map_err(|_| {
                        mir_error_at("MIR DMA buffer length does not fit Int", span)
                    })?;
                    Ok((address, length))
                }) {
                Some(Ok(address_and_length)) => address_and_length,
                Some(Err(error)) => {
                    self.dma_transfers.borrow_mut().remove(&transfer_id);
                    return Err(error);
                }
                None => {
                    self.dma_transfers.borrow_mut().remove(&transfer_id);
                    return Err(mir_error_at(
                        "MIR DMA transfer registry entry is missing",
                        span,
                    ));
                }
            };
        let values = vec![
            MirEvalValue::String(profile.to_string()),
            MirEvalValue::String(channel.to_string()),
            MirEvalValue::Int(address_and_length.0),
            MirEvalValue::Int(address_and_length.1),
        ];
        let result =
            crate::Comptime::try_ambient_mir_handle("jet_hardware_dma_start", None, values, span);
        match result {
            Some(Ok(crate::Comptime::AmbientMirHandleResult::Handle(token))) if token > 0 => {
                Ok(RuntimeValue::DmaTransfer {
                    token,
                    profile: profile.to_string(),
                    channel: channel.to_string(),
                    transfer_id,
                })
            }
            Some(Ok(crate::Comptime::AmbientMirHandleResult::Handle(_))) => {
                self.dma_transfers.borrow_mut().remove(&transfer_id);
                Err(mir_error_at(
                    "MIR hardware DMA start adapter reported unavailable status",
                    span,
                ))
            }
            Some(Ok(crate::Comptime::AmbientMirHandleResult::Value(_))) => {
                self.dma_transfers.borrow_mut().remove(&transfer_id);
                Err(mir_error_at(
                    "MIR hardware DMA start adapter returned the wrong result carrier",
                    span,
                ))
            }
            Some(Err(error)) => {
                self.dma_transfers.borrow_mut().remove(&transfer_id);
                Err(error)
            }
            None => {
                self.dma_transfers.borrow_mut().remove(&transfer_id);
                Err(mir_error_at(
                    "MIR hardware DMA start has no interpreter ambient host binding",
                    span,
                ))
            }
        }
    }

    fn eval_dma_wait(
        &mut self,
        frame_index: usize,
        receiver: MirValueId,
        profile: &str,
        channel: &str,
        buffer_ty: &MirType,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let carrier = self.frames[frame_index]
            .values
            .remove(&receiver)
            .ok_or_else(|| mir_error_at("MIR DMA wait receiver is unavailable", span))?;
        let RuntimeValue::DmaTransfer {
            token,
            profile: carrier_profile,
            channel: carrier_channel,
            transfer_id,
        } = carrier
        else {
            return Err(mir_error_at(
                "MIR DMA wait receiver is not the checked transfer carrier",
                span,
            ));
        };
        let transfer = self
            .dma_transfers
            .borrow_mut()
            .remove(&transfer_id)
            .ok_or_else(|| mir_error_at("MIR DMA transfer carrier was already consumed", span))?;
        if token <= 0 {
            return Err(mir_error_at("MIR DMA transfer token is unavailable", span));
        }
        if carrier_profile != profile || carrier_channel != channel {
            return Err(mir_error_at(
                "MIR DMA wait carrier disagrees with checked profile or channel",
                span,
            ));
        }
        if !transfer.buffer_ty.same_checked_type(buffer_ty) {
            return Err(mir_error_at(
                "MIR DMA wait carrier disagrees with checked buffer type",
                span,
            ));
        }
        let result = crate::Comptime::try_ambient_mir_handle(
            "jet_hardware_dma_wait",
            Some(token),
            vec![
                MirEvalValue::String(profile.to_string()),
                MirEvalValue::String(channel.to_string()),
            ],
            span,
        );
        match result {
            Some(Ok(crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Int(0)))) => {
                if let Some(handle) = transfer.completion_handle.as_ref() {
                    handle
                        .signal()
                        .map_err(|error| mir_error_at(&error, span))?;
                }
                dma_decode_buffer(transfer.value, buffer_ty, &transfer.bytes, span)
            }
            Some(Ok(crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Int(status))))
                if status < 0 =>
            {
                Err(mir_error_at(
                    &format!("MIR hardware DMA wait failed with status {status}"),
                    span,
                ))
            }
            Some(Ok(crate::Comptime::AmbientMirHandleResult::Value(_))) => Err(mir_error_at(
                "MIR hardware DMA wait adapter returned a non-status value",
                span,
            )),
            Some(Ok(crate::Comptime::AmbientMirHandleResult::Handle(_))) => Err(mir_error_at(
                "MIR hardware DMA wait adapter returned an opaque handle",
                span,
            )),
            Some(Err(error)) => Err(error),
            None => Err(mir_error_at(
                "MIR hardware DMA wait has no interpreter ambient host binding",
                span,
            )),
        }
    }

    fn eval_hardware_call(
        &mut self,
        frame_index: usize,
        call: MirPreludeCallId,
        op: &MirHardwareOp,
        receiver: Option<MirValueId>,
        args: &[jet_foundation::MIR::MirCallArg],
        _result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let expected_symbol = match op {
            MirHardwareOp::RegisterRead { .. } => "jet_hardware_register_read",
            MirHardwareOp::RegisterWrite { .. } => "jet_hardware_register_write",
            MirHardwareOp::DmaStart { .. } => "jet_hardware_dma_start",
            MirHardwareOp::DmaWait { .. } => "jet_hardware_dma_wait",
        };
        let (module, symbol) = {
            let row = self.prelude_row(call, span)?;
            (row.module.clone(), row.symbol.name().to_string())
        };
        if module != "core.hardware" || symbol != expected_symbol {
            return Err(mir_error_at(
                "MIR hardware call does not match its canonical Prelude symbol",
                span,
            ));
        }
        let mut values = Vec::new();
        let handle = match op {
            MirHardwareOp::RegisterRead {
                profile_id,
                block,
                register,
                width,
            } => {
                if receiver.is_some() || !args.is_empty() {
                    return Err(mir_error_at(
                        "MIR hardware register read has unexpected operands",
                        span,
                    ));
                }
                values.extend([
                    MirEvalValue::String(profile_id.clone()),
                    MirEvalValue::String(block.clone()),
                    MirEvalValue::String(register.clone()),
                    MirEvalValue::Int(width.bytes() as i64),
                ]);
                None
            }
            MirHardwareOp::RegisterWrite {
                profile_id,
                block,
                register,
                width,
            } => {
                if receiver.is_some() || args.len() != 1 {
                    return Err(mir_error_at(
                        "MIR hardware register write has invalid operands",
                        span,
                    ));
                }
                values.extend([
                    MirEvalValue::String(profile_id.clone()),
                    MirEvalValue::String(block.clone()),
                    MirEvalValue::String(register.clone()),
                    MirEvalValue::Int(width.bytes() as i64),
                ]);
                let mut call_values = self.call_args(frame_index, args, span)?;
                let value = call_values.pop().ok_or_else(|| {
                    mir_error_at("MIR hardware register write lost its value", span)
                })?;
                values.push(runtime_to_data(value, span)?);
                None
            }
            MirHardwareOp::DmaStart {
                profile_id,
                channel,
                buffer_ty,
            } => {
                if receiver.is_some() || args.len() != 1 {
                    return Err(mir_error_at(
                        "MIR hardware DMA start has invalid operands",
                        span,
                    ));
                }
                let mut call_values = self.call_args(frame_index, args, span)?;
                let buffer = call_values
                    .pop()
                    .ok_or_else(|| mir_error_at("MIR hardware DMA start lost its buffer", span))?;
                let bytes = dma_encode_buffer(&buffer, buffer_ty, span)?;
                return self.eval_dma_start(profile_id, channel, buffer, buffer_ty, bytes, span);
            }
            MirHardwareOp::DmaWait {
                profile_id,
                channel,
                buffer_ty,
            } => {
                if !args.is_empty() {
                    return Err(mir_error_at(
                        "MIR hardware DMA wait has invalid arguments",
                        span,
                    ));
                }
                let receiver = receiver.ok_or_else(|| {
                    mir_error_at("MIR hardware DMA wait has no transfer receiver", span)
                })?;
                return self.eval_dma_wait(
                    frame_index,
                    receiver,
                    profile_id,
                    channel,
                    buffer_ty,
                    span,
                );
            }
        };
        let result = crate::Comptime::try_ambient_mir_handle(expected_symbol, handle, values, span)
            .ok_or_else(|| {
                mir_error_at(
                    "MIR hardware operation has no interpreter ambient host binding",
                    span,
                )
            })??;
        match (op, result) {
            (
                MirHardwareOp::RegisterRead { .. },
                crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Int(value)),
            ) => Ok(RuntimeValue::Data(MirEvalValue::Int(value))),
            (
                MirHardwareOp::RegisterRead { .. },
                crate::Comptime::AmbientMirHandleResult::Value(_),
            )
            | (
                MirHardwareOp::RegisterRead { .. },
                crate::Comptime::AmbientMirHandleResult::Handle(_),
            ) => Err(mir_error_at(
                "MIR hardware register read adapter returned the wrong result carrier",
                span,
            )),
            (
                MirHardwareOp::RegisterWrite { .. },
                crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Unit),
            ) => Ok(RuntimeValue::Data(MirEvalValue::Unit)),
            (
                MirHardwareOp::RegisterWrite { .. },
                crate::Comptime::AmbientMirHandleResult::Value(_),
            )
            | (
                MirHardwareOp::RegisterWrite { .. },
                crate::Comptime::AmbientMirHandleResult::Handle(_),
            ) => Err(mir_error_at(
                "MIR hardware register write adapter returned the wrong result carrier",
                span,
            )),
            (MirHardwareOp::DmaStart { .. }, _) | (MirHardwareOp::DmaWait { .. }, _) => {
                Err(mir_error_at(
                    "MIR hardware DMA path did not use its pinned transfer carrier",
                    span,
                ))
            }
        }
    }
    fn eval_http_router_ambient(
        &mut self,
        operation: &str,
        receiver: Option<RuntimeValue>,
        values: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        self.eval_http_ambient_with_handler(operation, receiver, values, result_ty, None, span)
    }

    fn eval_http_route_ambient(
        &mut self,
        operation: &str,
        receiver: Option<RuntimeValue>,
        values: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        self.eval_http_ambient_with_handler(operation, receiver, values, result_ty, Some(2), span)
    }

    fn eval_http_ambient_with_handler(
        &mut self,
        operation: &str,
        receiver: Option<RuntimeValue>,
        values: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        handler_index: Option<usize>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let handle = receiver
            .map(|value| match value {
                RuntimeValue::ForeignHandle { token } => token.raw().ok_or_else(|| {
                    mir_error_at(
                        "MIR HTTP operation received an invalid handle receiver",
                        span,
                    )
                }),
                _ => Err(mir_error_at(
                    "MIR HTTP operation received a non-handle receiver",
                    span,
                )),
            })
            .transpose()?;
        let values = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                if handler_index == Some(index) {
                    mir_http_handler_token(value, span)
                } else {
                    runtime_to_data(value, span)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let result = crate::Comptime::try_ambient_mir_handle(operation, handle, values, span)
            .ok_or_else(|| {
                mir_error_at(
                    "MIR HTTP operation has no interpreter ambient host binding",
                    span,
                )
            })??;
        let constructor = operation.ends_with(".new");
        match result {
            crate::Comptime::AmbientMirHandleResult::Handle(raw) if constructor => {
                let handle = result_ty
                    .and_then(|ty| self.handle_id_for_type(ty))
                    .ok_or_else(|| {
                        mir_error_at(
                            "MIR HTTP constructor result has no canonical lifecycle row",
                            span,
                        )
                    })?;
                Ok(RuntimeValue::ForeignHandle {
                    token: MirHandleToken::new(handle, raw),
                })
            }
            crate::Comptime::AmbientMirHandleResult::Value(value) if !constructor => {
                Ok(RuntimeValue::Data(value))
            }
            _ if constructor => Err(mir_error_at(
                "MIR HTTP constructor returned a non-handle value",
                span,
            )),
            _ => Err(mir_error_at(
                "MIR HTTP operation returned a private handle where a value was expected",
                span,
            )),
        }
    }

    fn eval_game_handle(
        &mut self,
        module: &str,
        member: &str,
        receiver: RuntimeValue,
        args: Vec<RuntimeValue>,
        frame_schedule: Option<&jet_foundation::ResourceSchedule::JetFrameSchedule>,
        frame_schedule_derivation: Option<&jet_foundation::Facts::DerivationRef>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let arity = |expected: usize| {
            if args.len() == expected {
                Ok(())
            } else {
                Err(mir_error_at(
                    &format!(
                        "MIR game operation `{module}.{member}` received {} arguments; expected {expected}",
                        args.len()
                    ),
                    span,
                ))
            }
        };
        match (module, member) {
            ("core.handle", "game.scene_new") => {
                arity(0)?;
                let name = game_string(receiver, span)?;
                let dev_session = if game_dev_protocol::game_debug_data_enabled(
                    &self.config.release_devtools_policy,
                ) {
                    Some(
                        game_dev_protocol::new_session_with_policy(&name, "mir", true)
                            .map_err(|error| mir_error_at(&error, span))?,
                    )
                } else {
                    None
                };
                let scene = Rc::new(RefCell::new(MirGameScene {
                    name,
                    assets: Vec::new(),
                    bindings: Vec::new(),
                    components: Vec::new(),
                    callbacks: Vec::new(),
                    dev_session,
                }));
                Ok(RuntimeValue::Game(MirGameValue::Scene(scene)))
            }
            ("core.handle", "game.replay_record") => {
                arity(0)?;
                Ok(RuntimeValue::Game(MirGameValue::Replay(game_string(
                    receiver, span,
                )?)))
            }
            ("core.game", "backend_headless") => {
                arity(0)?;
                Ok(RuntimeValue::Game(MirGameValue::Backend(Rc::new(
                    RefCell::new(MirGameBackend {
                        renderer: "headless".to_string(),
                        audio: "none".to_string(),
                        editor: "none".to_string(),
                        frame_budget: jet_foundation::Game::JetGameFrameBudget::default(),
                    }),
                ))))
            }
            ("core.handle", "game.scene_on_frame") => {
                arity(1)?;
                let scene = game_scene(receiver, span)?;
                let callback = self.closure_handle(
                    args.into_iter()
                        .next()
                        .ok_or_else(|| mir_error_at("MIR game frame callback is missing", span))?,
                    span,
                )?;
                if let (Some(route), Some(closure)) =
                    (frame_schedule, callback.facts.frame_schedule.as_ref())
                {
                    if route != closure {
                        return Err(mir_error_at(
                            "MIR game frame callback schedule differs from its checked closure",
                            span,
                        ));
                    }
                }
                if let (Some(route), Some(closure)) = (
                    frame_schedule_derivation,
                    callback.facts.frame_schedule_derivation.as_ref(),
                ) {
                    if route != closure {
                        return Err(mir_error_at(
                            "MIR game frame callback derivation differs from its checked closure",
                            span,
                        ));
                    }
                }
                let schedule = frame_schedule
                    .cloned()
                    .or_else(|| callback.facts.frame_schedule.clone());
                let derivation = frame_schedule_derivation
                    .cloned()
                    .or_else(|| callback.facts.frame_schedule_derivation.clone());
                let mut scene = scene
                    .try_borrow_mut()
                    .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
                if scene.callbacks.len() >= MAX_MIR_GAME_SCENE_CALLBACKS {
                    return Err(mir_error_at("MIR game scene callback limit exceeded", span));
                }
                scene.callbacks.push(MirGameCallback {
                    completion: schedule.as_ref().map(|schedule| {
                        std::sync::Arc::new(std::sync::Mutex::new(
                            jet_foundation::ResourceSchedule::JetFrameCompletionState::new(
                                schedule,
                            ),
                        ))
                    }),
                    callback,
                    schedule,
                    derivation,
                });
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            ("core.handle", "game.scene_component") => {
                arity(1)?;
                let scene = game_scene(receiver, span)?;
                let component = game_string(
                    args.into_iter()
                        .next()
                        .ok_or_else(|| mir_error_at("MIR game component name is missing", span))?,
                    span,
                )?;
                let mut scene = scene
                    .try_borrow_mut()
                    .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
                if !scene.components.iter().any(|name| name == &component) {
                    scene.components.push(component);
                }
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            ("core.handle", "game.scene_query") => {
                arity(1)?;
                let scene = game_scene(receiver, span)?;
                let names = game_string(
                    args.into_iter()
                        .next()
                        .ok_or_else(|| mir_error_at("MIR game query names are missing", span))?,
                    span,
                )?;
                let scene = scene
                    .try_borrow()
                    .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
                let wanted = names
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>();
                if !wanted
                    .iter()
                    .all(|name| scene.components.iter().any(|existing| existing == name))
                {
                    return Ok(RuntimeValue::Data(MirEvalValue::List(Vec::new())));
                }
                let row = wanted
                    .iter()
                    .map(|name| match *name {
                        "Position" => "Position{x:0}".to_string(),
                        "Velocity" => "Velocity{dx:0}".to_string(),
                        other => format!("{other}{{}}"),
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                Ok(RuntimeValue::Data(MirEvalValue::List(if row.is_empty() {
                    Vec::new()
                } else {
                    vec![MirEvalValue::String(row)]
                })))
            }
            ("core.handle", "game.assets_image" | "game.assets_sound") => {
                arity(1)?;
                let scene = game_assets(receiver, span)?;
                let path = game_string(
                    args.into_iter()
                        .next()
                        .ok_or_else(|| mir_error_at("MIR game asset path is missing", span))?,
                    span,
                )?;
                if path.contains("missing") {
                    return Ok(RuntimeValue::Data(MirEvalValue::FailedTold(Box::new(
                        MirEvalValue::String(format!("asset not found: {path}")),
                    ))));
                }
                let kind = if member.ends_with("image") {
                    "image"
                } else {
                    "sound"
                };
                scene
                    .try_borrow_mut()
                    .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?
                    .assets
                    .push((kind.to_string(), path.clone()));
                let type_name = if kind == "image" {
                    "GameImage"
                } else {
                    "GameSound"
                };
                Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(
                    MirEvalValue::Struct {
                        type_name: type_name.to_string(),
                        fields: vec![("path".to_string(), MirEvalValue::String(path))],
                    },
                ))))
            }
            ("core.handle", "game.input_bind") => {
                arity(2)?;
                let scene = game_input_scene(receiver, span)?;
                let mut args = args.into_iter();
                let action = game_string(
                    args.next()
                        .ok_or_else(|| mir_error_at("MIR game input action is missing", span))?,
                    span,
                )?;
                let key = game_string(
                    args.next()
                        .ok_or_else(|| mir_error_at("MIR game input key is missing", span))?,
                    span,
                )?;
                let mut scene = scene
                    .try_borrow_mut()
                    .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
                if !scene
                    .bindings
                    .iter()
                    .any(|(existing_action, existing_key)| {
                        existing_action == &action && existing_key == &key
                    })
                {
                    scene.bindings.push((action, key));
                }
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            ("core.handle", "game.input_pressed") => {
                arity(1)?;
                let input = game_input_snapshot(receiver, span)?;
                let action = game_string(
                    args.into_iter()
                        .next()
                        .ok_or_else(|| mir_error_at("MIR game input action is missing", span))?,
                    span,
                )?;
                Ok(RuntimeValue::Data(MirEvalValue::Bool(
                    input.iter().any(|existing| existing == &action),
                )))
            }
            ("core.handle", "game.backend_should_continue") => {
                arity(0)?;
                let backend = game_backend(receiver, span)?;
                let backend = backend
                    .try_borrow()
                    .map_err(|_| mir_error_at("MIR game backend is already borrowed", span))?;
                Ok(RuntimeValue::Data(MirEvalValue::Bool(
                    backend.frame_budget.should_continue(),
                )))
            }
            ("core.handle", "game.backend_present") => {
                arity(0)?;
                let backend = game_backend(receiver, span)?;
                backend
                    .try_borrow_mut()
                    .map_err(|_| mir_error_at("MIR game backend is already borrowed", span))?
                    .frame_budget
                    .present();
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            _ => Err(mir_error_at(
                &format!("unknown interpreter game handle operation `{module}.{member}`"),
                span,
            )),
        }
    }

    fn eval_game_run(
        &mut self,
        args: Vec<RuntimeValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let mut args = args.into_iter();
        let scene = game_scene(
            args.next()
                .ok_or_else(|| mir_error_at("MIR game.run scene is missing", span))?,
            span,
        )?;
        let replay = game_option(args.next(), span)?;
        let backend = game_option(args.next(), span)?;
        let frames = game_option(args.next(), span)?;
        let replay = replay
            .as_ref()
            .map(|value| game_replay(value, span))
            .transpose()?;
        let backend = backend
            .as_ref()
            .map(|value| game_backend(value.clone(), span))
            .transpose()?;
        let requested_frames = frames
            .as_ref()
            .map(|value| game_int(value, span))
            .transpose()?;
        let backend = match backend {
            Some(backend) => backend,
            None => game_backend(
                self.eval_game_handle(
                    "core.game",
                    "backend_headless",
                    RuntimeValue::Data(MirEvalValue::Unit),
                    Vec::new(),
                    None,
                    None,
                    span,
                )?,
                span,
            )?,
        };
        let (renderer, audio, editor, mut frame_plan) = {
            let backend = backend
                .try_borrow()
                .map_err(|_| mir_error_at("MIR game backend is already borrowed", span))?;
            (
                backend.renderer.clone(),
                backend.audio.clone(),
                backend.editor.clone(),
                backend.frame_budget.clone(),
            )
        };
        frame_plan
            .set_requested(requested_frames)
            .map_err(|error| mir_error_at(error, span))?;
        let (name, assets, bindings, components, callbacks) = {
            let scene = scene
                .try_borrow()
                .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
            (
                scene.name.clone(),
                scene.assets.clone(),
                scene.bindings.clone(),
                scene.components.clone(),
                scene.callbacks.clone(),
            )
        };
        let (replay_path, replay_tape) = if let Some(path) = replay.as_ref() {
            let tape = JetGameReplay::from_path(path)
                .map_err(|error| mir_error_at(error.as_str(), span))?;
            (path.as_str(), Some(tape))
        } else {
            ("<none>", None)
        };
        let mut out = vec![
            format!("scene:{name}"),
            format!("backend:{renderer}/{audio}/{editor}"),
            format!("replay:{replay_path}"),
            format!(
                "assets:{}",
                if assets.is_empty() {
                    "none".to_string()
                } else {
                    assets
                        .iter()
                        .map(|(kind, path)| format!("{kind}:{path}"))
                        .collect::<Vec<_>>()
                        .join(",")
                }
            ),
            format!(
                "input:{}",
                if bindings.is_empty() {
                    "none".to_string()
                } else {
                    bindings
                        .iter()
                        .map(|(action, key)| format!("{action}={key}"))
                        .collect::<Vec<_>>()
                        .join(",")
                }
            ),
            format!(
                "components:{}",
                if components.is_empty() {
                    "none".to_string()
                } else {
                    components.join(",")
                }
            ),
        ];
        {
            let mut scene = scene
                .try_borrow_mut()
                .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
            if let Some(session) = scene.dev_session.as_mut() {
                let identity = session.profiler.identity().clone();
                session
                    .set_trace_context(game_dev_protocol::JetGameTraceContext::new(
                        identity.session,
                        name.clone(),
                        "mir",
                        format!("{renderer}/{audio}/{editor}"),
                        1,
                        1,
                        "default",
                    ))
                    .map_err(|error| mir_error_at(&error, span))?;
                session
                    .dispatch_pending_devtools_controls(0)
                    .map_err(|error| mir_error_at(&error, span))?;
                if session.phase == game_dev_protocol::GameDevPhase::Editing {
                    session
                        .dispatch_control(0, game_dev_protocol::GameDevControl::Play)
                        .map_err(|error| mir_error_at(&error, span))?;
                }
            }
        }
        let mut transcript = jet_foundation::Game::JetGameTranscriptHasher::new();
        let mut last_frame = 0_u64;
        while let Some(index) = frame_plan.next_frame() {
            loop {
                let admitted = {
                    let mut scene = scene
                        .try_borrow_mut()
                        .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
                    match scene.dev_session.as_mut() {
                        None => true,
                        Some(session) => {
                            session
                                .dispatch_pending_devtools_controls(index as u64)
                                .map_err(|error| mir_error_at(&error, span))?;
                            session.begin_simulation_frame()
                        }
                    }
                };
                if admitted {
                    break;
                }
                // A paused resident session keeps the issued frame in flight
                // while host commands are polled at the same boundary.
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            last_frame = index as u64;
            let pressed = replay_tape
                .as_ref()
                .and_then(|tape| tape.actions_at(index))
                .map(|actions| actions.to_vec())
                .unwrap_or_default();
            transcript.push_frame(index, &pressed);
            let frame = MirGameFrame {
                index,
                pressed: pressed.clone(),
            };
            let started = std::time::Instant::now();
            let mut previous_schedule_source = None;
            for registration in &callbacks {
                if let Some(schedule) = &registration.schedule {
                    if schedule
                        .operations
                        .windows(2)
                        .any(|window| window[0].source_index > window[1].source_index)
                    {
                        return Err(mir_error_at(
                            "MIR game frame schedule is not in checked source order",
                            span,
                        ));
                    }
                    if let Some(first) = schedule.operations.first() {
                        if previous_schedule_source
                            .is_some_and(|previous| first.source_index < previous)
                        {
                            return Err(mir_error_at(
                                "MIR game frame callbacks are not in checked source order",
                                span,
                            ));
                        }
                        previous_schedule_source = Some(first.source_index);
                    }
                    if registration.derivation.is_none() {
                        return Err(mir_error_at(
                            "MIR game frame schedule has no canonical derivation",
                            span,
                        ));
                    }
                }
                let completion = registration.completion.clone();
                let mut invoke = || {
                    self.invoke_callback(
                        RuntimeValue::Closure(registration.callback.clone()),
                        RuntimeValue::Game(MirGameValue::Frame(frame.clone())),
                        span,
                    )
                };
                if let Some(completion) = completion {
                    jet_foundation::ResourceSchedule::with_frame_completion_scope(
                        completion, invoke,
                    )?;
                } else {
                    invoke()?;
                }
                if let Some(schedule) = &registration.schedule {
                    let completion = registration.completion.as_ref().ok_or_else(|| {
                        mir_error_at("MIR game frame schedule has no completion state", span)
                    })?;
                    let completion = completion.lock().map_err(|_| {
                        mir_error_at("MIR frame completion state is poisoned", span)
                    })?;
                    completion
                        .assert_reuse_ready(schedule)
                        .map_err(|error| mir_error_at(&error, span))?;
                }
            }
            let elapsed_ns = started.elapsed().as_nanos() as u64;
            let input = if pressed.is_empty() {
                "none".to_string()
            } else {
                pressed.join("+")
            };
            out.push(format!("frame:{index} input:{input}"));
            {
                let mut scene = scene
                    .try_borrow_mut()
                    .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
                if let Some(session) = scene.dev_session.as_mut() {
                    let identity = session.profiler.identity().clone();
                    if session.profiler.profiler().state()
                        == game_dev_protocol::JetGameFrameProfilerState::Running
                    {
                        let frame_identity = game_dev_protocol::JetGameFrameIdentity::new(
                            name.clone(),
                            index as u64,
                            identity.build,
                            identity.revision,
                            identity.source,
                        );
                        let sample = game_dev_protocol::JetGameFrameSample::new(
                            frame_identity,
                            index as u64,
                            elapsed_ns,
                            None,
                        );
                        session
                            .record_and_publish(sample.into())
                            .map_err(|error| mir_error_at(&error, span))?;
                    }
                    session
                        .finish_simulation_frame(index as u64, index as u64)
                        .map_err(|error| mir_error_at(&error, span))?;
                }
            }
            frame_plan.present();
        }
        {
            let mut scene = scene
                .try_borrow_mut()
                .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
            if let Some(session) = scene.dev_session.as_mut() {
                session
                    .stop(last_frame, "game run completed")
                    .map_err(|error| mir_error_at(&error, span))?;
            }
        }
        if requested_frames.is_some() {
            out.push(format!("transcript_hash:{}", transcript.finish()));
        }
        Ok(RuntimeValue::Data(MirEvalValue::String(out.join("\n"))))
    }
    fn eval_atomic_builtin(
        &mut self,
        frame_index: usize,
        call: MirPreludeCallId,
        receiver: MirValueId,
        receiver_place: Option<MirPlaceId>,
        args: &[MirValueId],
        span: Span,
    ) -> Result<Option<RuntimeValue>, Diagnostic> {
        let route = self.prelude_row(call, span)?.clone();
        if route.family != jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
            || route.module != "core.mem"
            || !matches!(
                route.member.as_str(),
                "load" | "store" | "add" | "try_add" | "compare_exchange" | "publish" | "observe"
            )
        {
            return Ok(None);
        }
        let expected_symbol = format!("jet_atomic_{}", route.member);
        if route.symbol.name() != expected_symbol {
            return Err(mir_error_at(
                &format!(
                    "MIR Atomic route `{}` has symbol `{}`, expected `{}`",
                    route.member,
                    route.symbol.name(),
                    expected_symbol
                ),
                span,
            ));
        }
        if receiver_place.is_some() {
            return Err(mir_error_at(
                "MIR Atomic methods require a value receiver, not a receiver place",
                span,
            ));
        }
        let receiver_ty = self.value_type(frame_index, receiver, span)?;
        let MirTypeKind::Apply {
            name,
            args: type_args,
        } = receiver_ty.kind()
        else {
            return Err(mir_error_at(
                "MIR Atomic receiver is not an Atomic<T> application",
                span,
            ));
        };
        let [inner_ty] = type_args.as_slice() else {
            return Err(mir_error_at(
                "MIR Atomic receiver must have exactly one type argument",
                span,
            ));
        };
        if name.name != "Atomic" {
            return Err(mir_error_at(
                "MIR Atomic route receiver type is not Atomic<T>",
                span,
            ));
        }
        let kind = mir_atomic_kind(inner_ty, span)?;
        let RuntimeValue::Atomic(cell) = self.value(frame_index, receiver, span)? else {
            return Err(mir_error_at(
                "MIR Atomic receiver has no canonical atomic-word carrier",
                span,
            ));
        };
        let mut values = args
            .iter()
            .map(|value| self.value(frame_index, *value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        if route.member == "try_add" {
            if values.len() != 1 {
                return Err(mir_error_at("MIR Atomic.try_add takes one argument", span));
            }
            let delta = match mir_atomic_try_bits(values.remove(0), kind, span)? {
                Ok(delta) => delta,
                Err(error) => {
                    return Ok(Some(RuntimeValue::Result {
                        ok: false,
                        value: Box::new(RuntimeValue::Data(MirEvalValue::Struct {
                            type_name: crate::Syntax::TYPE_ALLOC_ERROR.to_string(),
                            fields: vec![
                                (
                                    "requested_bytes".to_string(),
                                    MirEvalValue::Int(error.requested_bytes),
                                ),
                                (
                                    "allocator".to_string(),
                                    MirEvalValue::String(error.allocator),
                                ),
                            ],
                        })),
                    }))
                }
            };
            let outcome = mir_atomic_try_add(&cell, kind, delta, span)?;
            return Ok(Some(match outcome {
                Ok(value) => RuntimeValue::Result {
                    ok: true,
                    value: Box::new(RuntimeValue::Data(value)),
                },
                Err(error) => RuntimeValue::Result {
                    ok: false,
                    value: Box::new(RuntimeValue::Data(MirEvalValue::Struct {
                        type_name: crate::Syntax::TYPE_ALLOC_ERROR.to_string(),
                        fields: vec![
                            (
                                "requested_bytes".to_string(),
                                MirEvalValue::Int(error.requested_bytes),
                            ),
                            (
                                "allocator".to_string(),
                                MirEvalValue::String(error.allocator),
                            ),
                        ],
                    })),
                },
            }));
        }
        let result = match route.member.as_str() {
            "load" => {
                if !values.is_empty() {
                    return Err(mir_error_at("MIR Atomic.load takes no arguments", span));
                }
                mir_atomic_runtime_value(kind, mir_atomic_load(&cell, kind, span)?, span)?
            }
            "observe" => {
                if !values.is_empty() {
                    return Err(mir_error_at("MIR Atomic.observe takes no arguments", span));
                }
                mir_atomic_runtime_value(kind, mir_atomic_observe(&cell, kind, span)?, span)?
            }
            "store" | "publish" => {
                if values.len() != 1 {
                    return Err(mir_error_at(
                        &format!("MIR Atomic.{} takes one argument", route.member),
                        span,
                    ));
                }
                let next = mir_atomic_bits(values.remove(0), kind, span)?;
                if route.member == "store" {
                    mir_atomic_store(&cell, kind, next, span)?;
                } else {
                    mir_atomic_publish(&cell, kind, next, span)?;
                }
                MirEvalValue::Unit
            }
            "add" => {
                if values.len() != 1 {
                    return Err(mir_error_at("MIR Atomic.add takes one argument", span));
                }
                let delta = mir_atomic_bits(values.remove(0), kind, span)?;
                mir_atomic_add(&cell, kind, delta, span)?
            }
            "compare_exchange" => {
                if values.len() != 2 {
                    return Err(mir_error_at(
                        "MIR Atomic.compare_exchange takes two arguments",
                        span,
                    ));
                }
                let expected = mir_atomic_bits(values.remove(0), kind, span)?;
                let replacement = mir_atomic_bits(values.remove(0), kind, span)?;
                let exchanged =
                    mir_atomic_compare_exchange(&cell, kind, expected, replacement, span)?;
                MirEvalValue::Bool(exchanged)
            }
            _ => unreachable!(),
        };
        Ok(Some(RuntimeValue::Data(result)))
    }

    fn eval_semantic(
        &mut self,
        frame_index: usize,
        operation: &MirSemanticOp,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match operation {
            MirSemanticOp::PluginInvoke {
                call,
                handle,
                export_name,
                signature,
                args,
            } => {
                let route = self.prelude_row(*call, span)?;
                if route.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err(mir_error_at(
                        "MIR plugin invocation has unsupported Prelude ABI",
                        span,
                    ));
                }
                let symbol = route.symbol.name().to_string();
                let receiver = self.value(frame_index, *handle, span)?;
                let receiver = runtime_to_data(self.materialize_runtime(receiver, span)?, span)?;
                let MirEvalValue::Struct { fields, .. } = receiver else {
                    return Err(mir_error_at(
                        "MIR plugin receiver is not its checked handle carrier",
                        span,
                    ));
                };
                let raw = fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("handle", MirEvalValue::Int(raw)) => Some(*raw),
                        _ => None,
                    })
                    .ok_or_else(|| mir_error_at("MIR plugin receiver has no handle field", span))?;
                let params = args
                    .iter()
                    .map(|value| {
                        let value = self.value(frame_index, *value, span)?;
                        runtime_to_data(self.materialize_runtime(value, span)?, span)
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                let result = crate::Comptime::try_ambient_mir_handle(
                    &symbol,
                    Some(raw),
                    vec![
                        MirEvalValue::String(export_name.clone()),
                        MirEvalValue::List(params),
                        MirEvalValue::String(signature.wire()),
                    ],
                    span,
                )
                .ok_or_else(|| {
                    mir_error_at(
                        "MIR plugin invocation has no interpreter ambient host binding",
                        span,
                    )
                })??;
                match result {
                    crate::Comptime::AmbientMirHandleResult::Value(value) => {
                        Ok(RuntimeValue::Data(value))
                    }
                    crate::Comptime::AmbientMirHandleResult::Handle(_) => Err(mir_error_at(
                        "MIR plugin invocation returned an unexpected opaque handle",
                        span,
                    )),
                }
            }
            MirSemanticOp::DataEntriesToMap { call, local } => {
                let value = self.frames[frame_index]
                    .locals
                    .get(local)
                    .cloned()
                    .ok_or_else(|| mir_error_at("MIR data-entry local is unavailable", span))?;
                let value = runtime_to_data(value, span)?;
                self.eval_prelude(*call, vec![value], result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirSemanticOp::MathBuiltin { call, args, .. }
            | MirSemanticOp::PreciseBuiltin { call, args, .. } => {
                self.eval_prelude_values(frame_index, *call, args, result_ty, span)
            }
            MirSemanticOp::Print { call, value } => {
                // The print row is `jet_term_write_stdout_line(text, flush)`;
                // AOT emits `flush = true`, so the evaluator supplies the same
                // constant to keep one executable meaning (I9).
                let text = self.value(frame_index, *value, span)?;
                let value_type = self.value_type(frame_index, *value, span)?.clone();
                let text = self.native_print_value(text, &value_type, span)?;
                self.eval_prelude_runtime_values(
                    *call,
                    vec![text, RuntimeValue::Data(MirEvalValue::Bool(true))],
                    result_ty,
                    span,
                )
            }
            MirSemanticOp::AmbientInput { call, prompt } => {
                let prompt = match prompt {
                    Some(value) => self.value(frame_index, *value, span)?,
                    None => RuntimeValue::Ambient(CtValue::absent(crate::AST::Type::String)),
                };
                self.eval_prelude_runtime_values(*call, vec![prompt], result_ty, span)
            }
            MirSemanticOp::RequireStop {
                call,
                kind,
                condition,
                location,
                context,
                values,
                always_stops,
            } => self.eval_require_stop(
                frame_index,
                *call,
                *kind,
                *condition,
                *location,
                context,
                values,
                result_ty,
                *always_stops,
                span,
            ),
            MirSemanticOp::LayoutCompare {
                call,
                op,
                left,
                right,
            } => {
                let row = self.prelude_row(*call, span)?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err(mir_error_at(
                        "MIR LayoutCompare route has unsupported Prelude ABI",
                        span,
                    ));
                }
                let left = runtime_to_data(self.value(frame_index, *left, span)?, span)?;
                let right = runtime_to_data(self.value(frame_index, *right, span)?, span)?;
                Ok(RuntimeValue::Data(MirEvalValue::Bool(layout_compare(
                    *op, &left, &right,
                ))))
            }
            MirSemanticOp::LayoutLiteral { inner } => self.value(frame_index, *inner, span),
            MirSemanticOp::StructLiteral {
                type_id,
                fields,
                extra,
                trait_coercion,
                boxed_fields,
            } => {
                let type_name = type_instance_name(self.program, *type_id, span)?;
                let values = fields
                    .iter()
                    .map(|(field, value)| Ok((*field, self.value(frame_index, *value, span)?)))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                let _ = (extra, trait_coercion, boxed_fields);
                self.aggregate_or_data(type_name, values, span)
            }
            MirSemanticOp::CellGuardProject {
                map_call,
                split_call,
                guard,
                paths,
                editable,
                edit_paths_disjoint,
            } => self.eval_cell_guard_project(
                frame_index,
                *map_call,
                *split_call,
                *guard,
                paths,
                *editable,
                *edit_paths_disjoint,
                result_ty,
                span,
            ),
            MirSemanticOp::SharedGuardMap {
                call,
                guard,
                path,
                editable,
            } => self.eval_shared_guard_map(
                frame_index,
                *call,
                *guard,
                path,
                *editable,
                result_ty,
                span,
            ),
            MirSemanticOp::SharedGuardSplit {
                call,
                map_call,
                guard,
                first,
                second,
                editable,
            } => self.eval_shared_guard_split(
                frame_index,
                *call,
                *map_call,
                *guard,
                first,
                second,
                *editable,
                result_ty,
                span,
            ),
            MirSemanticOp::SharedGuardWait {
                call,
                guard,
                condition,
                predicate,
            } => self.eval_prelude_values(
                frame_index,
                *call,
                &[*guard, *condition, *predicate],
                result_ty,
                span,
            ),
            MirSemanticOp::ConditionNotify {
                call,
                condition,
                all,
            } => {
                let condition = runtime_to_data(self.value(frame_index, *condition, span)?, span)?;
                self.eval_prelude(
                    *call,
                    vec![condition, MirEvalValue::Bool(*all)],
                    result_ty,
                    span,
                )
                .map(RuntimeValue::Data)
            }
            MirSemanticOp::AllocNew {
                call,
                kind,
                inline_size,
                args,
            } => {
                let row = self.prelude_row(*call, span)?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err(mir_error_at(
                        "MIR AllocNew route has unsupported Prelude ABI",
                        span,
                    ));
                }
                let expected_member = match kind {
                    jet_foundation::MIR::MirAllocatorKind::Arena => "arena.new",
                    jet_foundation::MIR::MirAllocatorKind::Bump => "bump.new",
                    jet_foundation::MIR::MirAllocatorKind::Pool => "pool.new",
                    jet_foundation::MIR::MirAllocatorKind::Fixed
                        if row.member == "fixed.over" =>
                    {
                        "fixed.over"
                    }
                    jet_foundation::MIR::MirAllocatorKind::Fixed => "fixed.new",
                };
                if row.member != expected_member {
                    return Err(mir_error_at(
                        "MIR AllocNew route does not match its checked allocator kind",
                        span,
                    ));
                }
                // Constructor arguments are checked TIR/MIR values. Evaluate
                // them even when this interpreter backend does not need their
                // native backing storage, so no side effect is discarded.
                for arg in args {
                    let _ = self.value(frame_index, arg.value, span)?;
                }
                Ok(RuntimeValue::Ambient(mir_runtime_owner_value(
                    MirAllocatorOwner::new(*kind, *inline_size),
                )))
            }
            MirSemanticOp::ColumnarRead {
                base,
                index,
                column,
                column_index,
                accessor,
            } => {
                let row = self.prelude_row(*accessor, span)?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err(mir_error_at(
                        "MIR columnar accessor has unsupported Prelude ABI",
                        span,
                    ));
                }
                let _ = field_name(self.program, *column)
                    .ok_or_else(|| mir_error_at("MIR columnar field ID is missing", span))?;
                let column_index = i64::try_from(*column_index)
                    .map_err(|_| mir_error_at("MIR columnar index does not fit Int", span))?;
                let values = vec![
                    runtime_to_data(self.value(frame_index, *base, span)?, span)?,
                    MirEvalValue::Int(column_index),
                    runtime_to_data(self.value(frame_index, *index, span)?, span)?,
                ];
                self.eval_prelude(*accessor, values, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirSemanticOp::StaticPreludeCall {
                call,
                args,
                type_args,
                owner_type_args,
            } => {
                let route = self.prelude_row(*call, span)?;
                if route.family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude
                    && route.module == "::jet_std::JetTaskGroup"
                    && matches!(route.member.as_str(), "new" | "with_limit" | "close")
                {
                    let expected_symbol = format!("jet_std::JetTaskGroup::{}", route.member);
                    if route.symbol.name() != expected_symbol {
                        return Err(mir_error_at(
                            "MIR TaskGroup route has a non-canonical symbol",
                            span,
                        ));
                    }
                    if route.abi != jet_foundation::MIR::MirPreludeAbi::Value
                        || !matches!(
                            &route.fallibility,
                            jet_foundation::MIR::MirCallFallibility::Infallible
                        )
                        || !type_args.is_empty()
                        || !owner_type_args.is_empty()
                    {
                        return Err(mir_error_at(
                            "MIR TaskGroup route has unsupported type arguments or ABI",
                            span,
                        ));
                    }
                    let member = route.member.clone();
                    let values = self.call_args(frame_index, args, span)?;
                    match member.as_str() {
                        "new" if values.is_empty() => {
                            let group = Arc::new(MirInterpreterTaskGroup {
                                children: Arc::new(
                                    crate::task_group::JetTaskGroupRuntime::new_defaulted(None),
                                ),
                            });
                            return Ok(RuntimeValue::Ambient(mir_runtime_owner_value(group)));
                        }
                        "with_limit" => {
                            let [value] = values.as_slice() else {
                                return Err(mir_error_at(
                                    "MIR TaskGroup.with_limit requires exactly one Int limit",
                                    span,
                                ));
                            };
                            let limit = int_value(runtime_to_data(value.clone(), span)?, span)?;
                            let group = Arc::new(MirInterpreterTaskGroup {
                                children: Arc::new(
                                    crate::task_group::JetTaskGroupRuntime::new_defaulted(Some(
                                        limit,
                                    )),
                                ),
                            });
                            return Ok(RuntimeValue::Ambient(mir_runtime_owner_value(group)));
                        }
                        "close" => {
                            let [value] = values.as_slice() else {
                                return Err(mir_error_at(
                                    "MIR TaskGroup.close requires exactly one group",
                                    span,
                                ));
                            };
                            let group = self.task_group_owner(value.clone(), span)?;
                            group.close();
                            return Ok(RuntimeValue::Data(MirEvalValue::Unit));
                        }
                        "new" => {
                            return Err(mir_error_at(
                                "MIR TaskGroup.new received unexpected arguments",
                                span,
                            ));
                        }
                        _ => unreachable!("checked TaskGroup route member"),
                    }
                }
                if route.family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude
                    && route.module == "::JetAtomic"
                    && matches!(route.member.as_str(), "new" | "try_new")
                {
                    let expected_symbol = format!("JetAtomic::{}", route.member);
                    if route.symbol.name() != expected_symbol {
                        return Err(mir_error_at(
                            "MIR Atomic constructor route has a non-canonical symbol",
                            span,
                        ));
                    }
                    if !type_args.is_empty() {
                        return Err(mir_error_at(
                            "MIR Atomic constructor received method type arguments",
                            span,
                        ));
                    }
                    let [jet_foundation::MIR::MirPreludeTypeArg::Type(inner_ty)] =
                        owner_type_args.as_slice()
                    else {
                        return Err(mir_error_at(
                            &format!(
                                "MIR Atomic.{} requires exactly one scalar owner type argument",
                                route.member
                            ),
                            span,
                        ));
                    };
                    let member = route.member.clone();
                    let values = self.call_args(frame_index, args, span)?;
                    if values.len() != 1 {
                        return Err(mir_error_at(
                            &format!("MIR Atomic.{member} requires exactly one value"),
                            span,
                        ));
                    }
                    let kind = mir_atomic_kind(inner_ty, span)?;
                    let value = values
                        .into_iter()
                        .next()
                        .expect("checked Atomic constructor arity");
                    let word = if member == "try_new" {
                        match mir_atomic_try_bits(value, kind, span)? {
                            Ok(word) => word,
                            Err(error) => {
                                return Ok(RuntimeValue::Result {
                                    ok: false,
                                    value: Box::new(RuntimeValue::Data(MirEvalValue::Struct {
                                        type_name: crate::Syntax::TYPE_ALLOC_ERROR.to_string(),
                                        fields: vec![
                                            (
                                                "requested_bytes".to_string(),
                                                MirEvalValue::Int(error.requested_bytes),
                                            ),
                                            (
                                                "allocator".to_string(),
                                                MirEvalValue::String(error.allocator),
                                            ),
                                        ],
                                    })),
                                })
                            }
                        }
                    } else {
                        mir_atomic_bits(value, kind, span)?
                    };
                    let atomic = RuntimeValue::Atomic(Arc::new(mir_atomic_cell(kind, word)));
                    if member == "try_new" {
                        return Ok(RuntimeValue::Result {
                            ok: true,
                            value: Box::new(atomic),
                        });
                    }
                    return Ok(atomic);
                }
                if route.module == "::jet_std::JetPool" && route.member == "new" {
                    if !args.is_empty()
                        || !type_args.is_empty()
                        || !matches!(
                            owner_type_args.as_slice(),
                            [jet_foundation::MIR::MirPreludeTypeArg::Type(_)]
                        )
                    {
                        return Err(mir_error_at(
                            "MIR Pool.new requires one owner type argument and no value arguments",
                            span,
                        ));
                    }
                    return Ok(RuntimeValue::Ambient(mir_runtime_owner_value(
                        MirPool::new(crate::Comptime::PoolRuntime::jet_std::JetPool::new()),
                    )));
                }
                if route.module.ends_with("jet_std::JetShared") && route.member == "new" {
                    let values = self.call_args(frame_index, args, span)?;
                    if values.len() != 1 {
                        return Err(mir_error_at(
                            "MIR Shared.new requires exactly one value",
                            span,
                        ));
                    }
                    return self.eval_shared_new(
                        values.into_iter().next().expect("checked Shared.new arity"),
                        span,
                    );
                }
                if route.module == "core.http" && route.member == "router" {
                    if !args.is_empty() {
                        return Err(mir_error_at(
                            "MIR HTTPRouter constructor received arguments",
                            span,
                        ));
                    }
                    return self.eval_http_router_ambient(
                        "http_router.new",
                        None,
                        Vec::new(),
                        result_ty,
                        span,
                    );
                }
                let codec_member = match (route.module.as_str(), route.member.as_str()) {
                    ("core.encoding.codec", "encode") => Some("encode"),
                    ("core.encoding.codec", "decode") => Some("decode"),
                    ("core.encoding.codec", "decode_typed") => Some("decode_typed"),
                    _ => None,
                };
                let values = self.call_args(frame_index, args, span)?;
                let values = values
                    .into_iter()
                    .map(|value| runtime_to_data(value, span))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                if let Some(member) = codec_member {
                    return self.eval_codec_prelude(member, values, type_args, result_ty, span);
                }
                self.eval_prelude(*call, values, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirSemanticOp::HostCall { call, args } => {
                let (module, member) = {
                    let route = self.prelude_row(*call, span)?;
                    (route.module.clone(), route.member.clone())
                };
                let values = self.call_args(frame_index, args, span)?;
                if module == "core.host"
                    && matches!(member.as_str(), "Pool.add" | "Pool.remove" | "Pool.ids")
                {
                    return self.eval_pool_host(&member, values, result_ty, span);
                }
                if module == "core.host"
                    && matches!(
                        member.as_str(),
                        "Shared.get"
                            | "Shared.set"
                            | "Shared.replace"
                            | "Shared.read"
                            | "Shared.edit"
                            | "Shared.guard_read"
                            | "Shared.guard_edit"
                            | "Shared.capture"
                            | "Shared.capture_with"
                            | "Shared.capture_txn"
                            | "Shared.try_replace"
                            | "Shared.read_txn"
                            | "Shared.edit_txn"
                            | "SharedSnapshot.value"
                    )
                {
                    return self.eval_shared_host(frame_index, &member, values, span);
                }
                self.eval_prelude_runtime_values(*call, values, result_ty, span)
            }
            MirSemanticOp::DecodeUnder {
                call,
                segment,
                inner,
            } => self.eval_prelude_values(frame_index, *call, &[*segment, *inner], result_ty, span),
            MirSemanticOp::HardwareCall {
                call,
                op,
                receiver,
                args,
            } => self.eval_hardware_call(frame_index, *call, op, *receiver, args, result_ty, span),
            MirSemanticOp::BuiltinMethod {
                call,
                receiver,
                receiver_place,
                args,
                ..
            } => {
                if let Some(value) = self.eval_atomic_builtin(
                    frame_index,
                    *call,
                    *receiver,
                    *receiver_place,
                    args,
                    span,
                )? {
                    return Ok(value);
                }
                if let Some(place) = receiver_place {
                    if let Some(value) = self.eval_mutating_builtin_place(
                        frame_index,
                        *call,
                        *place,
                        args,
                        result_ty,
                        span,
                    )? {
                        return Ok(value);
                    }
                }
                let mut values = vec![self.value(frame_index, *receiver, span)?];
                values.extend(
                    args.iter()
                        .map(|value| self.value(frame_index, *value, span))
                        .collect::<Result<Vec<_>, Diagnostic>>()?
                        .into_iter(),
                );
                self.eval_prelude_runtime_values(*call, values, result_ty, span)
            }
            MirSemanticOp::OptionLift2 {
                call,
                function,
                left,
                right,
            } => self.eval_prelude_runtime_values(
                *call,
                vec![
                    self.value(frame_index, *left, span)?,
                    self.value(frame_index, *right, span)?,
                    self.value(frame_index, *function, span)?,
                ],
                result_ty,
                span,
            ),
            MirSemanticOp::ClosureMethod {
                receiver,
                args,
                call,
            } => {
                let (family, module, member) = {
                    let row = self.prelude_row(*call, span)?;
                    (row.family, row.module.clone(), row.member.clone())
                };
                let receiver_value = self.value(frame_index, *receiver, span)?;
                if family == jet_foundation::MIR::MirPreludeFamily::ClosureMethod
                    && module == "core.event"
                {
                    let mut receiver = self.runtime_to_ct(receiver_value, span)?;
                    let args = self
                        .call_args(frame_index, args, span)?
                        .into_iter()
                        .map(|value| self.runtime_to_ct(value, span))
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    let result = crate::Comptime::eval_event_method(
                        &member,
                        &mut receiver,
                        &args,
                        span,
                        &mut |callback, args| {
                            let callback = RuntimeValue::Ambient(callback);
                            let args = args
                                .into_iter()
                                .map(|value| runtime_from_ct(value, span))
                                .collect::<Result<Vec<_>, Diagnostic>>()?;
                            let result = self.invoke_callback_args(callback, args, span)?;
                            self.runtime_to_ct(result, span)
                        },
                    )
                    .ok_or_else(|| {
                        mir_error_at(
                            &format!("MIR event method `{member}` has no EventLite adapter"),
                            span,
                        )
                    })??;
                    return runtime_from_ct(result, span);
                }
                let callback_args = self.call_args(frame_index, args, span)?;
                if family == jet_foundation::MIR::MirPreludeFamily::ClosureMethod
                    && Self::is_direct_collection_closure(&module, &member)
                {
                    return self.eval_collection_closure_method(
                        frame_index,
                        *receiver,
                        receiver_value,
                        &module,
                        &member,
                        callback_args,
                        result_ty,
                        span,
                    );
                }
                let mut values = vec![runtime_to_data(receiver_value, span)?];
                values.extend(
                    callback_args
                        .into_iter()
                        .map(|value| runtime_to_data(value, span))
                        .collect::<Result<Vec<_>, Diagnostic>>()?,
                );
                self.eval_prelude(*call, values, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirSemanticOp::HostBorrowCallback {
                callable,
                params: _,
            } => {
                let callable = self.value(frame_index, *callable, span)?;
                match callable {
                    RuntimeValue::Closure(_) | RuntimeValue::Data(MirEvalValue::Closure(_)) => {
                        let _ = self.closure_handle(callable.clone(), span)?;
                        Ok(callable)
                    }
                    _ => Err(mir_error_at(
                        "MIR host-borrow callback is not callable",
                        span,
                    )),
                }
            }
            MirSemanticOp::TextPatternMatch {
                call,
                subject,
                parts,
            } => {
                let row = self.prelude_row(*call, span)?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err(mir_error_at(
                        "MIR text pattern route has unsupported Prelude ABI",
                        span,
                    ));
                }
                let subject = runtime_to_data(self.value(frame_index, *subject, span)?, span)?;
                let pattern = parts
                    .iter()
                    .map(|part| match part {
                        MirTextPatternPart::Literal(value) => {
                            Ok(JetTextMatchPart::Literal(value.as_str()))
                        }
                        MirTextPatternPart::Hole { kind, .. } => Ok(JetTextMatchPart::Hole {
                            kind: text_hole_kind(*kind),
                        }),
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                let MirEvalValue::String(subject) = subject else {
                    return Err(mir_error_at("MIR text pattern subject is not String", span));
                };
                let captures = jet_text_pattern_match(&subject, &pattern);
                pattern_match_carrier(captures, result_ty, span)
            }
            MirSemanticOp::BinaryPatternMatch {
                call,
                subject,
                parts,
            } => {
                let row = self.prelude_row(*call, span)?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err(mir_error_at(
                        "MIR binary pattern route has unsupported Prelude ABI",
                        span,
                    ));
                }
                let subject = runtime_to_data(self.value(frame_index, *subject, span)?, span)?;
                let pattern = parts
                    .iter()
                    .map(|part| match part {
                        MirBinaryPatternPart::Literal(value) => {
                            Ok(JetBinMatchPart::Lit(value.as_slice()))
                        }
                        MirBinaryPatternPart::Bits { width, little, .. } => {
                            Ok(JetBinMatchPart::Bits {
                                width: usize::from(*width),
                                little: *little,
                            })
                        }
                        MirBinaryPatternPart::Rest { .. } => Ok(JetBinMatchPart::Rest),
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                let MirEvalValue::Bytes(subject) = subject else {
                    return Err(mir_error_at(
                        "MIR binary pattern subject is not Bytes",
                        span,
                    ));
                };
                let captures = jet_binary_pattern_match(&subject, &pattern);
                pattern_match_carrier(captures, result_ty, span)
            }
            MirSemanticOp::NumericMethod { call, receiver } => {
                let values = vec![runtime_to_data(
                    self.value(frame_index, *receiver, span)?,
                    span,
                )?];
                self.eval_prelude(*call, values, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirSemanticOp::NumericBinaryMethod {
                call,
                receiver,
                argument,
            } => {
                let values = vec![
                    runtime_to_data(self.value(frame_index, *receiver, span)?, span)?,
                    runtime_to_data(self.value(frame_index, *argument, span)?, span)?,
                ];
                self.eval_prelude(*call, values, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirSemanticOp::OverflowOption {
                call,
                left,
                right,
                location,
            } => {
                let left = runtime_to_data(self.value(frame_index, *left, span)?, span)?;
                let right = runtime_to_data(self.value(frame_index, *right, span)?, span)?;
                let values = self.overflow_option_args(*call, left, right, *location, span)?;
                self.eval_prelude(*call, values, result_ty, span)
                    .map(RuntimeValue::Data)
            }
            MirSemanticOp::HandleMethod {
                call,
                receiver,
                args,
                frame_schedule,
                frame_schedule_derivation,
            } => {
                let route = self.prelude_row(*call, span)?.clone();
                let receiver_value = self.value(frame_index, *receiver, span)?;
                if route.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                    && route.module == "core.channels"
                {
                    return self.eval_channel_handle(
                        &route,
                        receiver_value,
                        args,
                        frame_index,
                        span,
                    );
                }
                if route.module == "core.handle"
                    && matches!(
                        route.member.as_str(),
                        "clock.now" | "clock.tick" | "clock.advance" | "clock.wait"
                    )
                {
                    return self.eval_clock_handle(
                        &route.member,
                        receiver_value,
                        args,
                        frame_index,
                        span,
                    );
                }
                if route.member == "tcp_listener.local_addr" {
                    let raw = match receiver_value {
                        RuntimeValue::Data(MirEvalValue::Struct { fields, .. }) => fields
                            .iter()
                            .find_map(|(name, value)| (name == "id").then_some(value))
                            .and_then(|value| match value {
                                MirEvalValue::Int(raw) => Some(*raw),
                                _ => None,
                            })
                            .ok_or_else(|| {
                                mir_error_at("MIR TCP listener carrier has no identity", span)
                            })?,
                        _ => {
                            return Err(mir_error_at(
                                "MIR TCP listener receiver is not a listener carrier",
                                span,
                            ))
                        }
                    };
                    let result = crate::Comptime::try_ambient_mir_handle(
                        "tcp_listener.local_addr",
                        Some(raw),
                        Vec::new(),
                        span,
                    )
                    .ok_or_else(|| {
                        mir_error_at("MIR TCP listener has no interpreter host binding", span)
                    })??;
                    return match result {
                        crate::Comptime::AmbientMirHandleResult::Value(value) => {
                            Ok(RuntimeValue::Data(value))
                        }
                        _ => Err(mir_error_at(
                            "MIR TCP listener address host returned a handle",
                            span,
                        )),
                    };
                }
                if route.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                    && route.module == "deterministic_world"
                {
                    let RuntimeValue::ForeignHandle { token } = &receiver_value else {
                        return Err(mir_error_at(
                            "MIR deterministic world method receiver is not a world handle",
                            span,
                        ));
                    };
                    if token.handle_id() != MIR_DETERMINISTIC_WORLD_HANDLE {
                        return Err(mir_error_at(
                            "MIR deterministic world method receiver has the wrong handle identity",
                            span,
                        ));
                    }
                    let raw = token.raw().ok_or_else(|| {
                        mir_error_at("MIR deterministic world handle was moved", span)
                    })?;
                    let value = match route.member.as_str() {
                        "now" if args.is_empty() => with_deterministic_world(raw, |world| {
                            MirEvalValue::Int(crate::scheduler::jet_world_now(world))
                        }),
                        "advance" => {
                            let [duration] = args.as_slice() else {
                                return Err(mir_error_at(
                                    "MIR deterministic world advance expects one duration",
                                    span,
                                ));
                            };
                            let duration = int_value(
                                runtime_to_data(self.value(frame_index, *duration, span)?, span)?,
                                span,
                            )?;
                            with_deterministic_world(raw, |world| {
                                MirEvalValue::Int(crate::scheduler::jet_world_advance(
                                    world, duration,
                                ))
                            })
                        }
                        "wait_idle" if args.is_empty() => with_deterministic_world(raw, |world| {
                            crate::scheduler::jet_world_wait_idle(world);
                            MirEvalValue::Unit
                        }),
                        "history" if args.is_empty() => with_deterministic_world(raw, |world| {
                            MirEvalValue::String(crate::scheduler::jet_world_history(world))
                        }),
                        _ => {
                            return Err(mir_error_at(
                                &format!(
                                    "MIR deterministic world route `{}` has invalid arguments",
                                    route.member
                                ),
                                span,
                            ));
                        }
                    }
                    .ok_or_else(|| {
                        mir_error_at(
                            "MIR deterministic world handle is not live in this world scope",
                            span,
                        )
                    })?;
                    return Ok(RuntimeValue::Data(value));
                }
                if route.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                    && route.module == "core.tasks"
                    && matches!(route.member.as_str(), "join" | "detach")
                {
                    if !args.is_empty() {
                        return Err(mir_error_at(
                            "MIR task lifecycle route received unexpected arguments",
                            span,
                        ));
                    }
                    if route.member == "detach" {
                        let RuntimeValue::Ambient(value) = receiver_value else {
                            return Err(mir_error_at(
                                "MIR task detach receiver has no scheduler owner",
                                span,
                            ));
                        };
                        let task = mir_interpreter_task_from_ct(&value, span)?;
                        drop(task.take_entry(span)?);
                        return Ok(RuntimeValue::Data(MirEvalValue::Unit));
                    }
                    let flatten_result = route.symbol.name() == "jet_std::jet_task_join_result";
                    return self.eval_task_join(receiver_value, flatten_result, span);
                }
                if route.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                    && route.module == "core.handle"
                    && matches!(
                        route.member.as_str(),
                        "Arena.alloc"
                            | "Arena.try_alloc"
                            | "Arena.reset"
                            | "Bump.alloc"
                            | "Bump.try_alloc"
                            | "Bump.reset"
                            | "Pool.alloc"
                            | "Pool.try_alloc"
                            | "Pool.reset"
                            | "Fixed.alloc"
                            | "Fixed.try_alloc"
                            | "Fixed.reset"
                    )
                {
                    let mut values = vec![receiver_value];
                    values.extend(
                        args.iter()
                            .map(|value| self.value(frame_index, *value, span))
                            .collect::<Result<Vec<_>, Diagnostic>>()?,
                    );
                    return self.eval_allocator_runtime(&route.member, values, result_ty, span);
                }
                if let RuntimeValue::Ambient(value) = &receiver_value {
                    let values = args
                        .iter()
                        .map(|id| {
                            let value = self.value(frame_index, *id, span)?;
                            self.runtime_to_ct(value, span)
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    if route.module == "core.ui"
                        && route.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                    {
                        let (_, method) = route.member.split_once('.').ok_or_else(|| {
                            mir_error_at("UI backend route has no checked receiver name", span)
                        })?;
                        if let Some(result) = mir_ui_backend_method(value, method, &values, span) {
                            return result.and_then(|value| runtime_from_ct(value, span));
                        }
                    }
                    let result_type = result_ty.map(crate::Comptime::MirBridge::mir_to_ast_type);
                    let result = crate::Comptime::AppLite::apply_web_handle(
                        value,
                        &route.member,
                        &values,
                        span,
                        result_type.as_ref(),
                    )
                    .ok_or_else(|| {
                        mir_error_at("MIR native handle has no checked method adapter", span)
                    })??;
                    return runtime_from_ct(result, span);
                }
                if route.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                    && route.module == "core.web.app"
                {
                    let RuntimeValue::App(handle) = receiver_value else {
                        return Err(mir_error_at(
                            "MIR App method receiver is not an App runtime handle",
                            span,
                        ));
                    };
                    let callback_index = match route.member.as_str() {
                        "route" | "page" | "layout" => 1,
                        "loader" => 0,
                        "pending" | "not_found" | "error" => 0,
                        "action" | "form" | "data" | "mount" => 1,
                        _ => usize::MAX,
                    };
                    let callback_runtime = if callback_index != usize::MAX {
                        let value_id = args.get(callback_index).ok_or_else(|| {
                            mir_error_at(
                                &format!(
                                    "MIR App method `{}` has no callback argument",
                                    route.member
                                ),
                                span,
                            )
                        })?;
                        self.value(frame_index, *value_id, span)?
                    } else {
                        RuntimeValue::Moved
                    };
                    let (callback_types, callback_return_type) = if callback_index == usize::MAX {
                        (Vec::new(), None)
                    } else {
                        let closure = self.closure_handle(callback_runtime.clone(), span)?;
                        let function = program_function(self.program, closure.function)?;
                        (
                            function
                                .params
                                .iter()
                                .map(|param| param.ty.clone())
                                .collect(),
                            Some(function.return_type.clone()),
                        )
                    };
                    let mut values = Vec::with_capacity(args.len());
                    for (index, value_id) in args.iter().enumerate() {
                        let value = self.value(frame_index, *value_id, span)?;
                        if index == callback_index {
                            values.push(self.standalone_closure_value(value, span)?);
                        } else {
                            values.push(crate::Comptime::MirBridge::mir_to_ct_value(
                                runtime_to_data(self.materialize_runtime(value, span)?, span)?,
                                span,
                            )?);
                        }
                    }
                    let result = crate::Comptime::AppLite::app_method_runtime(
                        &handle,
                        &route.member,
                        &values,
                        &callback_types,
                        callback_return_type.as_ref(),
                        span,
                    )?;
                    return match result {
                        crate::Comptime::AppLite::AppRuntimeResult::App(handle) => {
                            Ok(RuntimeValue::App(handle))
                        }
                        crate::Comptime::AppLite::AppRuntimeResult::Value(value) => {
                            Ok(RuntimeValue::Data(
                                crate::Comptime::MirBridge::ct_to_mir_value(value, span)?,
                            ))
                        }
                    };
                }
                if route.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                    && route.module == "core.time"
                    && route.member == "duration_new"
                {
                    let value = runtime_to_data(
                        self.materialize_runtime(receiver_value.clone(), span)?,
                        span,
                    )?;
                    let unit = args
                        .first()
                        .ok_or_else(|| {
                            mir_error_at(
                                "MIR Duration constructor has no checked DurationUnit operand",
                                span,
                            )
                        })
                        .and_then(|unit| {
                            self.value(frame_index, *unit, span).and_then(|value| {
                                runtime_to_data(self.materialize_runtime(value, span)?, span)
                            })
                        })?;
                    let float = route.symbol.name() == "jet_duration_from_float";
                    return Ok(RuntimeValue::Data(eval_mir_duration_constructor(
                        &value, &unit, float,
                    )));
                }
                if route.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                    && route.module == "core.time"
                    && route.member.split_once('.').is_some_and(|(kind, method)| {
                        crate::Codegen::TIR::is_civil_time_method_name(Some(kind), method)
                    })
                {
                    let (kind, method) = route.member.split_once('.').ok_or_else(|| {
                        mir_error_at(
                            &format!(
                                "MIR civil-time route member `{}` is malformed",
                                route.member
                            ),
                            span,
                        )
                    })?;
                    if kind.is_empty() || method.is_empty() {
                        return Err(mir_error_at(
                            &format!(
                                "MIR civil-time route member `{}` is malformed",
                                route.member
                            ),
                            span,
                        ));
                    }
                    let receiver_value = self.materialize_runtime(receiver_value.clone(), span)?;
                    let receiver_ct = self.runtime_to_ct(receiver_value, span)?;
                    let values = args
                        .iter()
                        .map(|value| self.value(frame_index, *value, span))
                        .collect::<Result<Vec<_>, Diagnostic>>()?
                        .into_iter()
                        .map(|value| {
                            let value = self.materialize_runtime(value, span)?;
                            self.runtime_to_ct(value, span)
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    let result = crate::Comptime::Builtins::apply_method(
                        &receiver_ct,
                        method,
                        values,
                        span,
                    )?;
                    return runtime_from_ct(result, span);
                }
                if (route.module == "core.handle" && route.member.starts_with("game."))
                    || (route.module == "core.game" && route.member == "backend_headless")
                {
                    let values = args
                        .iter()
                        .map(|value| self.value(frame_index, *value, span))
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    return self.eval_game_handle(
                        &route.module,
                        &route.member,
                        receiver_value,
                        values,
                        frame_schedule.as_ref(),
                        frame_schedule_derivation.as_ref(),
                        span,
                    );
                }
                if route.member.starts_with("process.spec.")
                    || matches!(
                        route.member.as_str(),
                        "process.child.wait" | "process.stdin_write" | "process.stdin_close"
                    )
                {
                    let raw = if route.member.starts_with("process.spec.") {
                        let RuntimeValue::ProcessSpec { raw } = &receiver_value else {
                            return Err(mir_error_at(
                                "MIR process.spec route receiver is not a ProcessSpec",
                                span,
                            ));
                        };
                        if *raw == 0 {
                            return Err(mir_error_at(
                                "MIR ProcessSpec carrier has an invalid interpreter identity",
                                span,
                            ));
                        }
                        *raw
                    } else {
                        let expected_receiver = match route.member.as_str() {
                            "process.child.wait" => self.process_handle_id("ProcessChild", span)?,
                            "process.stdin_write" | "process.stdin_close" => {
                                self.process_handle_id("ProcessStdin", span)?
                            }
                            _ => {
                                return Err(mir_error_at(
                                    "MIR process route has no checked receiver identity",
                                    span,
                                ))
                            }
                        };
                        let RuntimeValue::ForeignHandle { token } = &receiver_value else {
                            return Err(mir_error_at(
                                "MIR process route receiver is not a lifecycle handle",
                                span,
                            ));
                        };
                        if token.handle_id() != expected_receiver {
                            return Err(mir_error_at(
                                "MIR process route receiver has the wrong lifecycle identity",
                                span,
                            ));
                        }
                        if route.member == "process.stdin_close" {
                            token
                                .take_raw()
                                .map(|(_, raw)| raw)
                                .ok_or_else(|| mir_error_at("MIR process handle was moved", span))?
                        } else {
                            token
                                .raw()
                                .ok_or_else(|| mir_error_at("MIR process handle was moved", span))?
                        }
                    };
                    if route.member == "process.stdin_close" {
                        if let RuntimeValue::ForeignHandle { token } = &receiver_value {
                            self.forget_process_stdin_token(token);
                        }
                    }
                    let values = args
                        .iter()
                        .map(|value| self.value(frame_index, *value, span))
                        .collect::<Result<Vec<_>, Diagnostic>>()?
                        .into_iter()
                        .map(|value| runtime_to_data(value, span))
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    let result = crate::Comptime::try_ambient_mir_handle(
                        &route.member,
                        Some(raw),
                        values,
                        span,
                    )
                    .ok_or_else(|| {
                        mir_error_at(
                            "MIR process handle operation has no interpreter ambient host binding",
                            span,
                        )
                    })??;
                    return match result {
                        crate::Comptime::AmbientMirHandleResult::Handle(raw)
                            if route.member == "process.spec.stdin" =>
                        {
                            if raw == 0 {
                                return Err(mir_error_at(
                                    "MIR ProcessSpec ambient binding returned an invalid identity",
                                    span,
                                ));
                            }
                            Ok(RuntimeValue::ProcessSpec { raw })
                        }
                        crate::Comptime::AmbientMirHandleResult::Handle(raw)
                            if route.member == "process.spec.spawn" =>
                        {
                            if raw == 0 {
                                return Err(mir_error_at(
                                    "MIR ProcessChild ambient binding returned an invalid identity",
                                    span,
                                ));
                            }
                            let handle = self.process_handle_id("ProcessChild", span)?;
                            Ok(RuntimeValue::ForeignHandle {
                                token: MirHandleToken::new(handle, raw),
                            })
                        }
                        crate::Comptime::AmbientMirHandleResult::Handle(_) => Err(mir_error_at(
                            "MIR process operation returned an unexpected opaque handle",
                            span,
                        )),
                        crate::Comptime::AmbientMirHandleResult::Value(value) => {
                            Ok(RuntimeValue::Data(value))
                        }
                    };
                }
                if route.module == "core.web" && route.member == "openapi" {
                    if !args.is_empty() {
                        return Err(mir_error_at(
                            "MIR HTTPRouter OpenAPI export received arguments",
                            span,
                        ));
                    }
                    return self.eval_http_router_ambient(
                        "http_router.openapi",
                        Some(receiver_value),
                        Vec::new(),
                        result_ty,
                        span,
                    );
                }
                if matches!(
                    route.member.as_str(),
                    "stream.with_event_time"
                        | "stream.with_event_time.datetime"
                        | "stream.key_by"
                        | "stream.window"
                ) {
                    let mut values = vec![receiver_value];
                    values.extend(
                        args.iter()
                            .map(|value| self.value(frame_index, *value, span))
                            .collect::<Result<Vec<_>, Diagnostic>>()?,
                    );
                    return self.eval_prelude_runtime_values(*call, values, result_ty, span);
                }
                if matches!(
                    route.member.as_str(),
                    "realtime.next_deadline"
                        | "realtime.receipt"
                        | "realtime.cancel"
                        | "realtime.is_cancelled"
                ) {
                    if route.abi != jet_foundation::MIR::MirPreludeAbi::Value
                        || !matches!(
                            route.fallibility,
                            jet_foundation::MIR::MirCallFallibility::Infallible
                        )
                    {
                        return Err(mir_error_at(
                            "MIR real-time lifecycle route has unsupported ABI or fallibility",
                            span,
                        ));
                    }
                    let state = Self::realtime_state(&receiver_value).ok_or_else(|| {
                        mir_error_at(
                            "MIR real-time lifecycle receiver is not a callback stream",
                            span,
                        )
                    })?;
                    let member = route.member.clone();
                    return self.eval_realtime_handle(&member, state, args.len(), span);
                }
                let mut values = vec![receiver_value];
                values.extend(
                    args.iter()
                        .map(|value| self.value(frame_index, *value, span))
                        .collect::<Result<Vec<_>, Diagnostic>>()?,
                );
                if let Some(metadata) = route.db_metadata.as_ref() {
                    values.push(RuntimeValue::Data(MirEvalValue::String(metadata.to_wire())));
                }
                self.eval_prelude_runtime_values(*call, values, result_ty, span)
            }
            MirSemanticOp::HttpRouterRegister {
                call,
                receiver,
                path,
                handler,
                method,
                handler_param_names,
                contract_json,
                location,
            } => {
                let route = self.prelude_row(*call, span)?;
                if route.abi != jet_foundation::MIR::MirPreludeAbi::Control {
                    return Err(mir_error_at(
                        "MIR HTTP router registration has unsupported Prelude ABI",
                        span,
                    ));
                }
                let receiver = self.value(frame_index, *receiver, span)?;
                let path = self.value(frame_index, *path, span)?;
                let handler = self.value(frame_index, *handler, span)?;
                let source_file = self.source_file_path(location.file, span)?;
                let values = vec![
                    RuntimeValue::Data(MirEvalValue::String(method.as_str().to_string())),
                    path,
                    handler,
                    RuntimeValue::Data(source_file),
                    RuntimeValue::Data(MirEvalValue::Int(i64::from(location.line))),
                    RuntimeValue::Data(MirEvalValue::String(contract_json.clone())),
                    RuntimeValue::Data(MirEvalValue::List(
                        handler_param_names
                            .iter()
                            .cloned()
                            .map(MirEvalValue::String)
                            .collect(),
                    )),
                ];
                if matches!(route.member.as_str(), "mux_add" | "mux_add_zero") {
                    return self.eval_http_route_ambient(
                        "http_mux.register",
                        Some(receiver),
                        values,
                        result_ty,
                        span,
                    );
                }
                if route.member != "router_register" {
                    return Err(mir_error_at(
                        "MIR HTTP route registration has an unknown checked route member",
                        span,
                    ));
                }
                self.eval_http_router_ambient(
                    "http_router.register",
                    Some(receiver),
                    values,
                    result_ty,
                    span,
                )
            }
            MirSemanticOp::CoreClosureCall {
                call,
                kind,
                values,
                closure,
                site,
                label,
            } => self.eval_core_closure_call(
                frame_index,
                *call,
                kind.clone(),
                values,
                *closure,
                *site,
                label,
                result_ty,
                span,
            ),
            MirSemanticOp::TaskGroup { call, kind, tasks } => {
                let values = tasks
                    .iter()
                    .map(|value| self.value(frame_index, *value, span))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                self.eval_task_group(*call, *kind, values, result_ty, span)
            }
            MirSemanticOp::Select {
                call,
                kind: _,
                values,
            } => {
                let values = values
                    .iter()
                    .map(|value| self.value(frame_index, *value, span))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                self.eval_channel_select(*call, values, result_ty, span)
            }
            MirSemanticOp::CarrierFact {
                call,
                receiver,
                field,
                notes,
            } => self.eval_carrier_fact(
                *call,
                self.value(frame_index, *receiver, span)?,
                *field,
                *notes,
                result_ty,
                span,
            ),
            MirSemanticOp::GcEdit {
                call,
                root,
                edges,
                edit,
                index,
                kind,
                site,
            } => {
                let root = self.value(frame_index, *root, span)?;
                let edges = edges
                    .iter()
                    .map(|value| self.value(frame_index, *value, span))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                let edit = self.value(frame_index, *edit, span)?;
                let index = index
                    .map(|value| self.value(frame_index, value, span))
                    .transpose()?;
                self.eval_gc_semantic(*call, root, edges, edit, index, *kind, *site, span)
            }
            MirSemanticOp::TypedTextInterp {
                call,
                kind,
                literals,
                holes,
            } => self.eval_typed_text_interp(
                frame_index,
                *call,
                *kind,
                literals,
                holes,
                result_ty,
                span,
            ),
            MirSemanticOp::CCallback {
                call,
                callback,
                lambda,
            } => {
                let adapter = self
                    .program
                    .callbacks
                    .iter()
                    .find(|adapter| adapter.id == *callback)
                    .ok_or_else(|| {
                        mir_error_at("MIR callback adapter ID has no canonical row", span)
                    })?;
                let function = program_function(self.program, adapter.function)?;
                if function.params.len() != adapter.params.len()
                    || function
                        .params
                        .iter()
                        .zip(&adapter.params)
                        .any(|(actual, expected)| {
                            actual.index != expected.index
                                || actual.access != expected.access
                                || !actual.ty.same_checked_type(&expected.ty)
                        })
                {
                    return Err(mir_error_at(
                        "MIR C callback adapter parameters disagree with its function row",
                        span,
                    ));
                }
                match (&adapter.return_type, &function.declared_return) {
                    (Some(expected), Some(actual)) if !expected.same_checked_type(actual) => {
                        return Err(mir_error_at(
                            "MIR C callback adapter return type disagrees with its function row",
                            span,
                        ));
                    }
                    (Some(_), None) if !function.return_type.is_unit() => {
                        return Err(mir_error_at(
                            "MIR C callback adapter return type is absent from its function row",
                            span,
                        ));
                    }
                    (None, Some(_)) => {
                        return Err(mir_error_at(
                            "MIR C callback adapter is missing its function return type",
                            span,
                        ));
                    }
                    (None, None) | (Some(_), None) | (Some(_), Some(_)) => {}
                }
                let row = self.prelude_row(*call, span)?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err(mir_error_at(
                        "MIR C callback route has unsupported Prelude ABI",
                        span,
                    ));
                }
                let value = self.value(frame_index, *lambda, span)?;
                let (function, _, _) = self.closure_parts(value.clone(), span)?;
                if function != adapter.function {
                    return Err(mir_error_at(
                        "MIR C callback lambda does not match its adapter function",
                        span,
                    ));
                }
                Ok(value)
            }
            MirSemanticOp::PolicyFunction { policy, values }
            | MirSemanticOp::InterruptFunction {
                interrupt: policy,
                values,
            } => {
                let target = self.value(frame_index, *policy, span)?;
                let (function, captures, capture_cells) = self.closure_parts(target, span)?;
                let values = values
                    .iter()
                    .map(|value| self.value(frame_index, *value, span))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                self.invoke_function_with_capture_cells(
                    function,
                    values,
                    captures,
                    capture_cells,
                    span,
                )
            }
        }
    }

    fn eval_require_stop(
        &mut self,
        frame_index: usize,
        call: jet_foundation::MIR::MirPreludeCallId,
        kind: MirRequireKind,
        condition: Option<MirValueId>,
        location: MirPanicLoc,
        context: &MirPanicContext,
        values: &[MirValueId],
        _result_ty: Option<&MirType>,
        _always_stops: bool,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match kind {
            MirRequireKind::Require if values.len() <= 1 => {}
            MirRequireKind::RequireEq if values.len() == 2 => {}
            MirRequireKind::Panic if values.len() == 1 => {}
            MirRequireKind::Require | MirRequireKind::RequireEq | MirRequireKind::Panic => {
                return Err(mir_error_at(
                    "MIR RequireStop diagnostic payload has invalid shape",
                    span,
                ))
            }
        }
        let row = self.prelude_row(call, span)?;
        if row.abi != jet_foundation::MIR::MirPreludeAbi::Control
            || !matches!(
                row.fallibility,
                jet_foundation::MIR::MirCallFallibility::Infallible
            )
        {
            return Err(mir_error_at(
                "MIR RequireStop route has unsupported ABI or fallibility",
                span,
            ));
        }

        let diagnostics = values
            .iter()
            .map(|value| runtime_to_data(self.value(frame_index, *value, span)?, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let condition = match kind {
            MirRequireKind::Require | MirRequireKind::RequireEq => {
                let condition = condition.ok_or_else(|| {
                    mir_error_at("MIR RequireStop route has no checked condition", span)
                })?;
                match runtime_to_data(self.value(frame_index, condition, span)?, span)? {
                    MirEvalValue::Bool(value) => Some(value),
                    _ => return Err(mir_error_at("MIR RequireStop condition is not Bool", span)),
                }
            }
            MirRequireKind::Panic => {
                if condition.is_some() {
                    return Err(mir_error_at(
                        "MIR panic carries an unexpected checked condition",
                        span,
                    ));
                }
                None
            }
        };
        let failure = match kind {
            MirRequireKind::Require => {
                if condition == Some(true) {
                    None
                } else {
                    let message = match diagnostics.as_slice() {
                        [] => "condition failed".to_string(),
                        [MirEvalValue::String(message)] => message.clone(),
                        [_] => return Err(mir_error_at("MIR require message is not String", span)),
                        _ => unreachable!("MIR require arity was validated above"),
                    };
                    Some(message)
                }
            }
            MirRequireKind::RequireEq => {
                if condition == Some(true) {
                    None
                } else {
                    let [left, right] = diagnostics.as_slice() else {
                        unreachable!("MIR require_eq arity was validated above")
                    };
                    Some(format!(
                        "expected: {}, got: {}",
                        mir_show(right),
                        mir_show(left)
                    ))
                }
            }
            MirRequireKind::Panic => {
                let [message] = diagnostics.as_slice() else {
                    unreachable!("MIR panic arity was validated above")
                };
                let MirEvalValue::String(message) = message else {
                    return Err(mir_error_at("MIR panic message is not String", span));
                };
                Some(message.clone())
            }
        };
        if let Some(message) = failure {
            return self.runtime_require_failure(frame_index, location, context, &message, span);
        }
        Ok(RuntimeValue::Data(MirEvalValue::Unit))
    }

    fn runtime_require_failure(
        &mut self,
        frame_index: usize,
        location: MirPanicLoc,
        context: &MirPanicContext,
        message: &str,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if !self.config.runtime_execution {
            return Err(crate::Comptime::comptime_panic(message, span));
        }
        if crate::scheduler::jet_scheduler_in_task() {
            std::panic::resume_unwind(Box::new(message.to_owned()));
        }
        self.last_runtime_stop = Some("E3001".to_string());
        let file = self
            .program
            .source_files
            .iter()
            .find(|source| source.id == location.file)
            .map(|source| source.path.as_str())
            .ok_or_else(|| mir_error_at("MIR source file ID has no source-file row", span))?;
        let locals = context
            .locals
            .iter()
            .filter_map(|(name, local)| {
                self.frames[frame_index]
                    .locals
                    .get(local)
                    .cloned()
                    .map(|value| {
                        runtime_to_data(value, span)
                            .map(|value| format!("{name} = {}", mir_show(&value)))
                    })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?
            .join(", ");
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            "E3001",
            file,
            location.line,
            &context.function,
            &context.source_line,
            location.column,
            context.caret,
            message,
            &locals,
        );
        self.stderr.push_str(&report.rendered);
        self.exit_code = report.exit_code;
        Err(crate::Sema::Diagnostics::soft_exit(
            report.exit_code.to_string(),
            "require/panic stop".to_string(),
            Some(span),
        ))
    }

    fn eval_task_spawn(
        &mut self,
        frame_index: usize,
        values: &[MirValueId],
        closure: Option<MirValueId>,
        route_module: &str,
        route_member: &str,
        route_symbol: &str,
        label: &str,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let grouped = match route_member {
            "spawn" => false,
            "spawn_grouped" => true,
            _ => {
                return Err(mir_error_at(
                    "MIR task spawn route has an unknown member",
                    span,
                ));
            }
        };
        let expected_symbol = if grouped {
            "jet_std::JetTaskGroup::spawn_at"
        } else {
            "jet_std::JetTask::spawn_at"
        };
        if route_module != "core.tasks" || route_symbol != expected_symbol {
            return Err(mir_error_at(
                "MIR task spawn route has a non-canonical module or symbol",
                span,
            ));
        }
        let expected_values = if grouped { 3 } else { 2 };
        if values.len() != expected_values {
            return Err(mir_error_at(
                "MIR task spawn route has invalid value arity",
                span,
            ));
        }
        let Some(closure) = closure else {
            return Err(mir_error_at("MIR task spawn has no checked callback", span));
        };
        let mut value_index = 0;
        let group = if grouped {
            let group = self.task_group_owner(self.value(frame_index, values[0], span)?, span)?;
            value_index += 1;
            Some(group)
        } else {
            None
        };
        let spawn_site = int_value(
            runtime_to_data(self.value(frame_index, values[value_index], span)?, span)?,
            span,
        )?;
        let spawn_site = usize::try_from(spawn_site)
            .map_err(|_| mir_error_at("MIR task spawn site must be non-negative", span))?;
        let source_label = match runtime_to_data(
            self.value(frame_index, values[value_index + 1], span)?,
            span,
        )? {
            MirEvalValue::String(value) if value == label => value,
            MirEvalValue::String(_) => {
                return Err(mir_error_at(
                    "MIR task spawn label does not match its route",
                    span,
                ));
            }
            _ => return Err(mir_error_at("MIR task spawn label must be a String", span)),
        };
        let callback =
            self.standalone_closure_value(self.value(frame_index, closure, span)?, span)?;
        let control = crate::scheduler::JetTaskControl::new();
        let permit = if let Some(group) = &group {
            let waiter = crate::scheduler::ParkSlot::new();
            let permit = group
                .children
                .acquire_with(waiter, |waiter| {
                    crate::scheduler::jet_scheduler_task_group_wait(waiter);
                    Ok::<(), ()>(())
                })
                .map_err(|_: ()| mir_error_at("MIR task-group admission wait failed", span))?;
            permit
        } else {
            None
        };
        let inherited_deadline = crate::scheduler::jet_ctx_deadline_ms();
        let callback_span = span;
        let handle = crate::scheduler::jet_scheduler_spawn_blocking_with_control_at(
            spawn_site,
            &source_label,
            move || {
                let _permit = permit;
                let _deadline_guard =
                    inherited_deadline.map(crate::scheduler::jet_ctx_push_deadline);
                match crate::Comptime::try_ambient_standalone_closure(
                    &callback,
                    Vec::new(),
                    callback_span,
                ) {
                    Some(Ok(value)) => value,
                    Some(Err(error)) => panic!("{}: {}", error.code, error.what),
                    None => panic!("MIR task callback has no standalone host"),
                }
            },
            control.clone(),
        );
        let task = Arc::new(MirInterpreterTask {
            handle: Mutex::new(Some(handle)),
            control,
        });
        if let Some(group) = group {
            group.children.register(task.clone());
        }
        Ok(RuntimeValue::Ambient(mir_interpreter_task_carrier(task)))
    }

    fn eval_core_closure_call(
        &mut self,
        frame_index: usize,
        call: MirPreludeCallId,
        kind: MirCoreClosureKind,
        values: &[MirValueId],
        closure: Option<MirValueId>,
        site: jet_foundation::MIR::MirSiteId,
        label: &str,
        _result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match &kind {
            MirCoreClosureKind::Spawn => {}
            MirCoreClosureKind::Serve => {
                if values.len() != 1 {
                    return Err(mir_error_at(
                        "MIR Serve CoreClosureCall expects one address value",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::Realtime => {
                if values.len() != 2 || closure.is_none() {
                    return Err(mir_error_at(
                        "MIR Realtime CoreClosureCall expects rate, frames, and a callback",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::OnInterrupt => {
                if values.len() != 1 || closure.is_some() {
                    return Err(mir_error_at(
                        "MIR OnInterrupt CoreClosureCall expects one callback value",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::Guard => {
                if !values.is_empty() {
                    return Err(mir_error_at(
                        "MIR Guard CoreClosureCall expects no value operands",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::OnCommit => {
                if values.len() != 1 {
                    return Err(mir_error_at(
                        "MIR OnCommit CoreClosureCall expects one transaction handle",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::OnRollback => {
                if values.len() != 1 {
                    return Err(mir_error_at(
                        "MIR OnRollback CoreClosureCall expects one transaction handle",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::ReactiveDerived
            | MirCoreClosureKind::ReactiveEffect
            | MirCoreClosureKind::UiMount => {
                if !values.is_empty() {
                    return Err(mir_error_at(
                        "MIR closure registration expects no value operands",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::UiPreview { .. } => {
                if values.len() != 2 {
                    return Err(mir_error_at(
                        "MIR UiPreview CoreClosureCall expects name and viewport values",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::UiAction => {
                if values.len() != 3 {
                    return Err(mir_error_at(
                        "MIR UiAction CoreClosureCall expects display, shortcut, and accessible label values",
                        span,
                    ));
                }
            }
            MirCoreClosureKind::UiTextInputOnDrop => {
                if values.len() != 2 {
                    return Err(mir_error_at(
                        "MIR UiTextInputOnDrop CoreClosureCall expects state and IME mode values",
                        span,
                    ));
                }
            }
        }
        if !matches!(&kind, MirCoreClosureKind::OnInterrupt) && closure.is_none() {
            return Err(mir_error_at(
                "MIR CoreClosureCall is missing its checked closure operand",
                span,
            ));
        }
        let row = self.prelude_row(call, span)?;
        if row.abi != jet_foundation::MIR::MirPreludeAbi::Value
            || !matches!(
                row.fallibility,
                jet_foundation::MIR::MirCallFallibility::Infallible
            )
        {
            return Err(mir_error_at(
                "MIR CoreClosureCall route has unsupported ABI or fallibility",
                span,
            ));
        }
        if matches!(&kind, MirCoreClosureKind::Spawn) {
            if row.family != jet_foundation::MIR::MirPreludeFamily::StaticPrelude {
                return Err(mir_error_at(
                    "MIR task spawn route has unsupported family",
                    span,
                ));
            }
            let module = row.module.clone();
            let member = row.member.clone();
            let symbol = row.symbol.name().to_string();
            return self.eval_task_spawn(
                frame_index,
                values,
                closure,
                &module,
                &member,
                &symbol,
                label,
                span,
            );
        }
        if matches!(
            &kind,
            MirCoreClosureKind::ReactiveDerived
                | MirCoreClosureKind::ReactiveEffect
                | MirCoreClosureKind::UiMount
        ) {
            let value = self.value(frame_index, closure.expect("checked closure operand"), span)?;
            let callback = self.standalone_closure_value(value, span)?;
            let (module, method) = if matches!(&kind, MirCoreClosureKind::ReactiveDerived) {
                ("core.reactive", "derived")
            } else if matches!(&kind, MirCoreClosureKind::UiMount) {
                ("core.ui", "reactive_render")
            } else {
                ("core.reactive", "effect")
            };
            let result_type = _result_ty.map(crate::Comptime::MirBridge::mir_to_ast_type);
            let value = crate::Comptime::AppLite::apply_suite(
                module,
                method,
                &[callback],
                span,
                result_type.as_ref(),
            )?;
            return runtime_from_ct(value, span);
        }
        let mut runtime_values = Vec::with_capacity(values.len());
        for value in values {
            let value = self.value(frame_index, *value, span)?;
            if matches!(&kind, MirCoreClosureKind::OnInterrupt) {
                if !matches!(
                    &value,
                    RuntimeValue::Closure(_) | RuntimeValue::Data(MirEvalValue::Closure(_))
                ) {
                    return Err(mir_error_at(
                        "MIR OnInterrupt callback value is not callable",
                        span,
                    ));
                }
                let _ = self.closure_handle(value.clone(), span)?;
            } else {
                let _ = runtime_to_data(value.clone(), span)?;
            }
            runtime_values.push(value);
        }
        if matches!(&kind, MirCoreClosureKind::Realtime) {
            let rate = int_value(
                runtime_to_data(self.value(frame_index, values[0], span)?, span)?,
                span,
            )?;
            let frames = int_value(
                runtime_to_data(self.value(frame_index, values[1], span)?, span)?,
                span,
            )?;
            if rate <= 0 || frames <= 0 {
                return Err(mir_error_at(
                    "MIR Realtime callback rate and frames must be positive",
                    span,
                ));
            }
            let frame_count = usize::try_from(frames)
                .map_err(|_| mir_error_at("MIR Realtime frame count is too large", span))?;
            let callback = self.value(
                frame_index,
                closure.expect("validated realtime closure"),
                span,
            )?;
            if !matches!(
                &callback,
                RuntimeValue::Closure(_) | RuntimeValue::Data(MirEvalValue::Closure(_))
            ) {
                return Err(mir_error_at(
                    "MIR Realtime callback value is not callable",
                    span,
                ));
            }
            let callback = RuntimeValue::Closure(self.closure_handle(callback, span)?);
            let sample_ring = Arc::new(crate::scheduler::JetRealtimeSampleRing::new(frame_count));

            let start_identity = jet_foundation::Monotonic::jet_time_monotonic_now_ns();
            let next_callback = 1_u128;
            let next_deadline_identity = start_identity.saturating_add(
                crate::scheduler::jet_rt_deadline_offset_ns(rate, frames, next_callback),
            );
            let task = Rc::new(RefCell::new(MirRealtimeTask {
                scheduler_id: 0,
                requested_rate_hz: rate,
                requested_frames: frames,
                start_identity,
                next_callback,
                next_deadline_identity,
                completed_callbacks: 0,
                completed_frames: 0,
                missed: 0,
                max_lateness_ns: 0,
                end_identity: 0,
                cancelled: false,
                callback,
                sample_ring,
            }));
            let scheduler_id =
                crate::scheduler::jet_scheduler_owner_deadline_register(next_deadline_identity);
            task.borrow_mut().scheduler_id = scheduler_id;
            return Ok(RuntimeValue::Stream(Rc::new(RefCell::new(
                MirInterpreterStream::Realtime { state: task },
            ))));
        }
        if matches!(
            &kind,
            MirCoreClosureKind::UiAction
                | MirCoreClosureKind::UiTextInputOnDrop
                | MirCoreClosureKind::UiPreview { .. }
        ) {
            let callback_id = closure.expect("validated UI closure");
            let callback = self.value(frame_index, callback_id, span)?;
            if !matches!(
                &callback,
                RuntimeValue::Closure(_) | RuntimeValue::Data(MirEvalValue::Closure(_))
            ) {
                return Err(mir_error_at("MIR UI callback value is not callable", span));
            }
            let callback = RuntimeValue::Closure(self.closure_handle(callback, span)?);
            let args = runtime_values
                .into_iter()
                .map(|value| runtime_to_data(value, span))
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            let callback = runtime_to_data(callback, span)?;
            if let Some(result) = crate::Comptime::try_ambient_core_closure(
                &row.module,
                &row.member,
                call,
                kind.clone(),
                args,
                Some(callback),
                site,
                label,
                span,
            ) {
                return result.map(RuntimeValue::Data);
            }
        }
        if matches!(&kind, MirCoreClosureKind::Spawn) {
            let _site = i64::try_from(site.0)
                .map_err(|_| mir_error_at("MIR CoreClosureCall site does not fit Int", span))?;
            let _ = label;
        }
        if let Some(closure) = closure {
            let closure = self.value(frame_index, closure, span)?;
            if !matches!(
                &closure,
                RuntimeValue::Closure(_) | RuntimeValue::Data(MirEvalValue::Closure(_))
            ) {
                return Err(mir_error_at(
                    "MIR CoreClosureCall closure operand is not callable",
                    span,
                ));
            }
            let _ = self.closure_handle(closure, span)?;
        }
        Err(mir_error_at(
            "MIR CoreClosureCall requires a typed callback host bridge; \
             the interpreter cannot marshal its opaque closure",
            span,
        ))
    }

    fn eval_cell_guard_project(
        &mut self,
        frame_index: usize,
        map_call: MirPreludeCallId,
        split_call: Option<MirPreludeCallId>,
        guard: MirValueId,
        paths: &[Vec<MirFieldId>],
        editable: bool,
        edit_paths_disjoint: bool,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if !(1..=2).contains(&paths.len()) || paths.iter().any(|path| path.is_empty()) {
            return Err(mir_error_at(
                "MIR Cell guard projection has an invalid checked path shape",
                span,
            ));
        }
        let is_split = paths.len() == 2;
        if split_call.is_some() != is_split {
            return Err(mir_error_at(
                "MIR Cell guard projection path arity disagrees with its split row",
                span,
            ));
        }
        if edit_paths_disjoint && (!editable || !is_split) {
            return Err(mir_error_at(
                "MIR Cell guard edit disjointness proof has an invalid shape",
                span,
            ));
        }
        if editable && is_split && !edit_paths_disjoint {
            return Err(mir_error_at(
                "MIR editable Cell guard split is missing disjoint paths proof",
                span,
            ));
        }
        self.ensure_cell_guard_route(map_call, span)?;
        let RuntimeValue::CellGuard(target) = self.value(frame_index, guard, span)? else {
            return Err(mir_error_at(
                "MIR Cell guard projection requires an opaque RuntimeValue CellGuard",
                span,
            ));
        };
        match paths {
            [path] => self
                .map_cell_guard_state(target.as_ref(), path, editable, span)
                .map(|guard| RuntimeValue::CellGuard(Rc::new(guard))),
            [first, second] => {
                let common = first
                    .iter()
                    .zip(second)
                    .take_while(|(left, right)| left == right)
                    .count();
                if common == first.len() || common == second.len() {
                    return Err(mir_error_at(
                        "MIR Cell guard split paths are identical or prefix-related",
                        span,
                    ));
                }
                let split_call = split_call.ok_or_else(|| {
                    mir_error_at("MIR Cell guard split has no exact split row", span)
                })?;
                self.ensure_cell_guard_route(split_call, span)?;
                let prefix =
                    self.map_cell_guard_state(target.as_ref(), &first[..common], editable, span)?;
                let first_guard =
                    self.map_cell_guard_state(&prefix, &first[common..], editable, span)?;
                let second_guard =
                    self.map_cell_guard_state(&prefix, &second[common..], editable, span)?;
                let field_ids = aggregate_field_ids(self.program, result_ty, 2, span)?;
                Ok(RuntimeValue::Aggregate(vec![
                    (field_ids[0], RuntimeValue::CellGuard(Rc::new(first_guard))),
                    (field_ids[1], RuntimeValue::CellGuard(Rc::new(second_guard))),
                ]))
            }
            _ => unreachable!("Cell guard path shape was checked above"),
        }
    }

    fn ensure_cell_guard_route(
        &self,
        call: MirPreludeCallId,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let row = self.prelude_row(call, span)?;
        if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
            return Err(mir_error_at(
                "MIR Cell guard route has unsupported Prelude ABI",
                span,
            ));
        }
        Ok(())
    }

    fn map_cell_guard_state(
        &self,
        target: &MirCellGuard,
        path: &[MirFieldId],
        editable: bool,
        span: Span,
    ) -> Result<MirCellGuard, Diagnostic> {
        if editable && !target.lease.editable {
            return Err(mir_error_at(
                "MIR editable Cell guard projection lacks an editable lease",
                span,
            ));
        }
        let mut mapped = target.path.clone();
        mapped.extend_from_slice(path);
        let _ = cell_payload_read(self.program, &target.payload, &mapped, span)?;
        Ok(MirCellGuard {
            payload: target.payload.clone(),
            path: mapped,
            lease: target.lease.clone(),
        })
    }
    fn eval_shared_guard_map(
        &mut self,
        frame_index: usize,
        call: MirPreludeCallId,
        guard: MirValueId,
        path: &[MirFieldId],
        editable: bool,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        self.ensure_shared_guard_route(call, span)?;
        if path.is_empty() {
            return Err(mir_error_at(
                "MIR SharedGuard map requires a non-empty static field path",
                span,
            ));
        }
        let RuntimeValue::SharedGuard(target) = self.value(frame_index, guard, span)? else {
            return Err(mir_error_at(
                "MIR SharedGuard map requires an opaque RuntimeValue guard",
                span,
            ));
        };
        let state =
            target.state.as_ref().cloned().ok_or_else(|| {
                mir_error_at("MIR SharedGuard has no active protocol state", span)
            })?;
        let state = map_shared_guard_state(state, path, editable, span)?;
        let mapped = Rc::new(MirSharedGuard {
            state: Some(state),
            payload: target.payload.clone(),
            storage: target.storage.clone(),
        });
        let _ = result_ty;
        let names = self.shared_guard_path_names(&mapped, span)?;
        let _ = shared_payload_read(&mapped.payload, &names, span)?;
        Ok(RuntimeValue::SharedGuard(mapped))
    }

    fn eval_shared_guard_split(
        &mut self,
        frame_index: usize,
        call: MirPreludeCallId,
        map_call: MirPreludeCallId,
        guard: MirValueId,
        first: &[MirFieldId],
        second: &[MirFieldId],
        editable: bool,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        self.ensure_shared_guard_route(call, span)?;
        self.ensure_shared_guard_route(map_call, span)?;
        if first.is_empty() || second.is_empty() {
            return Err(mir_error_at(
                "MIR SharedGuard split requires two non-empty static field paths",
                span,
            ));
        }
        let mut common = 0usize;
        while common < first.len() && common < second.len() && first[common] == second[common] {
            common += 1;
        }
        if common == first.len() || common == second.len() {
            return Err(mir_error_at(
                "MIR SharedGuard split paths must diverge after a common prefix",
                span,
            ));
        }
        let RuntimeValue::SharedGuard(target) = self.value(frame_index, guard, span)? else {
            return Err(mir_error_at(
                "MIR SharedGuard split requires an opaque RuntimeValue guard",
                span,
            ));
        };
        let state =
            target.state.as_ref().cloned().ok_or_else(|| {
                mir_error_at("MIR SharedGuard has no active protocol state", span)
            })?;
        let prefix_state = map_shared_guard_state(state, &first[..common], editable, span)?;
        let first_id = i64::try_from(first[common].0).map_err(|_| {
            mir_error_at(
                "MIR SharedGuard field ID does not fit protocol metadata",
                span,
            )
        })?;
        let second_id = i64::try_from(second[common].0).map_err(|_| {
            mir_error_at(
                "MIR SharedGuard field ID does not fit protocol metadata",
                span,
            )
        })?;
        let (first_state, second_state) =
            shared_protocol::jet_shared_guard_split(&prefix_state, first_id, second_id, editable)
                .map_err(|message| mir_error_at(message, span))?;
        let first_state =
            map_shared_guard_state(first_state, &first[common + 1..], editable, span)?;
        let second_state =
            map_shared_guard_state(second_state, &second[common + 1..], editable, span)?;
        let first_guard = Rc::new(MirSharedGuard {
            state: Some(first_state),
            payload: target.payload.clone(),
            storage: target.storage.clone(),
        });
        let second_guard = Rc::new(MirSharedGuard {
            state: Some(second_state),
            payload: target.payload.clone(),
            storage: target.storage.clone(),
        });
        let first_names = self.shared_guard_path_names(&first_guard, span)?;
        let second_names = self.shared_guard_path_names(&second_guard, span)?;
        let _ = shared_payload_read(&first_guard.payload, &first_names, span)?;
        let _ = shared_payload_read(&second_guard.payload, &second_names, span)?;
        let field_ids = aggregate_field_ids(self.program, result_ty, 2, span)?;
        Ok(RuntimeValue::Aggregate(vec![
            (field_ids[0], RuntimeValue::SharedGuard(first_guard)),
            (field_ids[1], RuntimeValue::SharedGuard(second_guard)),
        ]))
    }

    fn ensure_shared_guard_route(
        &self,
        call: MirPreludeCallId,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let row = self.prelude_row(call, span)?;
        if row.abi != jet_foundation::MIR::MirPreludeAbi::Control {
            return Err(mir_error_at(
                "MIR SharedGuard route has unsupported Prelude ABI",
                span,
            ));
        }
        Ok(())
    }

    fn shared_guard_path_names(
        &self,
        guard: &MirSharedGuard,
        span: Span,
    ) -> Result<Vec<String>, Diagnostic> {
        let state = guard
            .state
            .as_ref()
            .ok_or_else(|| mir_error_at("MIR SharedGuard has no active protocol state", span))?;
        state
            .path()
            .iter()
            .map(|field| {
                let field = u64::try_from(*field).map_err(|_| {
                    mir_error_at("MIR SharedGuard protocol field ID is negative", span)
                })?;
                field_name(self.program, MirFieldId(field)).ok_or_else(|| {
                    mir_error_at(
                        "MIR SharedGuard protocol field ID has no canonical field row",
                        span,
                    )
                })
            })
            .collect()
    }
    fn deref_shared_guard(
        &self,
        guard: Rc<MirSharedGuard>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let state = guard
            .state
            .as_ref()
            .ok_or_else(|| mir_error_at("MIR SharedGuard has no active protocol state", span))?;
        if !state.held() {
            return Err(mir_error_at(
                shared_protocol::JET_SHARED_GUARD_INVALID,
                span,
            ));
        }
        if let Some(storage) = &guard.storage {
            storage.refresh_scalar_shadow(span)?;
        }
        let path = self.shared_guard_path_names(&guard, span)?;
        let _ = shared_payload_read(&guard.payload, &path, span)?;
        Ok(RuntimeValue::SharedCell(Rc::new(MirSharedCell {
            guard,
            path,
        })))
    }

    fn shared_root_storage(
        &self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<Rc<MirSharedStorage>, Diagnostic> {
        let RuntimeValue::SharedCell(cell) = value else {
            return Err(mir_error_at(
                "MIR Shared host route receiver is not a Shared value",
                span,
            ));
        };
        if !cell.path.is_empty() || cell.guard.state.is_some() {
            return Err(mir_error_at(
                "MIR Shared host route receiver is not a root Shared value",
                span,
            ));
        }
        cell.guard
            .storage
            .clone()
            .ok_or_else(|| mir_error_at("MIR Shared value has no canonical storage", span))
    }

    fn eval_shared_new(&self, value: RuntimeValue, span: Span) -> Result<RuntimeValue, Diagnostic> {
        let value = runtime_to_data(value, span)?;
        let storage = MirSharedStorage::new(value);
        Ok(RuntimeValue::SharedCell(Rc::new(MirSharedCell {
            guard: Rc::new(MirSharedGuard {
                state: None,
                payload: storage.payload.clone(),
                storage: Some(storage),
            }),
            path: Vec::new(),
        })))
    }

    fn active_shared_transaction(&self, frame_index: usize) -> Option<Rc<MirSharedTransaction>> {
        let frame = self.frames.get(frame_index)?;
        frame
            .scopes
            .iter()
            .rev()
            .find_map(|scope| frame.shared_transactions.get(scope).cloned())
    }

    fn shared_transaction_handle(
        value: RuntimeValue,
        span: Span,
    ) -> Result<Rc<MirSharedTransaction>, Diagnostic> {
        match value {
            RuntimeValue::SharedTransaction(transaction) => Ok(transaction),
            _ => Err(mir_error_at(
                "MIR Shared transaction route requires an opaque STM handle",
                span,
            )),
        }
    }

    fn shared_transaction_value(
        &self,
        storage: &Rc<MirSharedStorage>,
        transaction: &Rc<MirSharedTransaction>,
        span: Span,
    ) -> Result<Option<Rc<RefCell<MirEvalValue>>>, Diagnostic> {
        let protocol = storage.protocol.clone();
        transaction.with_mut(span, |stm| {
            stm.touch(protocol.clone());
            Ok(stm.staged_value::<MirEvalValue>(&protocol))
        })
    }

    fn shared_transaction_stage_for_write(
        &self,
        storage: &Rc<MirSharedStorage>,
        transaction: &Rc<MirSharedTransaction>,
        span: Span,
    ) -> Result<Rc<RefCell<MirEvalValue>>, Diagnostic> {
        let initial = storage.capture(span)?.1;
        let protocol = storage.protocol.clone();
        transaction.with_mut(span, |stm| {
            let staged = stm.stage_value(protocol.clone(), move || initial);
            stm.mark_write(protocol);
            Ok(staged)
        })
    }

    fn register_shared_transaction_commit(
        &self,
        storage: Rc<MirSharedStorage>,
        transaction: Rc<MirSharedTransaction>,
        staged: Rc<RefCell<MirEvalValue>>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let protocol = storage.protocol.clone();
        let error_sink = transaction.clone();
        transaction.with_mut(span, move |stm| {
            let commit_storage = storage;
            let commit_staged = staged;
            let commit_error_sink = error_sink;
            stm.record_edit_with_commit(
                protocol,
                Box::new(|| {}),
                Box::new(move || {
                    let next = match commit_storage.next_revision(span) {
                        Ok(next) => next,
                        Err(error) => {
                            commit_error_sink.record_error(error);
                            return;
                        }
                    };
                    let value = match commit_staged.try_borrow() {
                        Ok(value) => value.clone(),
                        Err(_) => {
                            commit_error_sink.record_error(mir_error_at(
                                "MIR Shared transaction payload is already borrowed",
                                span,
                            ));
                            return;
                        }
                    };
                    if let Err(error) = commit_storage.publish_at_revision(value, next, span) {
                        commit_error_sink.record_error(error);
                    }
                }),
            );
            Ok(())
        })
    }

    fn shared_transaction_read(
        &self,
        storage: &Rc<MirSharedStorage>,
        transaction: &Rc<MirSharedTransaction>,
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        let staged = self.shared_transaction_value(storage, transaction, span)?;
        match staged {
            Some(staged) => staged.try_borrow().map(|value| value.clone()).map_err(|_| {
                mir_error_at("MIR Shared transaction payload is already borrowed", span)
            }),
            None => storage.read(span),
        }
    }

    fn eval_shared_transaction_callback(
        &mut self,
        storage: Rc<MirSharedStorage>,
        transaction: Rc<MirSharedTransaction>,
        callback: RuntimeValue,
        editable: bool,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let staged = if editable {
            Some(self.shared_transaction_stage_for_write(&storage, &transaction, span)?)
        } else {
            self.shared_transaction_value(&storage, &transaction, span)?
        };
        let callback_storage = staged
            .map(MirSharedStorage::from_payload)
            .unwrap_or_else(|| storage.clone());
        let result = self.eval_shared_callback(callback_storage, callback, editable, span)?;
        if editable {
            let staged = self
                .shared_transaction_value(&storage, &transaction, span)?
                .ok_or_else(|| {
                    mir_error_at("MIR Shared transaction staged value disappeared", span)
                })?;
            self.register_shared_transaction_commit(storage, transaction, staged, span)?;
        }
        Ok(result)
    }

    fn eval_shared_transaction_capture(
        &mut self,
        storage: Rc<MirSharedStorage>,
        transaction: Rc<MirSharedTransaction>,
        callback: Option<RuntimeValue>,
        span: Span,
    ) -> Result<Rc<MirSharedSnapshot>, Diagnostic> {
        let protocol = storage.protocol.clone();
        let staged = self.shared_transaction_value(&storage, &transaction, span)?;
        let (revision, value) = if let Some(staged) = staged {
            let revision = transaction.with_mut(span, |stm| {
                stm.snapshot_revision(&protocol, storage.revision.load(Ordering::Acquire))
                    .ok_or_else(|| mir_error_at("SharedRevisionError.GenerationExhausted", span))
            })?;
            let value = match callback {
                Some(callback) => {
                    let callback_storage = MirSharedStorage::from_payload(staged);
                    let snapshot = self.eval_shared_projection(callback_storage, callback, span)?;
                    snapshot.value.clone()
                }
                None => staged
                    .try_borrow()
                    .map(|value| value.clone())
                    .map_err(|_| {
                        mir_error_at("MIR Shared transaction payload is already borrowed", span)
                    })?,
            };
            (revision, value)
        } else {
            match callback {
                Some(callback) => {
                    let snapshot = self.eval_shared_projection(storage.clone(), callback, span)?;
                    (snapshot.revision, snapshot.value.clone())
                }
                None => storage.capture(span)?,
            }
        };
        let valid = Arc::new(AtomicBool::new(true));
        let consumed = Arc::new(AtomicBool::new(false));
        transaction.with_mut(span, |stm| {
            stm.record_snapshot(protocol, valid.clone());
            Ok(())
        })?;
        Ok(Rc::new(MirSharedSnapshot {
            owner: storage,
            revision,
            valid,
            consumed,
            value,
        }))
    }

    fn eval_shared_callback(
        &mut self,
        storage: Rc<MirSharedStorage>,
        callback: RuntimeValue,
        editable: bool,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        storage.refresh_scalar_shadow(span)?;
        if !editable && storage.scalar.is_some() {
            let guard = Rc::new(MirSharedGuard {
                state: None,
                payload: storage.payload.clone(),
                storage: Some(storage.clone()),
            });
            let cell = RuntimeValue::SharedCell(Rc::new(MirSharedCell {
                guard,
                path: Vec::new(),
            }));
            return self.invoke_callback(callback, cell, span);
        }
        let state = storage.acquire_guard(editable, span)?;
        let guard = Rc::new(MirSharedGuard {
            state: Some(state),
            payload: storage.payload.clone(),
            storage: Some(storage.clone()),
        });
        let cell = RuntimeValue::SharedCell(Rc::new(MirSharedCell {
            guard,
            path: Vec::new(),
        }));
        let result = self.invoke_callback(callback, cell, span)?;
        if editable {
            storage.commit_scalar_shadow(span)?;
        }
        Ok(result)
    }

    fn eval_shared_projection(
        &mut self,
        storage: Rc<MirSharedStorage>,
        callback: RuntimeValue,
        span: Span,
    ) -> Result<Rc<MirSharedSnapshot>, Diagnostic> {
        storage.refresh_scalar_shadow(span)?;
        let permit = shared_protocol::jet_shared_acquire(&storage.protocol, false, || false)
            .ok_or_else(|| mir_error_at("MIR Shared capture protocol acquisition failed", span))?;
        let revision = storage.revision.load(Ordering::Acquire);
        let state = if storage.scalar.is_some() {
            None
        } else {
            Some(storage.acquire_guard(false, span)?)
        };
        let guard = Rc::new(MirSharedGuard {
            state,
            payload: storage.payload.clone(),
            storage: Some(storage.clone()),
        });
        let cell = RuntimeValue::SharedCell(Rc::new(MirSharedCell {
            guard,
            path: Vec::new(),
        }));
        let projected = self.invoke_callback(callback, cell, span)?;
        drop(permit);
        Ok(Rc::new(MirSharedSnapshot {
            owner: storage,
            revision,
            valid: Arc::new(AtomicBool::new(true)),
            consumed: Arc::new(AtomicBool::new(false)),
            value: runtime_to_data(projected, span)?,
        }))
    }

    fn shared_revision_result(&self, variant: &str, payload: Option<MirEvalValue>) -> RuntimeValue {
        RuntimeValue::Data(MirEvalValue::Enum {
            type_name: "Result".to_string(),
            variant: variant.to_string(),
            args: payload.map_or_else(Vec::new, |value| vec![(None, value)]),
        })
    }

    fn eval_pool_host(
        &mut self,
        member: &str,
        args: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let mut args = args.into_iter();
        let receiver = args
            .next()
            .ok_or_else(|| mir_error_at("MIR Pool host route has no receiver", span))?;
        let receiver = self.runtime_to_ct(receiver, span)?;
        let pool = mir_runtime_owner::<MirPool>(&receiver)
            .ok_or_else(|| mir_error_at("MIR Pool receiver has no native owner", span))?;
        match member {
            "Pool.add" => {
                let value = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR Pool.add is missing its value", span))?;
                if args.next().is_some() {
                    return Err(mir_error_at("MIR Pool.add received extra arguments", span));
                }
                let value = self.runtime_to_ct(value, span)?;
                let mut pool = pool.lock().unwrap_or_else(|error| error.into_inner());
                Ok(RuntimeValue::Data(MirEvalValue::Int(
                    pool.add(value).to_word(),
                )))
            }
            "Pool.remove" => {
                let id = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR Pool.remove is missing its id", span))?;
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Pool.remove received extra arguments",
                        span,
                    ));
                }
                let id = match runtime_to_data(id, span)? {
                    MirEvalValue::Int(id) => MirPoolId::from_word(id),
                    _ => None,
                };
                let Some(id) = id else {
                    return self.pool_absent(result_ty, span);
                };
                let mut pool = pool.lock().unwrap_or_else(|error| error.into_inner());
                match pool.remove(id) {
                    Ok(value) => Ok(RuntimeValue::Result {
                        ok: true,
                        value: Box::new(runtime_from_ct(value, span)?),
                    }),
                    Err(_) => self.pool_absent(result_ty, span),
                }
            }
            "Pool.ids" => {
                if args.next().is_some() {
                    return Err(mir_error_at("MIR Pool.ids received extra arguments", span));
                }
                let pool = pool.lock().unwrap_or_else(|error| error.into_inner());
                let ids = pool
                    .ids()
                    .into_iter()
                    .map(|id| MirEvalValue::Int(id.to_word()))
                    .collect();
                Ok(RuntimeValue::Data(MirEvalValue::List(ids)))
            }
            _ => Err(mir_error_at(
                "MIR Pool host route has no interpreter implementation",
                span,
            )),
        }
    }

    fn pool_absent(
        &self,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let element = result_ty
            .and_then(MirType::option_inner)
            .cloned()
            .ok_or_else(|| mir_error_at("MIR Pool.remove has no Option result type", span))?;
        Ok(RuntimeValue::Absent { element })
    }
    fn eval_pool_index(
        &mut self,
        base: RuntimeValue,
        index: RuntimeValue,
        location: MirPanicLoc,
        context: Option<&MirPanicContext>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let base = self.runtime_to_ct(base, span)?;
        let pool = mir_runtime_owner::<MirPool>(&base)
            .ok_or_else(|| mir_error_at("MIR Pool index base has no native owner", span))?;
        let index = match runtime_to_data(index, span)? {
            MirEvalValue::Int(index) => MirPoolId::from_word(index),
            _ => None,
        };
        let Some(index) = index else {
            return Err(mir_error_at("MIR Pool index is not a valid Id", span));
        };
        let value = {
            let pool = pool.lock().unwrap_or_else(|error| error.into_inner());
            pool.checked_get(index).cloned()
        };
        match value {
            Some(value) => runtime_from_ct(value, span),
            None => Err(self.pool_index_stop(location, context, span)),
        }
    }

    fn pool_index_stop(
        &mut self,
        location: MirPanicLoc,
        context: Option<&MirPanicContext>,
        span: Span,
    ) -> Diagnostic {
        self.last_runtime_stop = Some("E3001".to_string());
        let file = self
            .program
            .source_files
            .iter()
            .find(|source| source.id == location.file)
            .map(|source| source.path.as_str())
            .unwrap_or_default();
        let (function, source_line, column, caret) =
            context.map_or(("", "", location.column, 1), |context| {
                (
                    context.function.as_str(),
                    context.source_line.as_str(),
                    location.column,
                    context.caret,
                )
            });
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            "E3001",
            file,
            location.line,
            function,
            source_line,
            column,
            caret,
            jet_foundation::Outcome::jet_pool_stale_message(),
            "",
        );
        self.stderr.push_str(&report.rendered);
        self.exit_code = report.exit_code;
        crate::Sema::Diagnostics::soft_exit(
            report.exit_code.to_string(),
            "runtime stop".to_string(),
            Some(span),
        )
    }

    fn eval_shared_host(
        &mut self,
        frame_index: usize,
        member: &str,
        args: Vec<RuntimeValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let mut args = args.into_iter();
        let receiver = args
            .next()
            .ok_or_else(|| mir_error_at("MIR Shared host route has no receiver", span))?;
        if member == "SharedSnapshot.value" {
            if args.next().is_some() {
                return Err(mir_error_at(
                    "MIR SharedSnapshot.value received extra arguments",
                    span,
                ));
            }
            let RuntimeValue::SharedSnapshot(snapshot) = receiver else {
                return Err(mir_error_at(
                    "MIR SharedSnapshot.value requires an opaque snapshot",
                    span,
                ));
            };
            return Ok(RuntimeValue::Data(snapshot.value.clone()));
        }
        let storage = self.shared_root_storage(receiver, span)?;
        let active_transaction = self.active_shared_transaction(frame_index);
        let revision_error = |variant: &str| {
            self.shared_revision_result(
                "Err",
                Some(MirEvalValue::Enum {
                    type_name: crate::Syntax::TYPE_SHARED_REVISION_ERROR.to_string(),
                    variant: variant.to_string(),
                    args: Vec::new(),
                }),
            )
        };
        match member {
            "Shared.get" => {
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared.get received extra arguments",
                        span,
                    ));
                }
                if let Some(transaction) = active_transaction.clone() {
                    self.shared_transaction_read(&storage, &transaction, span)
                        .map(RuntimeValue::Data)
                } else {
                    storage.read(span).map(RuntimeValue::Data)
                }
            }
            "Shared.set" => {
                let value = runtime_to_data(
                    args.next()
                        .ok_or_else(|| mir_error_at("MIR Shared.set is missing its value", span))?,
                    span,
                )?;
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared.set received extra arguments",
                        span,
                    ));
                }
                if let Some(transaction) = active_transaction.clone() {
                    let staged =
                        self.shared_transaction_stage_for_write(&storage, &transaction, span)?;
                    *staged.try_borrow_mut().map_err(|_| {
                        mir_error_at("MIR Shared transaction payload is already borrowed", span)
                    })? = value;
                    self.register_shared_transaction_commit(storage, transaction, staged, span)?;
                } else {
                    storage.set(value, span)?;
                }
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            "Shared.replace" => {
                let value = runtime_to_data(
                    args.next().ok_or_else(|| {
                        mir_error_at("MIR Shared.replace is missing its value", span)
                    })?,
                    span,
                )?;
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared.replace received extra arguments",
                        span,
                    ));
                }
                if let Some(transaction) = active_transaction.clone() {
                    let staged =
                        self.shared_transaction_stage_for_write(&storage, &transaction, span)?;
                    let previous =
                        staged
                            .try_borrow()
                            .map(|value| value.clone())
                            .map_err(|_| {
                                mir_error_at(
                                    "MIR Shared transaction payload is already borrowed",
                                    span,
                                )
                            })?;
                    *staged.try_borrow_mut().map_err(|_| {
                        mir_error_at("MIR Shared transaction payload is already borrowed", span)
                    })? = value;
                    self.register_shared_transaction_commit(storage, transaction, staged, span)?;
                    Ok(RuntimeValue::Data(previous))
                } else {
                    storage.replace(value, span).map(RuntimeValue::Data)
                }
            }
            "Shared.capture" => {
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared.capture received extra arguments",
                        span,
                    ));
                }
                if let Some(transaction) = active_transaction.clone() {
                    self.eval_shared_transaction_capture(storage, transaction, None, span)
                        .map(RuntimeValue::SharedSnapshot)
                } else {
                    storage.snapshot(span).map(RuntimeValue::SharedSnapshot)
                }
            }
            "Shared.capture_with" => {
                let callback = args.next().ok_or_else(|| {
                    mir_error_at("MIR Shared.capture_with is missing its projection", span)
                })?;
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared.capture_with received extra arguments",
                        span,
                    ));
                }
                if let Some(transaction) = active_transaction.clone() {
                    self.eval_shared_transaction_capture(storage, transaction, Some(callback), span)
                        .map(RuntimeValue::SharedSnapshot)
                } else {
                    self.eval_shared_projection(storage, callback, span)
                        .map(RuntimeValue::SharedSnapshot)
                }
            }
            "Shared.capture_txn" => {
                let transaction = Self::shared_transaction_handle(
                    args.next().ok_or_else(|| {
                        mir_error_at("MIR Shared.capture_txn is missing its STM handle", span)
                    })?,
                    span,
                )?;
                let callback = args.next();
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared.capture_txn received extra arguments",
                        span,
                    ));
                }
                self.eval_shared_transaction_capture(storage, transaction, callback, span)
                    .map(RuntimeValue::SharedSnapshot)
            }
            "Shared.read_txn" | "Shared.edit_txn" => {
                let transaction = Self::shared_transaction_handle(
                    args.next().ok_or_else(|| {
                        mir_error_at(
                            "MIR Shared transaction route is missing its STM handle",
                            span,
                        )
                    })?,
                    span,
                )?;
                let callback = args.next().ok_or_else(|| {
                    mir_error_at("MIR Shared transaction callback is missing", span)
                })?;
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared transaction callback received extra arguments",
                        span,
                    ));
                }
                self.eval_shared_transaction_callback(
                    storage,
                    transaction,
                    callback,
                    member == "Shared.edit_txn",
                    span,
                )
            }
            "Shared.try_replace" => {
                let snapshot = args.next().ok_or_else(|| {
                    mir_error_at("MIR Shared.try_replace is missing its snapshot", span)
                })?;
                let value = args.next().ok_or_else(|| {
                    mir_error_at("MIR Shared.try_replace is missing its value", span)
                })?;
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared.try_replace received extra arguments",
                        span,
                    ));
                }
                let RuntimeValue::SharedSnapshot(snapshot) = snapshot else {
                    return Err(mir_error_at(
                        "MIR Shared.try_replace requires an opaque snapshot",
                        span,
                    ));
                };
                if !Rc::ptr_eq(&storage, &snapshot.owner) {
                    return Ok(revision_error("WrongOwner"));
                }
                if !snapshot.valid.load(Ordering::Acquire)
                    || snapshot.consumed.load(Ordering::Acquire)
                    || storage.revision.load(Ordering::Acquire) != snapshot.revision
                {
                    return Ok(self.shared_revision_result("Ok", Some(MirEvalValue::Bool(false))));
                }
                let permit = shared_protocol::jet_shared_acquire(&storage.protocol, true, || false)
                    .ok_or_else(|| {
                        mir_error_at("MIR Shared edit protocol acquisition failed", span)
                    })?;
                if storage.revision.load(Ordering::Acquire) != snapshot.revision {
                    drop(permit);
                    return Ok(self.shared_revision_result("Ok", Some(MirEvalValue::Bool(false))));
                }
                let next = match storage.next_revision(span) {
                    Ok(next) => next,
                    Err(_) => {
                        drop(permit);
                        return Ok(revision_error("GenerationExhausted"));
                    }
                };
                let value = runtime_to_data(value, span)?;
                if snapshot
                    .consumed
                    .swap(true, std::sync::atomic::Ordering::AcqRel)
                {
                    drop(permit);
                    return Ok(self.shared_revision_result("Ok", Some(MirEvalValue::Bool(false))));
                }
                if let Some(scalar) = &storage.scalar {
                    let bits = mir_shared_scalar_bits(&value, scalar.kind(), span)?;
                    scalar.store(bits);
                } else {
                    *storage.payload.try_borrow_mut().map_err(|_| {
                        mir_error_at("MIR Shared payload is already borrowed", span)
                    })? = value;
                }
                snapshot.valid.store(false, Ordering::Release);
                storage.revision.store(next, Ordering::Release);
                drop(permit);
                Ok(self.shared_revision_result("Ok", Some(MirEvalValue::Bool(true))))
            }
            "Shared.read" | "Shared.edit" => {
                let callback = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR Shared callback is missing", span))?;
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared callback received extra arguments",
                        span,
                    ));
                }
                if let Some(transaction) = active_transaction {
                    self.eval_shared_transaction_callback(
                        storage,
                        transaction,
                        callback,
                        member == "Shared.edit",
                        span,
                    )
                } else {
                    self.eval_shared_callback(storage, callback, member == "Shared.edit", span)
                }
            }
            "Shared.guard_read" | "Shared.guard_edit" => {
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR Shared guard received extra arguments",
                        span,
                    ));
                }
                let editable = member == "Shared.guard_edit";
                let state = storage.acquire_guard(editable, span)?;
                Ok(RuntimeValue::SharedGuard(Rc::new(MirSharedGuard {
                    state: Some(state),
                    payload: storage.payload.clone(),
                    storage: Some(storage),
                })))
            }
            _ => Err(mir_error_at(
                "MIR Shared host route has no interpreter implementation",
                span,
            )),
        }
    }

    fn eval_prelude_values(
        &mut self,
        frame_index: usize,
        call: MirPreludeCallId,
        args: &[MirValueId],
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let values = args
            .iter()
            .map(|value| self.value(frame_index, *value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        self.eval_prelude_runtime_values(call, values, result_ty, span)
    }
    fn eval_codec_prelude(
        &mut self,
        member: &str,
        values: Vec<MirEvalValue>,
        type_args: &[MirType],
        _result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let values = values
            .into_iter()
            .map(|value| crate::Comptime::MirBridge::mir_to_ct_value(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        if member == "decode_typed" {
            if values.len() != 1 {
                return Err(mir_error_at(
                    &format!(
                        "MIR typed codec route expects one argument, got {}",
                        values.len()
                    ),
                    span,
                ));
            }
            let target = type_args
                .first()
                .map(crate::Comptime::MirBridge::mir_to_ast_type)
                .ok_or_else(|| {
                    mir_error_at("MIR typed codec decode has no checked type argument", span)
                })?;
            let decoded = crate::Comptime::decode_typed_builtin_value_for_mir(&target, &values[0])
                .ok_or_else(|| {
                    mir_error_at(
                        &format!(
                            "MIR typed codec decode has no builtin decoder for {}",
                            target.name()
                        ),
                        span,
                    )
                })?;
            let result = match decoded {
                Ok(value) => CtValue::Present(Box::new(value)),
                Err(error) => CtValue::failed(Box::new(error)),
            };
            return Ok(RuntimeValue::Data(
                crate::Comptime::MirBridge::ct_to_mir_value(result, span)?,
            ));
        }
        if values.len() != 2 {
            return Err(mir_error_at(
                &format!(
                    "MIR codec route expects two arguments, got {}",
                    values.len()
                ),
                span,
            ));
        }
        let _kind = match values.first() {
            Some(CtValue::Int(kind)) => *kind,
            _ => return Err(mir_error_at("MIR codec kind is not an Int", span)),
        };
        let result = match member {
            "encode" => {
                let target = type_args
                    .first()
                    .map(crate::Comptime::MirBridge::mir_to_ast_type)
                    .ok_or_else(|| {
                        mir_error_at("MIR codec encode has no checked type argument", span)
                    })?;
                let mut encode_nominal = |nominal: &Type, value: &CtValue| {
                    self.invoke_typed_encode(nominal, value, span)
                };
                let encoded = crate::Comptime::encode_typed_value_for_mir(
                    &target,
                    &values[1],
                    &mut encode_nominal,
                )
                .ok_or_else(|| {
                    mir_error_at(
                        &format!(
                            "MIR typed codec encode has no builtin encoder for {}",
                            target.name()
                        ),
                        span,
                    )
                })?
                .map_err(|error| {
                    mir_error_at(&format!("MIR typed codec encode failed: {error}"), span)
                })?;
                encoded
            }
            "decode" => {
                let target = type_args
                    .first()
                    .map(crate::Comptime::MirBridge::mir_to_ast_type)
                    .ok_or_else(|| {
                        mir_error_at("MIR codec decode has no checked type argument", span)
                    })?;
                match crate::Comptime::decode_builtin_codec(&target, &values[1]) {
                    Ok(value) => CtValue::Present(Box::new(value)),
                    Err(error) => CtValue::failed(Box::new(error)),
                }
            }
            _ => {
                return Err(mir_error_at(
                    "MIR codec route has no interpreter implementation",
                    span,
                ))
            }
        };
        Ok(RuntimeValue::Data(
            crate::Comptime::MirBridge::ct_to_mir_value(result, span)?,
        ))
    }

    fn eval_prelude_runtime_values(
        &mut self,
        call: MirPreludeCallId,
        args: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        self.eval_prelude_runtime(call, args, result_ty, span)
    }
    fn native_print_value(
        &mut self,
        value: RuntimeValue,
        ty: &MirType,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if !self.native_print_shape(ty, &BTreeMap::new()) {
            return Ok(value);
        }
        let fallback = value.clone();
        let value = self.materialize_runtime(value, span)?;
        let value = match value {
            RuntimeValue::Data(value) => value,
            RuntimeValue::Ambient(owner)
                if matches!(
                    ty.kind(),
                    MirTypeKind::Tagged {
                        marker: MirTagMarker::Internal(MirInternalTag::AllocatorView),
                        ..
                    }
                ) =>
            {
                runtime_to_data(RuntimeValue::Ambient(owner), span)?
            }
            RuntimeValue::Aggregate(fields) => {
                let fields = fields
                    .into_iter()
                    .map(|(field, value)| {
                        let name = field_name(self.program, field).ok_or_else(|| {
                            mir_error_at("MIR aggregate field ID is missing", span)
                        })?;
                        let value = runtime_to_data(value, span)?;
                        Ok((name, value))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>();
                let Ok(fields) = fields else {
                    return Ok(fallback);
                };
                MirEvalValue::Struct {
                    type_name: "aggregate".to_string(),
                    fields,
                }
            }
            _ => return Ok(fallback),
        };
        let Some(text) = self.render_native_print_value(&value, ty, &BTreeMap::new(), span) else {
            return Ok(fallback);
        };
        Ok(RuntimeValue::Data(MirEvalValue::String(text)))
    }

    fn native_print_shape(&self, ty: &MirType, substitutions: &BTreeMap<String, MirType>) -> bool {
        let ty = substitute_print_type(ty, substitutions);
        if self.native_printable_type_def(&ty).is_some() {
            return true;
        }
        match ty.kind() {
            MirTypeKind::List(inner)
            | MirTypeKind::FixedList { elem: inner, .. }
            | MirTypeKind::Shared(inner)
            | MirTypeKind::Option(inner) => self.native_print_shape(inner, substitutions),
            MirTypeKind::Map { key, value } => {
                self.native_print_shape(key, substitutions)
                    || self.native_print_shape(value, substitutions)
            }
            MirTypeKind::Result { ok, err } => {
                self.native_print_shape(ok, substitutions)
                    || self.native_print_shape(err, substitutions)
            }
            MirTypeKind::Tuple(fields) => fields
                .iter()
                .any(|(_, field)| self.native_print_shape(field, substitutions)),
            MirTypeKind::Tagged {
                marker: MirTagMarker::Internal(MirInternalTag::AllocatorView),
                ..
            } => true,
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => self.native_print_shape(base, substitutions),
            MirTypeKind::Union(members) => members
                .iter()
                .any(|member| self.native_print_shape(member, substitutions)),
            MirTypeKind::Apply { args, .. } => args
                .iter()
                .any(|arg| self.native_print_shape(arg, substitutions)),
            _ => false,
        }
    }

    fn native_printable_type_def(&self, ty: &MirType) -> Option<&jet_foundation::MIR::MirTypeDef> {
        let is_printable = |def: &jet_foundation::MIR::MirTypeDef| {
            def.auto_printable
                && matches!(
                    &def.kind,
                    MirTypeDefKind::Struct { .. } | MirTypeDefKind::Enum { .. }
                )
        };
        if let Some(identity) = ty.identity {
            if let Some(def) = self.program.types.iter().find(|def| def.id == identity) {
                if is_printable(def) {
                    return Some(def);
                }
            }
        }
        let MirTypeKind::Apply { name, .. } = ty.kind() else {
            return None;
        };
        self.program
            .types
            .iter()
            .find(|def| def.id == name.id || def.key == name.name || def.name == name.name)
            .filter(|def| is_printable(def))
    }

    fn native_print_substitutions(
        &self,
        def: &jet_foundation::MIR::MirTypeDef,
        ty: &MirType,
    ) -> BTreeMap<String, MirType> {
        let MirTypeKind::Apply { args, .. } = ty.kind() else {
            return BTreeMap::new();
        };
        def.generic_params
            .iter()
            .zip(args)
            .map(|(param, arg)| (param.name.clone(), arg.clone()))
            .collect()
    }

    fn render_native_print_value(
        &self,
        value: &MirEvalValue,
        ty: &MirType,
        substitutions: &BTreeMap<String, MirType>,
        span: Span,
    ) -> Option<String> {
        let ty = substitute_print_type(ty, substitutions);
        if let Some(def) = self.native_printable_type_def(&ty) {
            let nested_substitutions = self.native_print_substitutions(def, &ty);
            return match &def.kind {
                MirTypeDefKind::Struct { fields, .. } => {
                    self.render_native_struct(value, &def.name, fields, &nested_substitutions, span)
                }
                MirTypeDefKind::Enum { variants, .. } => {
                    self.render_native_enum(value, variants, &nested_substitutions, span)
                }
                MirTypeDefKind::Distinct { .. }
                | MirTypeDefKind::Alias { .. }
                | MirTypeDefKind::UnitFamily { .. } => None,
            };
        }
        match (&ty.kind, value) {
            (MirTypeKind::List(inner), MirEvalValue::List(values))
            | (MirTypeKind::FixedList { elem: inner, .. }, MirEvalValue::List(values)) => {
                let values = values
                    .iter()
                    .map(|value| self.render_native_print_value(value, inner, substitutions, span))
                    .collect::<Option<Vec<_>>>()?;
                Some(format!("[{}]", values.join(", ")))
            }
            (
                MirTypeKind::Map {
                    key: _,
                    value: value_type,
                },
                MirEvalValue::Map(values),
            ) => {
                let values = values
                    .iter()
                    .map(|(key, value)| {
                        Some((
                            mir_print_key(key),
                            self.render_native_print_value(value, value_type, substitutions, span)?,
                        ))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(jet_foundation::StructuralDebug::jet_debug_map(values))
            }
            (MirTypeKind::Option(inner), MirEvalValue::Present(value))
            | (MirTypeKind::Result { ok: inner, .. }, MirEvalValue::Present(value)) => {
                self.render_native_print_value(value, inner, substitutions, span)
            }
            (MirTypeKind::Option(_) | MirTypeKind::Result { .. }, MirEvalValue::Absent { .. }) => {
                Some("null".to_string())
            }
            (MirTypeKind::Option(_) | MirTypeKind::Result { .. }, MirEvalValue::FailedTold(_)) => {
                Some("err".to_string())
            }
            (MirTypeKind::Shared(inner), value)
            | (MirTypeKind::Tagged { inner, .. }, value)
            | (MirTypeKind::InlineRange { base: inner, .. }, value)
            | (MirTypeKind::Quantity { base: inner, .. }, value) => {
                self.render_native_print_value(value, inner, substitutions, span)
            }
            _ => self.native_print_leaf(value, span),
        }
    }

    fn render_native_struct(
        &self,
        value: &MirEvalValue,
        type_name: &str,
        fields: &[jet_foundation::MIR::MirField],
        substitutions: &BTreeMap<String, MirType>,
        span: Span,
    ) -> Option<String> {
        let MirEvalValue::Struct { fields: values, .. } = value else {
            return None;
        };
        let rendered = fields
            .iter()
            .filter(|field| !field.computed)
            .enumerate()
            .map(|(index, field)| {
                let source_name = mir_source_name(&field.name);
                let value = values
                    .iter()
                    .find(|(name, _)| print_names_match(name, &field.name))
                    .map(|(_, value)| value)
                    .or_else(|| values.get(index).map(|(_, value)| value))?;
                let value =
                    self.render_native_print_value(value, &field.ty, substitutions, span)?;
                Some((source_name.to_string(), value))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(jet_foundation::StructuralDebug::jet_debug_record(
            mir_source_name(type_name),
            rendered,
        ))
    }

    fn render_native_enum(
        &self,
        value: &MirEvalValue,
        variants: &[jet_foundation::MIR::MirVariant],
        substitutions: &BTreeMap<String, MirType>,
        span: Span,
    ) -> Option<String> {
        let MirEvalValue::Enum { variant, args, .. } = value else {
            return None;
        };
        let variant_row = variants
            .iter()
            .find(|candidate| print_names_match(&candidate.name, variant))?;
        let variant_name = mir_source_name(&variant_row.name);
        match &variant_row.payload {
            MirVariantPayload::Unit => Some(jet_foundation::StructuralDebug::jet_debug_variant(
                variant_name,
                None,
            )),
            MirVariantPayload::Single(expected) => {
                let (_, value) = args.first()?;
                let value = self.render_native_print_value(value, expected, substitutions, span)?;
                Some(jet_foundation::StructuralDebug::jet_debug_variant(
                    variant_name,
                    Some(value),
                ))
            }
            MirVariantPayload::Named(fields) => {
                let rendered = fields
                    .iter()
                    .filter(|field| !field.computed)
                    .enumerate()
                    .map(|(index, field)| {
                        let value = args
                            .iter()
                            .find(|(name, _)| {
                                name.as_deref()
                                    .is_some_and(|name| print_names_match(name, &field.name))
                            })
                            .map(|(_, value)| value)
                            .or_else(|| args.get(index).map(|(_, value)| value))?;
                        let value =
                            self.render_native_print_value(value, &field.ty, substitutions, span)?;
                        Some((mir_source_name(&field.name).to_string(), value))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(jet_foundation::StructuralDebug::jet_debug_record(
                    variant_name,
                    rendered,
                ))
            }
        }
    }

    fn native_print_leaf(&self, value: &MirEvalValue, span: Span) -> Option<String> {
        match value {
            MirEvalValue::Struct { .. } | MirEvalValue::Enum { .. } => {
                let value =
                    crate::Comptime::MirBridge::mir_to_ct_value(value.clone(), span).ok()?;
                crate::Comptime::display_core_pure_value(&value)
            }
            _ => Some(mir_show(value)),
        }
    }

    fn prelude_row(
        &self,
        call: MirPreludeCallId,
        span: Span,
    ) -> Result<&MirPreludeCall, Diagnostic> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| mir_error_at("MIR PreludeCallId has no canonical row", span))?;
        Ok(row)
    }

    fn ensure_core_route(
        &self,
        call: MirCoreCallId,
        route: MirPreludeCallId,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let row = self
            .program
            .core_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| mir_error_at("MIR CoreCallId has no canonical row", span))?;
        let _ = self.prelude_row(route, span)?;
        if !row.interpreter_route.is_executable() {
            return Err(mir_error_at(
                "MIR CoreCall row has no executable interpreter route",
                span,
            ));
        }
        Ok(())
    }

    fn source_file_path(
        &self,
        file: MirSourceFileId,
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        self.program
            .source_files
            .iter()
            .find(|source| source.id == file)
            .map(|source| MirEvalValue::String(source.path.clone()))
            .ok_or_else(|| mir_error_at("MIR source file ID has no source-file row", span))
    }
    fn value_type(
        &self,
        frame_index: usize,
        value: MirValueId,
        span: Span,
    ) -> Result<&MirType, Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        function
            .values
            .iter()
            .find(|(id, _, _, _)| *id == value)
            .map(|(_, ty, _, _)| ty)
            .ok_or_else(|| mir_error_at("MIR value ID has no type fact", span))
    }

    fn eval_index_read(
        &mut self,
        member: &str,
        args: &[MirEvalValue],
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        match (member, args) {
            (
                "index_list" | "index_list_mut",
                [MirEvalValue::List(items), index, MirEvalValue::String(file), MirEvalValue::Int(line)],
            ) => {
                let index = int_value(index.clone(), span)?;
                crate::fixed_list::jet_fixed_list_index(items.len(), index, |index| {
                    items[index].clone()
                })
                .map_err(|error| {
                    self.located_runtime_stop("E3010", file, *line as u32, &error.message(), span)
                })
            }
            (
                "index_map" | "index_map_mut",
                [MirEvalValue::Map(entries), key, MirEvalValue::String(file), MirEvalValue::Int(line), MirEvalValue::String(function), MirEvalValue::String(source), MirEvalValue::Int(column), MirEvalValue::Int(caret)],
            ) => {
                let wanted = data_key(key, span)?;
                if let Some((_, value)) = entries.iter().find(|(key, _)| key == &wanted) {
                    return Ok(value.clone());
                }
                let message = jet_foundation::Outcome::jet_missing_map_key_value(mir_show(key));
                let report = jet_foundation::Outcome::jet_render_runtime_stop(
                    "E3001",
                    file,
                    *line as u32,
                    function,
                    source,
                    *column as u32,
                    *caret as u32,
                    &message,
                    "",
                );
                self.stderr.push_str(&report.rendered);
                self.exit_code = report.exit_code;
                Err(crate::Sema::Diagnostics::soft_exit(
                    report.exit_code.to_string(),
                    "map index stop".to_string(),
                    Some(span),
                ))
            }
            _ => Err(mir_error_at(
                "MIR index read arguments do not match the checked route",
                span,
            )),
        }
    }

    fn index_prelude_args(
        &self,
        call: MirPreludeCallId,
        base: MirEvalValue,
        index: MirEvalValue,
        location: MirPanicLoc,
        context: Option<&MirPanicContext>,
        span: Span,
    ) -> Result<Vec<MirEvalValue>, Diagnostic> {
        let arity = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .map(|row| row.signature.arity)
            .ok_or_else(|| mir_error_at("MIR PreludeCallId has no canonical row", span))?;
        let mut args = Vec::with_capacity(arity);
        args.push(base);
        args.push(index);
        match arity {
            4 => {
                args.push(self.source_file_path(location.file, span)?);
                args.push(MirEvalValue::Int(i64::from(location.line)));
            }
            6 => {
                let context = context.ok_or_else(|| {
                    mir_error_at(
                        "MIR pool index is missing its canonical panic context",
                        span,
                    )
                })?;
                args.push(self.source_file_path(location.file, span)?);
                args.push(MirEvalValue::Int(i64::from(location.line)));
                args.push(MirEvalValue::String(context.function.clone()));
                args.push(MirEvalValue::String(context.source_line.clone()));
            }
            8 => {
                let context = context.ok_or_else(|| {
                    mir_error_at("MIR map index is missing its canonical panic context", span)
                })?;
                args.push(self.source_file_path(location.file, span)?);
                args.push(MirEvalValue::Int(i64::from(location.line)));
                args.push(MirEvalValue::String(context.function.clone()));
                args.push(MirEvalValue::String(context.source_line.clone()));
                args.push(MirEvalValue::Int(i64::from(location.column)));
                args.push(MirEvalValue::Int(i64::from(context.caret)));
            }
            _ => {
                return Err(mir_error_at(
                    "MIR index arguments do not match the canonical Prelude row",
                    span,
                ));
            }
        }
        Ok(args)
    }
    fn index_setter_args(
        &self,
        call: MirPreludeCallId,
        base: MirEvalValue,
        index: MirEvalValue,
        value: MirEvalValue,
        location: MirPanicLoc,
        context: Option<&MirPanicContext>,
        span: Span,
    ) -> Result<Vec<MirEvalValue>, Diagnostic> {
        let arity = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .map(|row| row.signature.arity)
            .ok_or_else(|| mir_error_at("MIR PreludeCallId has no canonical row", span))?;
        let mut args = Vec::with_capacity(arity);
        args.push(base);
        args.push(index);
        args.push(value);
        match arity {
            3 => {}
            5 => {
                args.push(self.source_file_path(location.file, span)?);
                args.push(MirEvalValue::Int(i64::from(location.line)));
            }
            7 => {
                let context = context.ok_or_else(|| {
                    mir_error_at(
                        "MIR pool index setter is missing its canonical panic context",
                        span,
                    )
                })?;
                args.push(self.source_file_path(location.file, span)?);
                args.push(MirEvalValue::Int(i64::from(location.line)));
                args.push(MirEvalValue::String(context.function.clone()));
                args.push(MirEvalValue::String(context.source_line.clone()));
            }
            _ => {
                return Err(mir_error_at(
                    "MIR index setter arguments do not match the canonical Prelude row",
                    span,
                ));
            }
        }
        Ok(args)
    }

    fn binary_prelude_args(
        &self,
        call: MirPreludeCallId,
        left: MirEvalValue,
        right: MirEvalValue,
        location: Option<MirPanicLoc>,
        span: Span,
    ) -> Result<Vec<MirEvalValue>, Diagnostic> {
        let arity = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .map(|row| row.signature.arity)
            .ok_or_else(|| mir_error_at("MIR PreludeCallId has no canonical row", span))?;
        let mut args = Vec::with_capacity(arity);
        args.push(left);
        args.push(right);
        match arity {
            2 => {}
            4 => {
                let location = location.ok_or_else(|| {
                    mir_error_at("MIR binary Prelude route requires source location", span)
                })?;
                args.push(self.source_file_path(location.file, span)?);
                args.push(MirEvalValue::Int(i64::from(location.line)));
            }
            _ => {
                return Err(mir_error_at(
                    "MIR binary Prelude route has unsupported arity",
                    span,
                ));
            }
        }
        Ok(args)
    }
    fn overflow_option_args(
        &self,
        call: MirPreludeCallId,
        left: MirEvalValue,
        right: MirEvalValue,
        location: Option<MirPanicLoc>,
        span: Span,
    ) -> Result<Vec<MirEvalValue>, Diagnostic> {
        let arity = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .map(|row| row.signature.arity)
            .ok_or_else(|| mir_error_at("MIR PreludeCallId has no canonical row", span))?;
        let mut args = Vec::with_capacity(arity);
        args.push(left);
        args.push(right);
        match arity {
            2 => {
                if location.is_some() {
                    return Err(mir_error_at(
                        "MIR overflow option value route must not carry source location",
                        span,
                    ));
                }
            }
            4 => {
                let location = location.ok_or_else(|| {
                    mir_error_at(
                        "MIR overflow option trap route requires source location",
                        span,
                    )
                })?;
                args.push(self.source_file_path(location.file, span)?);
                args.push(MirEvalValue::Int(i64::from(location.line)));
            }
            _ => {
                return Err(mir_error_at(
                    "MIR overflow option route has unsupported arity",
                    span,
                ));
            }
        }
        Ok(args)
    }

    fn eval_mutating_builtin_place(
        &mut self,
        frame_index: usize,
        call: MirPreludeCallId,
        place_id: MirPlaceId,
        args: &[MirValueId],
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<Option<RuntimeValue>, Diagnostic> {
        let member = self.prelude_row(call, span)?.member.clone();
        let values = args
            .iter()
            .map(|value| self.value(frame_index, *value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?
            .into_iter()
            .map(|value| runtime_to_data(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        if let Some(receiver) = self.place_data_mut(frame_index, place_id, span)? {
            let result = mutate_mir_receiver(receiver, &member, values, result_ty, span)?;
            return Ok(Some(RuntimeValue::Data(result)));
        }

        let mut receiver = runtime_to_data(self.read_place(frame_index, place_id, span)?, span)?;
        let result = mutate_mir_receiver(&mut receiver, &member, values, result_ty, span)?;
        self.write_place(frame_index, place_id, RuntimeValue::Data(receiver), span)?;
        Ok(Some(RuntimeValue::Data(result)))
    }

    fn eval_stream_operator(
        &mut self,
        member: &str,
        args: Vec<RuntimeValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let mut args = args.into_iter();
        let receiver = args
            .next()
            .ok_or_else(|| mir_error_at("stream operator has no receiver", span))?;
        let source = match receiver {
            RuntimeValue::Stream(handle) => handle,
            RuntimeValue::Data(MirEvalValue::List(values)) => {
                Rc::new(RefCell::new(MirInterpreterStream::Source {
                    values: values.into(),
                }))
            }
            RuntimeValue::Moved => {
                return Err(mir_error_at(
                    "stream operator receiver was already moved",
                    span,
                ))
            }
            _ => {
                return Err(mir_error_at(
                    "interpreter stream operator requires a checked Stream value",
                    span,
                ))
            }
        };

        match member {
            "stream.with_event_time" | "stream.with_event_time.datetime" => {
                let callback = args
                    .next()
                    .ok_or_else(|| mir_error_at("stream.with_event_time has no callback", span))?;
                Ok(RuntimeValue::Stream(Rc::new(RefCell::new(
                    MirInterpreterStream::EventTime {
                        source,
                        callback,
                        datetime: member == "stream.with_event_time.datetime",
                    },
                ))))
            }
            "stream.key_by" => {
                let callback = args
                    .next()
                    .ok_or_else(|| mir_error_at("stream.key_by has no callback", span))?;
                Ok(RuntimeValue::Stream(Rc::new(RefCell::new(
                    MirInterpreterStream::Keyed { source, callback },
                ))))
            }
            "stream.window" => {
                let window_ns = args
                    .next()
                    .map(|value| runtime_to_data(value, span))
                    .transpose()?
                    .and_then(|value| mir_stream_duration_ns(&value))
                    .ok_or_else(|| {
                        mir_error_at("stream.window requires a Duration window", span)
                    })?;
                let lateness_ns = args
                    .next()
                    .map(|value| runtime_to_data(value, span))
                    .transpose()?
                    .and_then(|value| mir_stream_duration_ns(&value))
                    .ok_or_else(|| {
                        mir_error_at("stream.window requires a Duration watermark", span)
                    })?;
                let late = args
                    .next()
                    .map(|value| runtime_to_data(value, span))
                    .transpose()?
                    .ok_or_else(|| {
                        mir_error_at("stream.window requires a late-event disposition", span)
                    })?;
                if window_ns <= 0 {
                    return Err(mir_error_at(
                        "stream.window requires a positive window Duration",
                        span,
                    ));
                }
                if lateness_ns < 0 {
                    return Err(mir_error_at(
                        "stream.window requires a non-negative watermark Duration",
                        span,
                    ));
                }
                let side_output = match late {
                    MirEvalValue::Enum { variant, .. } if variant == "SideOutput" => true,
                    MirEvalValue::Enum { variant, .. } if variant == "Drop" => false,
                    _ => {
                        return Err(mir_error_at(
                            "stream.window received an unknown late-event disposition",
                            span,
                        ))
                    }
                };
                Ok(RuntimeValue::Stream(Rc::new(RefCell::new(
                    MirInterpreterStream::Window {
                        source,
                        window_ns,
                        lateness_ns,
                        side_output,
                        keys: Vec::new(),
                        active: Vec::new(),
                        side_output_events: Vec::new(),
                        max_event_ns: None,
                        watermark_ns: None,
                        ready: VecDeque::new(),
                        finished: false,
                    },
                ))))
            }
            _ => Err(mir_error_at("unknown interpreter stream operator", span)),
        }
    }

    fn eval_conversion_prelude(
        &mut self,
        call: MirPreludeCallId,
        args: Vec<MirEvalValue>,
        target: &MirType,
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        use jet_foundation::NumericConversion::{
            jet_numeric_bit_count, jet_numeric_checked_widen, jet_numeric_fixed_from_i128,
            jet_numeric_float_narrow, jet_numeric_float_to_int, jet_numeric_try_from_fixed,
            JET_NUMERIC_CONVERSION_ERROR, JET_NUMERIC_CONVERSION_TRAP, JET_NUMERIC_WIDEN_TRAP,
        };
        use jet_foundation::{
            jet_unit_conversion_exact, jet_unit_conversion_rounded, UnitRoundingMode,
        };

        let symbol = self.prelude_row(call, span)?.symbol.name().to_string();
        let integer = |index: usize| match args.get(index) {
            Some(MirEvalValue::Int(value)) => Ok(*value),
            _ => Err(mir_error_at(
                "numeric Prelude argument requires an integer",
                span,
            )),
        };
        let float = |index: usize| match args.get(index) {
            Some(MirEvalValue::Float { value, .. }) => Ok(*value),
            _ => Err(mir_error_at(
                "numeric Prelude argument requires a float",
                span,
            )),
        };
        let boolean = |index: usize| match args.get(index) {
            Some(MirEvalValue::Bool(value)) => Ok(*value),
            _ => Err(mir_error_at(
                "numeric Prelude argument requires a boolean",
                span,
            )),
        };
        let text = |index: usize| match args.get(index) {
            Some(MirEvalValue::String(value)) => Ok(value.as_str()),
            _ => Err(mir_error_at("numeric Prelude argument requires text", span)),
        };
        let whole = || match args.first() {
            Some(MirEvalValue::Int(value)) => Ok(Some(i128::from(*value))),
            Some(MirEvalValue::BigInt(value)) => Ok(value.parse::<i128>().ok()),
            _ => Err(mir_error_at(
                "numeric Prelude operand requires an integer",
                span,
            )),
        };
        let raw = || match args.first() {
            Some(MirEvalValue::Int(value)) => Ok(*value as u64),
            Some(MirEvalValue::BigInt(value)) => value
                .parse::<u64>()
                .map_err(|_| mir_error_at("fixed integer carrier exceeds its checked width", span)),
            _ => Err(mir_error_at(
                "numeric Prelude operand requires fixed integer bits",
                span,
            )),
        };
        fn outcome(result: Result<MirEvalValue, impl Into<String>>) -> MirEvalValue {
            match result {
                Ok(value) => MirEvalValue::Present(Box::new(value)),
                Err(error) => {
                    MirEvalValue::FailedTold(Box::new(MirEvalValue::String(error.into())))
                }
            }
        }
        let fixed = |value: i128| match i64::try_from(value) {
            Ok(value) => MirEvalValue::Int(value),
            Err(_) => MirEvalValue::BigInt(value.to_string()),
        };
        let decimal = |value: f64| MirEvalValue::Float { value, f32: false };
        let measurement = |value: f64, relative_uncertainty: f64| {
            let (value, uncertainty) =
                mir_measurement_prelude::jet_measurement_kernel_from_relative(
                    value,
                    relative_uncertainty,
                );
            MirEvalValue::Struct {
                type_name: crate::Syntax::TYPE_MEASUREMENT.to_string(),
                fields: vec![
                    ("value".to_string(), decimal(value)),
                    ("uncertainty".to_string(), decimal(uncertainty)),
                ],
            }
        };

        match symbol.as_str() {
            "jet_inline_range_from_int" => Ok(outcome(
                crate::inline_range::jet_inline_range_from_int(
                    integer(0)?,
                    integer(1)?,
                    integer(2)?,
                )
                .map(|value| fixed(i128::from(value))),
            )),
            "jet_numeric_float_to_int" => Ok(outcome(
                jet_numeric_float_to_int(float(0)?, integer(1)?).map(fixed),
            )),
            "jet_numeric_float_narrow" => {
                Ok(outcome(jet_numeric_float_narrow(float(0)?).map(|value| {
                    MirEvalValue::Float {
                        value: f64::from(value),
                        f32: true,
                    }
                })))
            }
            "jet_numeric_bit_count" => Ok(MirEvalValue::Int(jet_numeric_bit_count(
                integer(0)?,
                integer(1)?,
                integer(2)?,
            ))),
            "jet_numeric_int_bit_count" => {
                let value = args.first().and_then(mir_exact_big).ok_or_else(|| {
                    mir_error_at("numeric Prelude operand requires an exact integer", span)
                })?;
                let method = match integer(1)? {
                    0 => "count_ones",
                    1 => "count_zeros",
                    2 => "leading_zeros",
                    3 => "trailing_zeros",
                    _ => panic!("checked integer population operation"),
                };
                Ok(MirEvalValue::Int(
                    value.bit_count(integer(2)? as u32, method).unwrap_or(0),
                ))
            }
            "jet_numeric_try_from_fixed" => Ok(outcome(
                jet_numeric_try_from_fixed(raw()?, boolean(1)?, integer(2)?).map(fixed),
            )),
            "jet_std::jet_int_try_from_checked" => {
                let kind = integer(1)?;
                Ok(outcome(
                    whole()?
                        .and_then(|value| jet_numeric_fixed_from_i128(value, kind))
                        .map(fixed)
                        .ok_or(JET_NUMERIC_CONVERSION_ERROR),
                ))
            }
            "jet_std::jet_int_checked_fixed" => {
                let kind = integer(1)?;
                match whole()?.and_then(|value| jet_numeric_fixed_from_i128(value, kind)) {
                    Some(value) => Ok(fixed(value)),
                    None => self.numeric_runtime_stop(
                        text(2)?,
                        integer(3)? as u32,
                        JET_NUMERIC_CONVERSION_TRAP,
                        span,
                    ),
                }
            }
            "jet_numeric_checked_widen_at" | "jet_std::jet_int_checked_widen" => {
                let (value, target_f32, file, line) = if symbol == "jet_numeric_checked_widen_at" {
                    let target_f32 = boolean(2)?;
                    (
                        jet_numeric_checked_widen(raw()?, boolean(1)?, target_f32),
                        target_f32,
                        text(3)?,
                        integer(4)?,
                    )
                } else {
                    let target_f32 = boolean(1)?;
                    let value = match args.first() {
                        Some(MirEvalValue::Int(value)) => {
                            jet_numeric_checked_widen(*value as u64, true, target_f32)
                        }
                        Some(MirEvalValue::BigInt(value)) => CtBigInt::from_str(value)
                            .map_err(|_| mir_error_at("invalid exact integer carrier", span))?
                            .checked_widen(target_f32),
                        _ => return Err(mir_error_at("invalid Int widening carrier", span)),
                    };
                    (value, target_f32, text(2)?, integer(3)?)
                };
                match value {
                    Some(value) => Ok(MirEvalValue::Float {
                        value,
                        f32: target_f32,
                    }),
                    None => {
                        self.numeric_runtime_stop(file, line as u32, JET_NUMERIC_WIDEN_TRAP, span)
                    }
                }
            }
            "jet_unit_conversion_exact" => {
                match jet_unit_conversion_exact(float(0)?, text(1)?, text(2)?, text(3)?, text(4)?) {
                    Some(value) => Ok(MirEvalValue::Present(Box::new(decimal(value)))),
                    None => match target.option_inner() {
                        Some(element) => Ok(MirEvalValue::Absent {
                            element: element.clone(),
                        }),
                        None => Err(mir_error_at(
                            "exact unit conversion requires an Option target",
                            span,
                        )),
                    },
                }
            }
            "jet_unit_conversion_exact_measurement" => {
                match jet_unit_conversion_exact(float(0)?, text(1)?, text(2)?, text(3)?, text(4)?) {
                    Some(value) => Ok(MirEvalValue::Present(Box::new(measurement(
                        value,
                        float(5)?,
                    )))),
                    None => match target.option_inner() {
                        Some(element) => Ok(MirEvalValue::Absent {
                            element: element.clone(),
                        }),
                        None => Err(mir_error_at(
                            "exact measured unit conversion requires an Option target",
                            span,
                        )),
                    },
                }
            }
            "jet_unit_conversion_rounded" => {
                let mode = match integer(5)? {
                    0 => UnitRoundingMode::TowardZero,
                    1 => UnitRoundingMode::Floor,
                    2 => UnitRoundingMode::Ceiling,
                    3 => UnitRoundingMode::NearestEven,
                    _ => return Err(mir_error_at("invalid checked unit rounding tag", span)),
                };
                Ok(outcome(
                    jet_unit_conversion_rounded(
                        float(0)?,
                        text(1)?,
                        text(2)?,
                        text(3)?,
                        text(4)?,
                        mode,
                        integer(6)?,
                    )
                    .map(decimal),
                ))
            }
            "jet_unit_conversion_rounded_measurement" => {
                let mode = match integer(5)? {
                    0 => UnitRoundingMode::TowardZero,
                    1 => UnitRoundingMode::Floor,
                    2 => UnitRoundingMode::Ceiling,
                    3 => UnitRoundingMode::NearestEven,
                    _ => return Err(mir_error_at("invalid checked unit rounding tag", span)),
                };
                let relative_uncertainty = float(7)?;
                Ok(outcome(
                    jet_unit_conversion_rounded(
                        float(0)?,
                        text(1)?,
                        text(2)?,
                        text(3)?,
                        text(4)?,
                        mode,
                        integer(6)?,
                    )
                    .map(|value| measurement(value, relative_uncertainty)),
                ))
            }
            _ => self.eval_prelude(call, args, Some(target), span),
        }
    }

    fn numeric_runtime_stop(
        &mut self,
        file: &str,
        line: u32,
        message: &str,
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        Err(self.located_runtime_stop("E3001", file, line, message, span))
    }

    fn eval_prelude(
        &mut self,
        call: MirPreludeCallId,
        args: Vec<MirEvalValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        let values = args.into_iter().map(RuntimeValue::Data).collect();
        runtime_to_data(
            self.eval_prelude_runtime(call, values, result_ty, span)?,
            span,
        )
    }

    /// One exact `Int` operator row (`core.numeric.{add,…,shr}`), evaluated
    /// with the same `CtBigInt` algorithms the AOT `jet_int_*` Prelude and the
    /// Cranelift `JitHeap` use. Division-family rows carry `(file, line)` and
    /// stop with the shared `E3010` arithmetic report exactly like
    /// `jet_arithmetic_stop` on AOT.
    fn eval_exact_int_prelude(
        &mut self,
        member: &str,
        args: Vec<MirEvalValue>,
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        let operand = |index: usize| {
            args.get(index).and_then(mir_exact_big).ok_or_else(|| {
                mir_error_at("exact Int operator requires whole-number operands", span)
            })
        };
        let left = operand(0)?;
        let right = operand(1)?;
        let location = match (args.get(2), args.get(3)) {
            (Some(MirEvalValue::String(file)), Some(MirEvalValue::Int(line))) => {
                Some((file.clone(), *line as u32))
            }
            _ => None,
        };
        let result = match member {
            "add" => Some(left.add(&right)),
            "sub" => Some(left.sub(&right)),
            "mul" => Some(left.mul(&right)),
            "bit_and" => Some(left.bit_and(&right)),
            "bit_or" => Some(left.bit_or(&right)),
            "bit_xor" => Some(left.bit_xor(&right)),
            "div" => left.div_rem(&right).map(|(quotient, _)| quotient),
            "rem" => left.div_rem(&right).map(|(_, remainder)| remainder),
            "div_euclid" => left.div_rem_euclid(&right).map(|(quotient, _)| quotient),
            "rem_euclid" => left.div_rem_euclid(&right).map(|(_, remainder)| remainder),
            "floor_div" => left.div_rem(&right).map(|(quotient, remainder)| {
                if !remainder.is_zero() && mir_big_negative(&left) != mir_big_negative(&right) {
                    quotient.sub(&CtBigInt::from_int(1))
                } else {
                    quotient
                }
            }),
            "mod" => left.div_rem(&right).map(|(_, remainder)| {
                if !remainder.is_zero() && mir_big_negative(&left) != mir_big_negative(&right) {
                    remainder.add(&right)
                } else {
                    remainder
                }
            }),
            "pow" => left.pow(&right),
            "shl" => left.shl(&right),
            "shr" => left.shr(&right),
            _ => {
                return Err(mir_error_at(
                    &format!("exact Int operator row `core.numeric.{member}` has no evaluator"),
                    span,
                ))
            }
        };
        match result {
            Some(value) => Ok(mir_exact_int_value(value)),
            None => {
                let message = match member {
                    "pow" => "Negative default Int exponent",
                    "shl" | "shr" => "Invalid shift count",
                    _ => "divided by zero",
                };
                let (file, line) = location.ok_or_else(|| {
                    mir_error_at("exact Int operator stop requires a source location", span)
                })?;
                Err(self.located_runtime_stop("E3010", &file, line, message, span))
            }
        }
    }

    /// One `precise_builtin_route` row. The type name is the module and the
    /// function is the member: constructors map onto the Core math rows and
    /// methods onto the registered receiver rows, so this adds no second
    /// numeric implementation. `None` means the row has no comptime evaluator
    /// and the caller reports it through the ordinary Core binding.
    fn eval_precise_builtin(
        &mut self,
        type_name: &str,
        func: &str,
        values: &[CtValue],
        span: Span,
    ) -> Option<Result<CtValue, Diagnostic>> {
        // D-TYPE2-IMAG1=A: the precise Complex carrier shares its constructor
        // and arithmetic kernels with comptime/REPL through ComplexParity.
        // MIR values arrive as CtValue fields here; no host-specific complex
        // representation or scalar re-encoding is needed.
        if type_name == crate::Syntax::TYPE_COMPLEX {
            return Some(match func {
                "from_parts" => {
                    let [real, imaginary] = values else {
                        return Some(Err(mir_error_at(
                            "Complex.from_parts requires two numeric parts",
                            span,
                        )));
                    };
                    let Some(real) = crate::Comptime::ComplexParity::part(real) else {
                        return Some(Err(mir_error_at(
                            "Complex.from_parts requires a numeric real part",
                            span,
                        )));
                    };
                    let Some(imaginary) = crate::Comptime::ComplexParity::part(imaginary) else {
                        return Some(Err(mir_error_at(
                            "Complex.from_parts requires a numeric imaginary part",
                            span,
                        )));
                    };
                    Ok(crate::Comptime::ComplexParity::from_parts(real, imaginary))
                }
                "add" | "sub" | "mul" | "div" => {
                    let [left, right] = values else {
                        return Some(Err(mir_error_at(
                            "Complex arithmetic requires two operands",
                            span,
                        )));
                    };
                    crate::Comptime::ComplexParity::binary(func, left, right)
                        .ok_or_else(|| mir_error_at("malformed Complex value", span))
                }
                "abs" => {
                    let [value] = values else {
                        return Some(Err(mir_error_at("Complex.abs requires one operand", span)));
                    };
                    crate::Comptime::ComplexParity::abs(value)
                        .map(|magnitude| CtValue::Float(CtFloat::f64(magnitude)))
                        .ok_or_else(|| mir_error_at("malformed Complex value", span))
                }
                "to_string" => {
                    let [value] = values else {
                        return Some(Err(mir_error_at(
                            "Complex.to_string requires one operand",
                            span,
                        )));
                    };
                    crate::Comptime::ComplexParity::to_string(value)
                        .map(CtValue::Str)
                        .ok_or_else(|| mir_error_at("malformed Complex value", span))
                }
                _ => return None,
            });
        }

        let constructor = match (type_name, func) {
            (_, "from_str") if type_name == crate::Syntax::TYPE_DECIMAL => Some("decimal"),
            (_, "from_parts" | "new") if type_name == crate::Syntax::TYPE_FRACTION => {
                Some("fraction")
            }
            _ => None,
        };
        if let Some(member) = constructor {
            let result = crate::Comptime::apply_core_call_without_ambient_with_type(
                "core.math",
                member,
                values.to_vec(),
                span,
                false,
                None,
            );
            // `core.math.fraction` is the optional constructor (`jet_fraction_new`);
            // `from_parts` is its unconditional twin (`jet_fraction_from_parts`,
            // MathRandomTime.rs), which stops on a zero denominator.
            if func != "from_parts" {
                return Some(result);
            }
            return Some(match result {
                Ok(CtValue::Present(value)) => Ok(*value),
                Ok(_) => {
                    Err(self.located_runtime_stop("E3001", "", 0, "invalid exact quotient", span))
                }
                Err(error) => Err(error),
            });
        }
        let (receiver, rest) = values.split_first()?;
        crate::Comptime::apply_core_pure_method(receiver, func, rest, span)
    }
    fn located_runtime_stop(
        &mut self,
        code: &'static str,
        file: &str,
        line: u32,
        message: &str,
        span: Span,
    ) -> Diagnostic {
        self.last_runtime_stop = Some(code.to_string());
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            code, file, line, "", "", 1, 1, message, "",
        );
        self.stderr.push_str(&report.rendered);
        self.exit_code = report.exit_code;
        crate::Sema::Diagnostics::soft_exit(
            report.exit_code.to_string(),
            "runtime stop".to_string(),
            Some(span),
        )
    }

    fn eval_http_text_runtime(
        &mut self,
        member: &str,
        args: Vec<RuntimeValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        fn field<'a>(value: &'a MirEvalValue, name: &str) -> Option<&'a MirEvalValue> {
            match value {
                MirEvalValue::Struct { fields, .. } => fields
                    .iter()
                    .find_map(|(field, value)| (field == name).then_some(value)),
                _ => None,
            }
        }
        fn body_bytes(value: &MirEvalValue) -> Option<Vec<u8>> {
            match value {
                MirEvalValue::Bytes(bytes) => Some(bytes.clone()),
                MirEvalValue::Struct { .. } => field(value, "body")
                    .and_then(body_bytes)
                    .or_else(|| field(value, "bytes").and_then(body_bytes)),
                MirEvalValue::Present(value) => body_bytes(value),
                _ => None,
            }
        }
        let mut args = args.into_iter();
        let receiver = args
            .next()
            .ok_or_else(|| mir_error_at("MIR HTTP text route has no receiver", span))?;
        let limit_arg = args.next();
        if args.next().is_some() {
            return Err(mir_error_at(
                "MIR HTTP text route received extra arguments",
                span,
            ));
        }
        let limit = match member {
            "request_text" | "response_text" => {
                if limit_arg.is_some() {
                    return Err(mir_error_at(
                        "MIR HTTP text route received an unexpected limit",
                        span,
                    ));
                }
                None
            }
            "request_text_with_limit" | "response_text_with_limit" | "body_text" => {
                let Some(RuntimeValue::Data(MirEvalValue::Int(limit))) = limit_arg else {
                    return Err(mir_error_at(
                        "MIR HTTP text route requires an Int limit",
                        span,
                    ));
                };
                Some(limit)
            }
            _ => return Err(mir_error_at("MIR HTTP text route is not registered", span)),
        };
        let receiver = self.materialize_runtime(receiver, span)?;
        let receiver = runtime_to_data(receiver, span)?;
        let bytes = body_bytes(&receiver)
            .ok_or_else(|| mir_error_at("MIR HTTP text route receiver has no body bytes", span))?;
        let result = crate::Comptime::AppLite::http_text_from_bytes(bytes, limit);
        let result = match result {

            Ok(text) => MirEvalValue::Present(Box::new(MirEvalValue::String(text))),
            Err(error) => MirEvalValue::FailedTold(Box::new(
                crate::Comptime::MirBridge::ct_to_mir_value(error, span)?,
            )),
        };
        Ok(RuntimeValue::Data(result))
    }
    fn eval_allocator_runtime(
        &mut self,
        member: &str,
        args: Vec<RuntimeValue>,
        _result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let Some((allocator, operation)) = member.split_once('.') else {
            return Err(mir_error_at("MIR allocator route has no checked owner", span));
        };
        if !matches!(allocator, "Arena" | "Bump" | "Pool" | "Fixed") {
            return Err(mir_error_at("MIR allocator route has an unknown owner", span));
        }
        let (receiver, values) = args
            .split_first()
            .ok_or_else(|| mir_error_at("MIR allocator method has no receiver", span))?;
        let receiver = match receiver {
            RuntimeValue::Address(address) => {
                require_address_access(address, MirAccess::Read, span)?;
                self.read_place(address.frame, address.place, span)?
            }
            receiver => receiver.clone(),
        };
        let RuntimeValue::Ambient(receiver) = receiver else {
            return Err(mir_error_at(
                "MIR allocator receiver has no native owner",
                span,
            ));
        };
        let owner = mir_runtime_owner::<MirAllocatorOwner>(&receiver)
            .ok_or_else(|| mir_error_at("MIR allocator receiver has no native owner", span))?;
        let mut state = owner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let expected_kind = match allocator {
            "Arena" => jet_foundation::MIR::MirAllocatorKind::Arena,
            "Bump" => jet_foundation::MIR::MirAllocatorKind::Bump,
            "Pool" => jet_foundation::MIR::MirAllocatorKind::Pool,
            "Fixed" => jet_foundation::MIR::MirAllocatorKind::Fixed,
            _ => unreachable!("allocator owner checked above"),
        };
        if state.kind != expected_kind {
            return Err(mir_error_at(
                "MIR allocator receiver kind does not match its checked owner",
                span,
            ));
        }
        if state.closed {
            return Err(mir_error_at("MIR allocator is closed", span));
        }
        match operation {
            "alloc" | "try_alloc" => {
                let [value] = values else {
                    return Err(mir_error_at(
                        "MIR allocator allocation requires one value",
                        span,
                    ));
                };
                let value = runtime_to_data(value.clone(), span)?;
                if let Some(error) = mir_allocator_try_charge(&mut state, allocator, &value) {
                    if operation == "try_alloc" {
                        return Ok(mir_alloc_error_result(error));
                    }
                    return Err(mir_error_at("MIR allocator is exhausted", span));
                }
                if operation == "try_alloc" {
                    // AOT clones the placed value out of the fallible Result; keep
                    // the same owned payload so `{value}` shows the Int, not the
                    // opaque owner handle.
                    Ok(RuntimeValue::Result {
                        ok: true,
                        value: Box::new(RuntimeValue::Data(value)),
                    })
                } else {
                    Ok(RuntimeValue::Ambient(mir_runtime_owner_value(
                        MirAllocatorView {
                            state: owner.state.clone(),
                            generation: state.generation,
                            value,
                        },
                    )))
                }
            }
            "reset" if values.is_empty() => {
                state.generation = state.generation.wrapping_add(1);
                state.used = 0;
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            "reset" => Err(mir_error_at(
                "MIR allocator reset received unexpected arguments",
                span,
            )),
            _ => Err(mir_error_at(
                "MIR allocator route has no interpreter implementation",
                span,
            )),
        }
    }

    fn eval_prelude_runtime(
        &mut self,
        call: MirPreludeCallId,
        args: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let row = self.prelude_row(call, span)?;
        if args.len() < row.signature.arity || args.len() > row.signature.max_arity {
            return Err(mir_error_at(
                &format!(
                    "MIR PreludeCallId {call:?} received {} arguments; expected {}..={}",
                    args.len(),
                    row.signature.arity,
                    row.signature.max_arity
                ),
                span,
            ));
        }
        let member = row.member.clone();
        if matches!(
            member.as_str(),
            "stream.with_event_time"
                | "stream.with_event_time.datetime"
                | "stream.key_by"
                | "stream.window"
        ) {
            return self.eval_stream_operator(&member, args, span);
        }
        let module = row.module.clone();
        let member_name = row.member.clone();
        let symbol = row.symbol.name().to_string();
        let family = row.family;
        if family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude && module == "core.clock"
        {
            use crate::Comptime::ClockRuntime;
            let clock = match (member_name.as_str(), args.as_slice()) {
                ("clock_new", [value]) => {
                    let value = self.runtime_to_ct(value.clone(), span)?;
                    ClockRuntime::jet_std_clock_new(crate::Comptime::Builtins::as_int(
                        &value, span,
                    )?)
                }
                ("clock_system", []) => ClockRuntime::jet_std_clock_system(),
                _ => {
                    return Err(mir_error_at(
                        "MIR Clock constructor has an invalid checked call",
                        span,
                    ))
                }
            };
            return Ok(RuntimeValue::Ambient(mir_runtime_owner_value(
                MirClock::new(clock),
            )));
        }
        if family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
            && module == "core.handle"
            && matches!(
                member_name.as_str(),
                "Arena.alloc"
                    | "Arena.try_alloc"
                    | "Arena.reset"
                    | "Bump.alloc"
                    | "Bump.try_alloc"
                    | "Bump.reset"
                    | "Pool.alloc"
                    | "Pool.try_alloc"
                    | "Pool.reset"
                    | "Fixed.alloc"
                    | "Fixed.try_alloc"
                    | "Fixed.reset"
            )
        {
            return self.eval_allocator_runtime(&member_name, args, result_ty, span);
        }
        if family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
            && module == "core.http"
            && matches!(
                member_name.as_str(),
                "request_text"
                    | "request_text_with_limit"
                    | "response_text"
                    | "response_text_with_limit"
                    | "body_text"
            )
        {
            return self.eval_http_text_runtime(&member_name, args, span);
        }
        if family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude
            && module == "core.http"
            && member_name == "project_json_decode_error"
        {
            let mut args = args.into_iter();
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR HTTP JSON projection has no result", span))?;
            if args.next().is_some() {
                return Err(mir_error_at(
                    "MIR HTTP JSON projection received extra arguments",
                    span,
                ));
            }
            let value = self.materialize_runtime(value, span)?;
            let value = self.runtime_to_ct(value, span)?;
            return runtime_from_ct(
                crate::Comptime::AppLite::http_project_json_decode_error(value),
                span,
            );
        }
        if module == "core.index"
            && matches!(
                member_name.as_str(),
                "index_list" | "index_list_mut" | "index_map" | "index_map_mut"
            )
        {
            let values = args
                .into_iter()
                .map(|value| {
                    let value = self.materialize_runtime(value, span)?;
                    runtime_to_data(value, span)
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            return self
                .eval_index_read(&member_name, &values, span)
                .map(RuntimeValue::Data);
        }
        // `exact_int_binary_route` / `TNumericOp` rows: exact `Int` operators
        // carry the packed-Int Prelude symbol, not a `(module, member)` Core
        // row, so they never reach the Core dispatch below.
        if family == jet_foundation::MIR::MirPreludeFamily::Overflow
            && module == "core.numeric"
            && symbol.starts_with("jet_std::jet_int_")
        {
            let values = args
                .into_iter()
                .map(|value| {
                    let value = self.materialize_runtime(value, span)?;
                    runtime_to_data(value, span)
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            return self
                .eval_exact_int_prelude(&member_name, values, span)
                .map(RuntimeValue::Data);
        }
        if family == jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
            && module == "core.numeric"
            && matches!(member_name.as_str(), "bit_count" | "int_bit_count")
        {
            let target = result_ty
                .ok_or_else(|| mir_error_at("numeric method has no checked result type", span))?;
            let args = args
                .into_iter()
                .map(|value| {
                    let value = self.materialize_runtime(value, span)?;
                    runtime_to_data(value, span)
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            return self
                .eval_conversion_prelude(call, args, target, span)
                .map(RuntimeValue::Data);
        }
        if family == jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
            && module == "core.builtin"
        {
            match member_name.as_str() {
                "list_lazy" => {
                    let [receiver] = args.as_slice() else {
                        return Err(mir_error_at(
                            "MIR list.lazy route requires one receiver",
                            span,
                        ));
                    };
                    let source = match receiver.clone() {
                        RuntimeValue::Stream(source) => source,
                        RuntimeValue::Data(MirEvalValue::List(values)) => {
                            Rc::new(RefCell::new(MirInterpreterStream::Source {
                                values: values.into(),
                            }))
                        }
                        RuntimeValue::Moved => {
                            return Err(mir_error_at("MIR list.lazy receiver was moved", span));
                        }
                        _ => {
                            return Err(mir_error_at("MIR list.lazy receiver is not a List", span));
                        }
                    };
                    return Ok(RuntimeValue::Stream(source));
                }
                "iter_take" => {
                    let [receiver, count] = args.as_slice() else {
                        return Err(mir_error_at(
                            "MIR Iter.take route requires a receiver and count",
                            span,
                        ));
                    };
                    let count = int_value(runtime_to_data(count.clone(), span)?, span)?;
                    if let Some(message) =
                        crate::Comptime::CollectionEval::sequence_argument_message("take", count)
                    {
                        return Err(if self.config.runtime_execution {
                            self.located_runtime_stop(
                                "E3001",
                                "<core.collections>",
                                0,
                                message,
                                span,
                            )
                        } else {
                            crate::Comptime::comptime_panic(message, span)
                        });
                    }
                    let remaining = usize::try_from(count)
                        .map_err(|_| mir_error_at("MIR Iter.take count is too large", span))?;
                    let source = match receiver.clone() {
                        RuntimeValue::Stream(source) => source,
                        RuntimeValue::Data(MirEvalValue::List(values)) => {
                            Rc::new(RefCell::new(MirInterpreterStream::Source {
                                values: values.into(),
                            }))
                        }
                        RuntimeValue::Moved => {
                            return Err(mir_error_at("MIR Iter.take receiver was moved", span));
                        }
                        _ => {
                            return Err(mir_error_at(
                                "MIR Iter.take receiver is not an Iter",
                                span,
                            ));
                        }
                    };
                    return Ok(RuntimeValue::Stream(Rc::new(RefCell::new(
                        MirInterpreterStream::Take { source, remaining },
                    ))));
                }
                "try_collect" => {
                    let mut args = args.into_iter();
                    let source = match (args.next(), args.next()) {
                        (Some(RuntimeValue::Stream(source)), None) => source,
                        (Some(RuntimeValue::Data(MirEvalValue::List(values))), None) => {
                            Rc::new(RefCell::new(MirInterpreterStream::Source {
                                values: values.into(),
                            }))
                        }
                        _ => {
                            return Err(mir_error_at(
                                "MIR try_collect route requires one List or Iter receiver",
                                span,
                            ));
                        }
                    };
                    let values = std::iter::from_fn(|| {
                        match mir_stream_pull_handle(&source, self, span) {
                            Ok(None) => None,
                            Ok(Some(MirEvalValue::Present(value))) => Some(Ok(*value)),
                            Ok(Some(value @ (MirEvalValue::FailedTold(_)
                                | MirEvalValue::Absent { .. }))) => Some(Err(Ok(value))),
                            Ok(Some(_)) => Some(Err(Err(mir_error_at(
                                "MIR try_collect element is not an outcome",
                                span,
                            )))),
                            Err(error) => Some(Err(Err(error))),
                        }
                    });
                    return match crate::Comptime::CollectionEval::try_collect(values) {
                        Ok(values) => Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(
                            MirEvalValue::List(values),
                        )))),
                        Err(Ok(failure)) => Ok(RuntimeValue::Data(failure)),
                        Err(Err(error)) => Err(error),
                    };
                }
                _ => {}
            }
        }
        let values = args
            .into_iter()
            .map(|value| {
                let value = self.materialize_runtime(value, span)?;
                self.runtime_to_ct(value, span)
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        if family == jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
            && (module == "core.builtin"
                || (module == "core.list" && member_name == "min_max"))
        {
            let static_method = match member_name.as_str() {
                "int_parse" => Some(("Int", "parse")),
                "int_from_radix" => Some(("Int", "from_radix")),
                _ => None,
            };
            if let Some((owner, method)) = static_method {
                let result = crate::Comptime::Builtins::apply_static_type_method(
                    owner, method, values, span,
                )
                .ok_or_else(|| mir_error_at("MIR static builtin route has no evaluator", span))??;
                return runtime_from_ct(result, span);
            }
            let method = match member_name.as_str() {
                "iter_to_list" => Some("to_list"),
                "iter_collect" => Some("collect"),
                "list_lazy" => Some("lazy"),
                "iter_take" | "list_take" => Some("take"),
                "iter_skip" | "list_skip" => Some("skip"),
                "iter_step_by" => Some("step_by"),
                "iter_dedup" => Some("dedup"),
                "iter_chunks" => Some("chunks"),
                "iter_windows" => Some("windows"),
                "iter_flatten" | "list_flatten" => Some("flatten"),
                "iter_intersperse" => Some("intersperse"),
                "iter_repeat" => Some("repeat"),
                "iter_cycle" => Some("cycle"),
                "iter_drop_last" => Some("drop_last"),
                "iter_shuffle" => Some("shuffle"),
                "iter_is_sorted" => Some("is_sorted"),
                "iter_last_index_of" => Some("last_index_of"),
                "iter_average_int" | "iter_average_float" => Some("average"),
                "iter_compare" => Some("compare"),
                "iter_split" => Some("split"),
                "min_max" => Some("min_max"),
                "map_min" => Some("min"),
                "map_max" => Some("max"),
                "map_top_n" => Some("top_n"),
                _ => None,
            };
            if let Some(method) = method {
                let Some((receiver, args)) = values.split_first() else {
                    return Err(mir_error_at(
                        "MIR builtin method route is missing its receiver",
                        span,
                    ));
                };
                let result =
                    crate::Comptime::Builtins::apply_method(receiver, method, args.to_vec(), span)?;
                return runtime_from_ct(result, span);
            }
        }
        if module == "core.encoding.json"
            && matches!(member_name.as_str(), "to_string" | "to_string_pretty")
        {
            let [value] = values.as_slice() else {
                return Err(mir_error_at(
                    "core.encoding.json renderer expects one DataTree value",
                    span,
                ));
            };
            let rendered = if member_name == "to_string_pretty" {
                crate::Comptime::render_datatree_pretty_for_tir(value)
            } else {
                crate::Comptime::render_datatree_for_tir(value)
            };
            return Ok(RuntimeValue::Data(MirEvalValue::String(rendered)));
        }
        // The static formatter route has no declaration metadata at the
        // comptime boundary. Keep core-owned displays on their shared parity
        // path, but render checked user aggregates from MIR source names
        // instead of delegating them to `CtValue::jet_show` (which must use
        // generated Rust identifiers for AOT debug parity).
        if module == "core.text.fmt" && member_name == "display" {
            if let Some(value) = values.first() {
                if let Some(text) = crate::Comptime::display_core_pure_value(value) {
                    return Ok(RuntimeValue::Data(MirEvalValue::String(text)));
                }
                if matches!(value, CtValue::Struct { .. } | CtValue::Enum { .. }) {
                    let value = crate::Comptime::MirBridge::ct_to_mir_value(value.clone(), span)?;
                    return Ok(RuntimeValue::Data(MirEvalValue::String(mir_show(&value))));
                }
            }
        }

        if family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
            && module == "core.reactive"
        {
            let (_, method) = member_name.split_once('.').ok_or_else(|| {
                mir_error_at("reactive method route has no checked receiver name", span)
            })?;
            let Some((receiver, args)) = values.split_first() else {
                return Err(mir_error_at("reactive method route has no receiver", span));
            };
            let result_type = result_ty.map(crate::Comptime::MirBridge::mir_to_ast_type);
            let result = crate::Comptime::AppLite::apply_web_handle(
                receiver,
                method,
                args,
                span,
                result_type.as_ref(),
            )
            .ok_or_else(|| mir_error_at("reactive method receiver has no Prelude owner", span))??;
            return runtime_from_ct(result, span);
        }
        if family == jet_foundation::MIR::MirPreludeFamily::HandleMethod && module == "core.time" {
            let (_, method) = member_name.split_once('.').ok_or_else(|| {
                mir_error_at("civil time method route has no checked receiver name", span)
            })?;
            let Some((receiver, args)) = values.split_first() else {
                return Err(mir_error_at(
                    "civil time method route has no receiver",
                    span,
                ));
            };
            let result =
                crate::Comptime::Builtins::apply_method(receiver, method, args.to_vec(), span)?;
            return runtime_from_ct(result, span);
        }
        if (family == jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
            && module == "core.builtin"
            && matches!(
                member_name.as_str(),
                "len"
                    | "is_empty"
                    | "get"
                    | "first"
                    | "last"
                    | "count_bytes"
                    | "contains"
                    | "bytes"
            ))
            || (family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
                && module == "core.encoding.datatree"
                && matches!(
                    member_name.as_str(),
                    "field"
                        | "at"
                        | "int"
                        | "text"
                        | "bool"
                        | "float"
                        | "to_text"
                        | "equal_unordered"
                ))
        {
            let Some((receiver, args)) = values.split_first() else {
                return Err(mir_error_at(
                    "builtin method route is missing its receiver",
                    span,
                ));
            };
            let result = crate::Comptime::Builtins::apply_method(
                receiver,
                &member_name,
                args.to_vec(),
                span,
            )?;
            return Ok(RuntimeValue::Data(
                crate::Comptime::MirBridge::ct_to_mir_value(result, span)?,
            ));
        }
        // `precise_builtin_route` rows spell the type as the module
        // (`Decimal::from_str`, `Decimal::add`). Constructors are the Core
        // math rows; methods are the registered receiver rows.
        if family == jet_foundation::MIR::MirPreludeFamily::PreciseBuiltin {
            if let Some(result) = self.eval_precise_builtin(&module, &member_name, &values, span) {
                return Ok(RuntimeValue::Data(
                    crate::Comptime::MirBridge::ct_to_mir_value(result?, span)?,
                ));
            }
        }
        let resolved_ret = result_ty.map(crate::Comptime::MirBridge::mir_to_ast_type);
        let runtime = self.is_runtime_invocation();
        let mut sink = if runtime {
            Some(crate::Comptime::DevSink::default())
        } else {
            None
        };
        let result = match (symbol.as_str(), values.as_slice()) {
            ("jet_journey_reset", []) => Ok({
                jet_foundation::Outcome::jet_journey_reset();
                CtValue::Unit
            }),
            (
                "jet_journey_frame_text",
                [CtValue::Str(file), CtValue::Int(line), CtValue::Str(function), CtValue::Str(note)],
            ) => Ok({
                jet_foundation::Outcome::jet_journey_frame_text(file, *line as u32, function, note);
                CtValue::Unit
            }),
            ("jet_err_from_message", [CtValue::Str(message)]) => Ok(CtValue::from_jet_err(
                &jet_foundation::Outcome::jet_err_from_message(message.clone()),
            )),
            (
                "jet_err_with_context_frame",
                [error, CtValue::Str(file), CtValue::Int(line), CtValue::Str(function), CtValue::Str(note)],
            ) => Ok({
                let error = error.to_jet_err().ok_or_else(|| {
                    mir_error_at(
                        "checked failure context requires the default Err carrier",
                        span,
                    )
                })?;
                CtValue::from_jet_err(&jet_foundation::Outcome::jet_err_with_context_frame(
                    error,
                    file,
                    *line as u32,
                    function,
                    note.clone(),
                ))
            }),
            ("jet_entry_error_exit_jet", [error]) => {
                let error = error.to_jet_err().ok_or_else(|| {
                    mir_error_at(
                        "checked protocol exit requires the default Err carrier",
                        span,
                    )
                })?;
                self.stderr
                    .push_str(&jet_foundation::Outcome::jet_error_report(&error).render());
                self.exit_code = 1;
                return Err(crate::Sema::Diagnostics::soft_exit(
                    "1".to_string(),
                    "propagated error exit".to_string(),
                    Some(span),
                ));
            }
            _ => eval_core_call_binding(
                self.data_pipeline.as_deref_mut(),
                &module,
                &member_name,
                values,
                &[],
                resolved_ret.as_ref(),
                None,
                span,
                runtime,
                &self.config.base_dir,
                sink.as_mut(),
                None,
            ),
        };
        self.merge_runtime_sink(sink);
        let result = result?;
        runtime_from_ct(result, span)
    }
    fn is_direct_collection_closure(module: &str, member: &str) -> bool {
        matches!(
            (module, member),
            (
                "core.list",
                "sort_by"
                    | "sort_by_desc"
                    | "try_sort_by"
                    | "try_sort_by_desc"
                    | "sort_by_compare"
                    | "para_fold"
                    | "map"
                    | "map_mut"
                    | "try_map"
                    | "filter"
                    | "try_filter"
                    | "partition"
                    | "each"
                    | "each_mut"
                    | "find"
                    | "any"
                    | "all"
                    | "reduce"
                    | "take_while"
                    | "skip_while"
                    | "flat_map"
                    | "position"
                    | "min_by"
                    | "max_by"
                    | "group_by"
                    | "count_by"
                    | "scan"
            ) | ("core.iter", _)
                | ("core.view", "map" | "try_map" | "try_filter")
                | ("core.option", "map")
                | ("core.bag", "any")
        )
    }

    fn closure_callback_outcome(
        value: RuntimeValue,
    ) -> Result<Result<RuntimeValue, RuntimeValue>, Diagnostic> {
        match value {
            RuntimeValue::Result { ok: true, value } => Ok(Ok(*value)),
            RuntimeValue::Result { ok: false, value } => {
                Ok(Err(RuntimeValue::Result { ok: false, value }))
            }
            RuntimeValue::Data(MirEvalValue::Present(value)) => Ok(Ok(RuntimeValue::Data(*value))),
            RuntimeValue::Data(MirEvalValue::FailedTold(value)) => {
                Ok(Err(RuntimeValue::Data(MirEvalValue::FailedTold(value))))
            }
            RuntimeValue::Data(value @ MirEvalValue::Absent { .. }) => {
                Ok(Err(RuntimeValue::Data(value)))
            }
            RuntimeValue::Absent { element } => Ok(Err(RuntimeValue::Absent { element })),
            value => Ok(Ok(value)),
        }
    }

    fn closure_callback_data(value: RuntimeValue, span: Span) -> Result<MirEvalValue, Diagnostic> {
        match Self::closure_callback_outcome(value)? {
            Ok(value) => runtime_to_data(value, span),
            Err(_) => Err(mir_error_at(
                "MIR infallible collection callback returned a failure",
                span,
            )),
        }
    }

    fn closure_callback_ordering(
        &mut self,
        callback: &RuntimeValue,
        left: MirEvalValue,
        right: MirEvalValue,
        span: Span,
    ) -> Result<std::cmp::Ordering, Diagnostic> {
        let value = Self::closure_callback_data(
            self.invoke_callback_args(
                callback.clone(),
                vec![RuntimeValue::Data(left), RuntimeValue::Data(right)],
                span,
            )?,
            span,
        )?;
        let MirEvalValue::Enum { variant, .. } = value else {
            return Err(mir_error_at(
                "MIR sort comparator callback must return Ordering",
                span,
            ));
        };
        match variant.as_str() {
            "Less" | "__jet_Less" => Ok(std::cmp::Ordering::Less),
            "Equal" | "__jet_Equal" => Ok(std::cmp::Ordering::Equal),
            "Greater" | "__jet_Greater" => Ok(std::cmp::Ordering::Greater),
            _ => Err(mir_error_at(
                "MIR sort comparator callback returned an unknown Ordering variant",
                span,
            )),
        }
    }

    fn receiver_place_for_value(
        &self,
        frame_index: usize,
        value: MirValueId,
    ) -> Option<MirPlaceId> {
        let function_id = self.frames.get(frame_index)?.function;
        let function = self
            .program
            .functions
            .iter()
            .find(|function| function.id == function_id)?;
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                if instruction.result != Some(value) {
                    return None;
                }
                match &instruction.operation {
                    MirOperation::ReadPlace(place) => Some(*place),
                    _ => None,
                }
            })
    }

    fn write_collection_receiver(
        &mut self,
        frame_index: usize,
        receiver: MirValueId,
        items: Vec<MirEvalValue>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some(place) = self.receiver_place_for_value(frame_index, receiver) else {
            return Ok(());
        };
        self.write_place(
            frame_index,
            place,
            RuntimeValue::Data(MirEvalValue::List(items)),
            span,
        )
    }

    fn eval_collection_closure_method(
        &mut self,
        frame_index: usize,
        receiver_id: MirValueId,
        receiver: RuntimeValue,
        module: &str,
        member: &str,
        args: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if module == "core.iter" && matches!(member, "zip" | "zip_strict" | "zip_pad") {
            let left = match receiver {
                RuntimeValue::Stream(source) => source,
                RuntimeValue::Data(MirEvalValue::List(values)) => {
                    Rc::new(RefCell::new(MirInterpreterStream::Source {
                        values: values.into(),
                    }))
                }
                RuntimeValue::Moved => {
                    return Err(mir_error_at("MIR zip receiver was already moved", span))
                }
                _ => return Err(mir_error_at("MIR zip receiver is not an Iter", span)),
            };
            let mut args = args.into_iter();
            let right = args
                .next()
                .ok_or_else(|| mir_error_at("MIR zip right iterator is missing", span))?;
            let right = match right {
                RuntimeValue::Stream(source) => source,
                RuntimeValue::Data(MirEvalValue::List(values)) => {
                    Rc::new(RefCell::new(MirInterpreterStream::Source {
                        values: values.into(),
                    }))
                }
                RuntimeValue::Moved => {
                    return Err(mir_error_at(
                        "MIR zip right iterator was already moved",
                        span,
                    ))
                }
                _ => return Err(mir_error_at("MIR zip right argument is not an Iter", span)),
            };
            let (mode, callback) = match member {
                "zip" | "zip_strict" => {
                    let callback = args
                        .next()
                        .ok_or_else(|| mir_error_at("MIR zip combine callback is missing", span))?;
                    if args.next().is_some() {
                        return Err(mir_error_at("MIR zip received extra arguments", span));
                    }
                    let mode = if member == "zip" {
                        MirInterpreterZipMode::Short
                    } else {
                        MirInterpreterZipMode::Strict
                    };
                    (mode, callback)
                }
                "zip_pad" => {
                    let left_fill = args
                        .next()
                        .ok_or_else(|| mir_error_at("MIR zip_pad left fill is missing", span))?;
                    let right_fill = args
                        .next()
                        .ok_or_else(|| mir_error_at("MIR zip_pad right fill is missing", span))?;
                    let callback = args.next().ok_or_else(|| {
                        mir_error_at("MIR zip_pad combine callback is missing", span)
                    })?;
                    if args.next().is_some() {
                        return Err(mir_error_at("MIR zip_pad received extra arguments", span));
                    }
                    (
                        MirInterpreterZipMode::Pad {
                            left_fill: runtime_to_data(left_fill, span)?,
                            right_fill: runtime_to_data(right_fill, span)?,
                        },
                        callback,
                    )
                }
                _ => unreachable!("MIR zip route was checked above"),
            };
            return Ok(RuntimeValue::Stream(Rc::new(RefCell::new(
                MirInterpreterStream::Zip {
                    left,
                    right,
                    callback,
                    mode,
                },
            ))));
        }

        if module == "core.option" && member == "map" {
            let mut args = args.into_iter();
            let callback = args
                .next()
                .ok_or_else(|| mir_error_at("MIR Option map callback is missing", span))?;
            if args.next().is_some() {
                return Err(mir_error_at(
                    "MIR Option map received extra arguments",
                    span,
                ));
            }
            return match runtime_to_data(receiver, span)? {
                MirEvalValue::Present(value) => {
                    let value = self.invoke_callback(callback, RuntimeValue::Data(*value), span)?;
                    let value = Self::closure_callback_data(value, span)?;
                    Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(value))))
                }
                MirEvalValue::Absent { element } => {
                    Ok(RuntimeValue::Data(MirEvalValue::Absent { element }))
                }
                _ => Err(mir_error_at(
                    "MIR Option map receiver is not an Option",
                    span,
                )),
            };
        }
        if module == "core.bag" && member == "any" {
            let mut args = args.into_iter();
            let callback = args
                .next()
                .ok_or_else(|| mir_error_at("MIR bag any callback is missing", span))?;
            if args.next().is_some() {
                return Err(mir_error_at("MIR bag any received extra arguments", span));
            }
            let value = runtime_to_data(receiver, span)?;
            let MirEvalValue::Map(entries) = value else {
                return Err(mir_error_at("MIR bag any receiver is not a Tally", span));
            };
            fn key_value(key: &MirConstKey) -> MirEvalValue {
                mir_const_key_value(key)
            }
            for (key, count) in entries {
                if !matches!(count, MirEvalValue::Int(value) if value != 0) {
                    continue;
                }
                let value = self.invoke_callback(
                    callback.clone(),
                    RuntimeValue::Data(key_value(&key)),
                    span,
                )?;
                let value = Self::closure_callback_data(value, span)?;
                if let MirEvalValue::Bool(true) = value {
                    return Ok(RuntimeValue::Data(MirEvalValue::Bool(true)));
                }
            }
            return Ok(RuntimeValue::Data(MirEvalValue::Bool(false)));
        }
        if module == "core.iter"
            && matches!(member, "try_map" | "try_filter")
            && matches!(receiver, RuntimeValue::Stream(_))
        {
            let RuntimeValue::Stream(source) = receiver else {
                unreachable!()
            };
            let callback = args
                .into_iter()
                .next()
                .ok_or_else(|| mir_error_at("MIR collection callback is missing", span))?;
            let mut output = Vec::new();
            loop {
                let Some(item) = mir_stream_pull_handle(&source, self, span)? else {
                    break;
                };
                let value =
                    self.invoke_callback(callback.clone(), RuntimeValue::Data(item.clone()), span)?;
                let value = match Self::closure_callback_outcome(value)? {
                    Ok(value) => runtime_to_data(value, span)?,
                    Err(failure) => return Ok(failure),
                };
                if member == "try_map" {
                    output.push(value);
                } else {
                    let MirEvalValue::Bool(keep) = value else {
                        return Err(mir_error_at(
                            "MIR try_filter callback must return Bool",
                            span,
                        ));
                    };
                    if keep {
                        output.push(item);
                    }
                }
            }
            return Ok(RuntimeValue::Result {
                ok: true,
                value: Box::new(RuntimeValue::Data(MirEvalValue::List(output))),
            });
        }
        if matches!(module, "core.iter" | "core.list")
            && matches!(
                member,
                "each" | "each_mut" | "find" | "any" | "all" | "reduce" | "position"
            )
            && matches!(receiver, RuntimeValue::Stream(_))
        {
            let source = match receiver {
                RuntimeValue::Stream(handle) => handle,
                RuntimeValue::Data(MirEvalValue::List(values)) => {
                    Rc::new(RefCell::new(MirInterpreterStream::Source {
                        values: values.into(),
                    }))
                }
                _ => {
                    return Err(mir_error_at(
                        "MIR Iter callback receiver is not a sequence",
                        span,
                    ))
                }
            };
            let mut callback_args = args.into_iter();
            let seed = if member == "reduce" {
                Some(
                    callback_args
                        .next()
                        .ok_or_else(|| mir_error_at("MIR reduce seed is missing", span))
                        .and_then(|value| runtime_to_data(value, span))?,
                )
            } else {
                None
            };
            let callback = callback_args
                .next()
                .ok_or_else(|| mir_error_at("MIR collection callback is missing", span))?;
            let mut index = 0_i64;
            let mut accumulator = seed;
            loop {
                let Some(item) = mir_stream_pull_handle(&source, self, span)? else {
                    break;
                };
                let value = if member == "reduce" {
                    self.invoke_callback_args(
                        callback.clone(),
                        vec![
                            RuntimeValue::Data(accumulator.take().unwrap()),
                            RuntimeValue::Data(item.clone()),
                        ],
                        span,
                    )?
                } else {
                    self.invoke_callback(callback.clone(), RuntimeValue::Data(item.clone()), span)?
                };
                let value = Self::closure_callback_data(value, span)?;
                match member {
                    "each" | "each_mut" => {}
                    "find" => {
                        let MirEvalValue::Bool(keep) = value else {
                            return Err(mir_error_at("MIR find callback must return Bool", span));
                        };
                        if keep {
                            return Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(item))));
                        }
                    }
                    "any" | "all" => {
                        let MirEvalValue::Bool(keep) = value else {
                            return Err(mir_error_at(
                                "MIR predicate callback must return Bool",
                                span,
                            ));
                        };
                        if (member == "any" && keep) || (member == "all" && !keep) {
                            return Ok(RuntimeValue::Data(MirEvalValue::Bool(member == "any")));
                        }
                    }
                    "position" => {
                        let MirEvalValue::Bool(keep) = value else {
                            return Err(mir_error_at(
                                "MIR position callback must return Bool",
                                span,
                            ));
                        };
                        if keep {
                            return Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(
                                MirEvalValue::Int(index),
                            ))));
                        }
                    }
                    "reduce" => accumulator = Some(value),
                    _ => {}
                }
                index += 1;
            }
            return match member {
                "each" | "each_mut" => Ok(RuntimeValue::Data(MirEvalValue::Unit)),
                "find" | "position" => {
                    Ok(RuntimeValue::Data(direct_absent_value(result_ty, span)?))
                }
                "any" => Ok(RuntimeValue::Data(MirEvalValue::Bool(false))),
                "all" => Ok(RuntimeValue::Data(MirEvalValue::Bool(true))),
                "reduce" => Ok(RuntimeValue::Data(accumulator.unwrap())),
                _ => unreachable!(),
            };
        }
        let lazy_method = matches!(
            (module, member),
            ("core.list", "take_while" | "skip_while" | "scan")
                | ("core.iter", "map" | "map_mut" | "filter" | "filter_map")
                | (
                    "core.iter",
                    "take_while" | "skip_while" | "flat_map" | "scan"
                )
        );
        let receiver = if lazy_method {
            receiver
        } else {
            self.materialize_runtime(receiver, span)?
        };
        let source_for_lazy = match &receiver {
            RuntimeValue::Stream(handle) => Some(handle.clone()),
            RuntimeValue::Data(MirEvalValue::List(values)) => {
                Some(Rc::new(RefCell::new(MirInterpreterStream::Source {
                    values: values.clone().into(),
                })))
            }
            _ => None,
        };
        let mut items = match receiver {
            RuntimeValue::Data(MirEvalValue::List(items)) => items,
            RuntimeValue::Stream(_) if lazy_method => Vec::new(),
            _ => {
                return Err(mir_error_at(
                    "MIR list callback method receiver is not a List",
                    span,
                ))
            }
        };
        if matches!(
            (module, member),
            (
                "core.list",
                "map" | "map_mut" | "filter" | "each" | "each_mut" | "find" | "any" | "all"
            ) | ("core.iter", "map" | "map_mut" | "filter" | "filter_map")
                | ("core.list", "try_map" | "try_filter")
                | ("core.iter", "try_map" | "try_filter")
                | (
                    "core.list",
                    "reduce" | "position" | "min_by" | "max_by" | "group_by" | "count_by"
                )
                | (
                    "core.iter",
                    "reduce" | "position" | "min_by" | "max_by" | "group_by" | "count_by"
                )
                | (
                    "core.list",
                    "take_while" | "skip_while" | "flat_map" | "scan"
                )
                | (
                    "core.iter",
                    "take_while" | "skip_while" | "flat_map" | "scan"
                )
                | ("core.view", "map" | "try_map" | "try_filter")
        ) {
            let scan_seed = if member == "scan" {
                Some(
                    args.first()
                        .cloned()
                        .ok_or_else(|| mir_error_at("MIR scan seed is missing", span))
                        .and_then(|value| runtime_to_data(value, span))?,
                )
            } else {
                None
            };
            let reduce_seed = if member == "reduce" {
                Some(
                    args.first()
                        .cloned()
                        .ok_or_else(|| mir_error_at("MIR reduce seed is missing", span))
                        .and_then(|value| runtime_to_data(value, span))?,
                )
            } else {
                None
            };
            let mut callback_args = args.into_iter();
            let callback = if matches!(member, "scan" | "reduce") {
                callback_args.next();
                callback_args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR reduction callback is missing", span))?
            } else {
                callback_args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR collection callback is missing", span))?
            };
            let lazy = lazy_method;
            let source = source_for_lazy.clone().ok_or_else(|| {
                mir_error_at(
                    "MIR lazy collection method receiver is not a sequence",
                    span,
                )
            })?;
            if lazy {
                let stream = match member {
                    "map" | "map_mut" => MirInterpreterStream::Map { source, callback },
                    "filter_map" => MirInterpreterStream::FilterMap { source, callback },
                    "filter" => MirInterpreterStream::Filter { source, callback },
                    "take_while" => MirInterpreterStream::TakeWhile {
                        source,
                        callback,
                        done: false,
                    },
                    "skip_while" => MirInterpreterStream::SkipWhile {
                        source,
                        callback,
                        skipping: true,
                    },
                    "flat_map" => MirInterpreterStream::FlatMap {
                        source,
                        callback,
                        pending: VecDeque::new(),
                    },
                    "scan" => MirInterpreterStream::Scan {
                        source,
                        callback,
                        accumulator: scan_seed.unwrap_or(MirEvalValue::Unit),
                    },
                    _ => {
                        return Err(mir_error_at(
                            "MIR lazy collection method has no stream adapter",
                            span,
                        ))
                    }
                };
                return Ok(RuntimeValue::Stream(Rc::new(RefCell::new(stream))));
            }
            let mut output = Vec::new();
            match member {
                "map" | "map_mut" => {
                    for item in items {
                        let value =
                            self.invoke_callback(callback.clone(), RuntimeValue::Data(item), span)?;
                        output.push(Self::closure_callback_data(value, span)?);
                    }
                    return Ok(RuntimeValue::Data(MirEvalValue::List(output)));
                }
                "filter" => {
                    for item in items {
                        let value = self.invoke_callback(
                            callback.clone(),
                            RuntimeValue::Data(item.clone()),
                            span,
                        )?;
                        let MirEvalValue::Bool(keep) = Self::closure_callback_data(value, span)?
                        else {
                            return Err(mir_error_at("MIR filter callback must return Bool", span));
                        };
                        if keep {
                            output.push(item);
                        }
                    }
                    return Ok(RuntimeValue::Data(MirEvalValue::List(output)));
                }
                "reduce" => {
                    let mut accumulator = reduce_seed
                        .ok_or_else(|| mir_error_at("MIR reduce seed is missing", span))?;
                    for item in items {
                        let value = self.invoke_callback_args(
                            callback.clone(),
                            vec![RuntimeValue::Data(accumulator), RuntimeValue::Data(item)],
                            span,
                        )?;
                        accumulator = Self::closure_callback_data(value, span)?;
                    }
                    return Ok(RuntimeValue::Data(accumulator));
                }
                "position" => {
                    for (index, item) in items.into_iter().enumerate() {
                        let value =
                            self.invoke_callback(callback.clone(), RuntimeValue::Data(item), span)?;
                        let MirEvalValue::Bool(keep) = Self::closure_callback_data(value, span)?
                        else {
                            return Err(mir_error_at(
                                "MIR position callback must return Bool",
                                span,
                            ));
                        };
                        if keep {
                            return Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(
                                MirEvalValue::Int(index as i64),
                            ))));
                        }
                    }
                    return Ok(RuntimeValue::Data(direct_absent_value(result_ty, span)?));
                }
                "min_by" | "max_by" => {
                    let maximum = member == "max_by";
                    let mut best: Option<(MirEvalValue, MirEvalValue)> = None;
                    for item in items {
                        let key = Self::closure_callback_data(
                            self.invoke_callback(
                                callback.clone(),
                                RuntimeValue::Data(item.clone()),
                                span,
                            )?,
                            span,
                        )?;
                        let replace = match best.as_ref() {
                            None => true,
                            Some((_, current)) => {
                                let order = mir_compare(&key, current, span)?;
                                if maximum {
                                    order.is_gt()
                                } else {
                                    order.is_lt()
                                }
                            }
                        };
                        if replace {
                            best = Some((item, key));
                        }
                    }
                    return match best {
                        Some((item, _)) => {
                            Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(item))))
                        }
                        None => Ok(RuntimeValue::Data(direct_absent_value(result_ty, span)?)),
                    };
                }
                "group_by" | "count_by" => {
                    let grouping = member == "group_by";
                    let mut entries: Vec<(MirConstKey, MirEvalValue)> = Vec::new();
                    for item in items {
                        let key_value = Self::closure_callback_data(
                            self.invoke_callback(
                                callback.clone(),
                                RuntimeValue::Data(item.clone()),
                                span,
                            )?,
                            span,
                        )?;
                        let key = mir_const_key(key_value, span)?;
                        if let Some((_, value)) =
                            entries.iter_mut().find(|(existing, _)| *existing == key)
                        {
                            if grouping {
                                if let MirEvalValue::List(values) = value {
                                    values.push(item);
                                }
                            } else if let MirEvalValue::Int(count) = value {
                                *count += 1;
                            }
                        } else if grouping {
                            entries.push((key, MirEvalValue::List(vec![item])));
                        } else {
                            entries.push((key, MirEvalValue::Int(1)));
                        }
                    }
                    return Ok(RuntimeValue::Data(MirEvalValue::Map(entries)));
                }
                "try_map" | "try_filter" => {
                    for item in items {
                        let value = self.invoke_callback(
                            callback.clone(),
                            RuntimeValue::Data(item.clone()),
                            span,
                        )?;
                        let value = match Self::closure_callback_outcome(value)? {
                            Ok(value) => runtime_to_data(value, span)?,
                            Err(failure) => return Ok(failure),
                        };
                        if member == "try_map" {
                            output.push(value);
                        } else if let MirEvalValue::Bool(keep) = value {
                            if keep {
                                output.push(item);
                            }
                        } else {
                            return Err(mir_error_at(
                                "MIR try_filter callback must return Bool",
                                span,
                            ));
                        }
                    }
                    return Ok(RuntimeValue::Result {
                        ok: true,
                        value: Box::new(RuntimeValue::Data(MirEvalValue::List(output))),
                    });
                }
                "flat_map" => {
                    for item in items {
                        let value =
                            self.invoke_callback(callback.clone(), RuntimeValue::Data(item), span)?;
                        let MirEvalValue::List(values) = Self::closure_callback_data(value, span)?
                        else {
                            return Err(mir_error_at(
                                "MIR flat_map callback must return a List",
                                span,
                            ));
                        };
                        output.extend(values);
                    }
                    return Ok(RuntimeValue::Data(MirEvalValue::List(output)));
                }
                "each" | "each_mut" => {
                    for item in items {
                        let value =
                            self.invoke_callback(callback.clone(), RuntimeValue::Data(item), span)?;
                        let _ = Self::closure_callback_data(value, span)?;
                    }
                    return Ok(RuntimeValue::Data(MirEvalValue::Unit));
                }
                "find" | "any" | "all" => {
                    let mut found = member == "all";
                    for item in items {
                        let value = self.invoke_callback(
                            callback.clone(),
                            RuntimeValue::Data(item.clone()),
                            span,
                        )?;
                        let MirEvalValue::Bool(keep) = Self::closure_callback_data(value, span)?
                        else {
                            return Err(mir_error_at(
                                "MIR predicate callback must return Bool",
                                span,
                            ));
                        };
                        if member == "find" && keep {
                            return Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(item))));
                        }
                        if member == "any" && keep {
                            found = true;
                            break;
                        }
                        if member == "all" && !keep {
                            found = false;
                            break;
                        }
                    }
                    if member == "find" {
                        return Ok(RuntimeValue::Data(direct_absent_value(result_ty, span)?));
                    }
                    return Ok(RuntimeValue::Data(MirEvalValue::Bool(found)));
                }
                _ => {
                    return Err(mir_error_at(
                        "MIR sequence callback has no checked adapter",
                        span,
                    ))
                }
            }
        }
        let mut args = args.into_iter();
        let result = match (module, member) {
            ("core.list", "partition") => {
                let callback = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR partition callback is missing", span))?;
                if args.next().is_some() {
                    return Err(mir_error_at("MIR partition received extra arguments", span));
                }
                let (yes, no) = mir_sort_prelude::jet_list_try_partition_kernel(items, |item| {
                    let value = self.invoke_callback(
                        callback.clone(),
                        RuntimeValue::Data(item.clone()),
                        span,
                    )?;
                    let MirEvalValue::Bool(keep) = Self::closure_callback_data(value, span)? else {
                        return Err(mir_error_at(
                            "MIR partition callback must return Bool",
                            span,
                        ));
                    };
                    Ok(keep)
                })?;
                RuntimeValue::Data(MirEvalValue::Struct {
                    type_name: "Tuple".to_string(),
                    fields: vec![
                        ("false_".to_string(), MirEvalValue::List(no)),
                        ("true_".to_string(), MirEvalValue::List(yes)),
                    ],
                })
            }
            ("core.list", "sort_by")
            | ("core.list", "sort_by_desc")
            | ("core.list", "try_sort_by")
            | ("core.list", "try_sort_by_desc") => {
                let callback = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR sort_by callback is missing", span))?;
                if args.next().is_some() {
                    return Err(mir_error_at("MIR sort_by received extra arguments", span));
                }
                let fallible = member.starts_with("try_");
                let descending = member.ends_with("_desc");
                enum KeyError {
                    Diagnostic(Diagnostic),
                    Failure(RuntimeValue),
                }
                let mut sort_error = None;
                let sorted = mir_sort_prelude::jet_list_try_sort_by_key_kernel(
                    &mut items,
                    |item| {
                        let callback_value = self
                            .invoke_callback_args(
                                callback.clone(),
                                vec![RuntimeValue::Data(item.clone())],
                                span,
                            )
                            .map_err(KeyError::Diagnostic)?;
                        match Self::closure_callback_outcome(callback_value)
                            .map_err(KeyError::Diagnostic)?
                        {
                            Ok(value) if fallible => {
                                runtime_to_data(value, span).map_err(KeyError::Diagnostic)
                            }
                            Ok(value) => Self::closure_callback_data(value, span)
                                .map_err(KeyError::Diagnostic),
                            Err(failure) => Err(KeyError::Failure(failure)),
                        }
                    },
                    |left, right| match mir_compare(left, right, span) {
                        Ok(ordering) if descending => ordering.reverse(),
                        Ok(ordering) => ordering,
                        Err(error) => {
                            sort_error.get_or_insert(error);
                            std::cmp::Ordering::Equal
                        }
                    },
                );
                match sorted {
                    Ok(()) => {}
                    Err(KeyError::Diagnostic(error)) => return Err(error),
                    Err(KeyError::Failure(failure)) if fallible => return Ok(failure),
                    Err(KeyError::Failure(_)) => {
                        return Err(mir_error_at(
                            "MIR sort_by callback returned a failure",
                            span,
                        ))
                    }
                }
                if let Some(error) = sort_error {
                    return Err(error);
                }
                self.write_collection_receiver(frame_index, receiver_id, items, span)?;
                if fallible {
                    RuntimeValue::Result {
                        ok: true,
                        value: Box::new(RuntimeValue::Data(MirEvalValue::Unit)),
                    }
                } else {
                    RuntimeValue::Data(MirEvalValue::Unit)
                }
            }
            ("core.list", "sort_by_compare") => {
                let callback = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR sort_by_compare callback is missing", span))?;
                if args.next().is_some() {
                    return Err(mir_error_at(
                        "MIR sort_by_compare received extra arguments",
                        span,
                    ));
                }
                let mut sort_error = None;
                mir_sort_prelude::jet_list_sort_by_compare_kernel(&mut items, |left, right| {
                    if sort_error.is_some() {
                        return std::cmp::Ordering::Equal;
                    }
                    match self.closure_callback_ordering(
                        &callback,
                        left.clone(),
                        right.clone(),
                        span,
                    ) {
                        Ok(ordering) => ordering,
                        Err(error) => {
                            sort_error.get_or_insert(error);
                            std::cmp::Ordering::Equal
                        }
                    }
                });
                if let Some(error) = sort_error {
                    return Err(error);
                }
                self.write_collection_receiver(frame_index, receiver_id, items, span)?;
                RuntimeValue::Data(MirEvalValue::Unit)
            }
            ("core.list", "para_fold") => {
                let seed = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR para_fold seed is missing", span))?;
                let step = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR para_fold step is missing", span))?;
                let merge = args
                    .next()
                    .ok_or_else(|| mir_error_at("MIR para_fold merge is missing", span))?;
                if args.next().is_some() {
                    return Err(mir_error_at("MIR para_fold received extra arguments", span));
                }
                let value = if items.is_empty() {
                    Self::closure_callback_data(
                        self.invoke_callback_args(seed, Vec::new(), span)?,
                        span,
                    )?
                } else {
                    let indexed = fixed_float_reduction_prelude::jet_list_para_chunks_serial_kernel(
                        items.len(),
                        |range| {
                            let mut acc = Self::closure_callback_data(
                                self.invoke_callback_args(seed.clone(), Vec::new(), span)?,
                                span,
                            )?;
                            for item in &items[range] {
                                acc = Self::closure_callback_data(
                                    self.invoke_callback_args(
                                        step.clone(),
                                        vec![
                                            RuntimeValue::Data(acc),
                                            RuntimeValue::Data(item.clone()),
                                        ],
                                        span,
                                    )?,
                                    span,
                                )?;
                            }
                            Ok::<_, Diagnostic>(acc)
                        },
                    );
                    let partials = indexed
                        .into_iter()
                        .map(|(_, result)| result)
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    fixed_float_reduction_prelude::jet_list_para_merge_tree(
                        partials,
                        |left, right| {
                            Self::closure_callback_data(
                                self.invoke_callback_args(
                                    merge.clone(),
                                    vec![RuntimeValue::Data(left), RuntimeValue::Data(right)],
                                    span,
                                )?,
                                span,
                            )
                        },
                    )?
                };
                RuntimeValue::Data(value)
            }
            _ => {
                return Err(mir_error_at(
                    "MIR collection callback method has no direct evaluator",
                    span,
                ));
            }
        };
        let _ = result_ty;
        Ok(result)
    }
    fn eval_carrier_fact(
        &mut self,
        call: MirPreludeCallId,
        receiver: RuntimeValue,
        field: MirFieldId,
        notes: bool,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let row = self.prelude_row(call, span)?;
        if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
            return Err(mir_error_at(
                "MIR carrier fact route has unsupported Prelude ABI",
                span,
            ));
        }
        let field = field_name(self.program, field)
            .ok_or_else(|| mir_error_at("MIR carrier fact field ID is missing", span))?;
        let receiver = runtime_to_data(receiver, span)?;
        let report = |value: &MirEvalValue| match value {
            MirEvalValue::Struct { fields, .. } => fields
                .iter()
                .find(|(name, _)| name == &field)
                .map(|(_, value)| value.clone()),
            _ => None,
        };
        let outcome: Result<(), &MirEvalValue> = match &receiver {
            MirEvalValue::FailedTold(value) => Err(value.as_ref()),
            _ => Ok(()),
        };
        if notes {
            let notes = jet_foundation::Outcome::jet_notes(&outcome, |value| match report(value) {
                Some(MirEvalValue::List(values)) => values,
                _ => Vec::new(),
            });
            return Ok(RuntimeValue::Data(MirEvalValue::List(notes)));
        }
        match jet_foundation::Outcome::jet_partial(&outcome, |value: &&MirEvalValue| report(*value))
        {
            Ok(Some(value)) => Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(value)))),
            Ok(None) => Err(mir_error_at(
                "MIR carrier report has no requested partial field",
                span,
            )),
            Err(_) => {
                let element = result_ty
                    .and_then(MirType::option_inner)
                    .cloned()
                    .ok_or_else(|| {
                        mir_error_at(
                            "MIR carrier partial route has no Option element type fact",
                            span,
                        )
                    })?;
                Ok(RuntimeValue::Absent { element })
            }
        }
    }

    fn eval_typed_text_interp(
        &mut self,
        frame_index: usize,
        call: MirPreludeCallId,
        kind: jet_foundation::Syntax::TypedHeadKind,
        literals: &[String],
        holes: &[MirValueId],
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if literals.len() != holes.len().saturating_add(1) {
            return Err(mir_error_at(
                "MIR typed text interpolation literal/hole arity is inconsistent",
                span,
            ));
        }
        let row = self.prelude_row(call, span)?;
        if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
            return Err(mir_error_at(
                "MIR typed text route has unsupported Prelude ABI",
                span,
            ));
        }
        let values = holes
            .iter()
            .map(|value| self.value(frame_index, *value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let values = values
            .into_iter()
            .map(|value| runtime_to_data(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let shown = values.iter().map(mir_show).collect::<Vec<_>>();
        let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
        let value = match kind {
            jet_foundation::Syntax::TypedHeadKind::SQL => {
                let params = holes
                    .iter()
                    .zip(values.iter())
                    .map(|(value, value_data)| {
                        let ty = self.value_type(frame_index, *value, span)?;
                        mir_sql_binding(value_data, ty, span)
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                let (template, _) = mir_typed_text_prelude::jet_typed_sql_interpolate::<String>(
                    &literal_refs,
                    shown,
                );
                MirEvalValue::Struct {
                    type_name: "SQL".to_string(),
                    fields: vec![
                        ("template".to_string(), MirEvalValue::String(template)),
                        ("params".to_string(), MirEvalValue::List(params)),
                    ],
                }
            }
            jet_foundation::Syntax::TypedHeadKind::HTML => {
                let trusted = holes
                    .iter()
                    .map(|value| {
                        self.value_type(frame_index, *value, span)
                            .map(|ty| ty.display_name() == crate::Syntax::TYPE_HTML)
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                MirEvalValue::String(mir_typed_text_prelude::jet_typed_html_interpolate(
                    &literal_refs,
                    shown,
                    &trusted,
                ))
            }
            jet_foundation::Syntax::TypedHeadKind::Sh => MirEvalValue::List(
                mir_typed_text_prelude::jet_typed_sh_interpolate(&literal_refs, shown)
                    .into_iter()
                    .map(MirEvalValue::String)
                    .collect(),
            ),
            jet_foundation::Syntax::TypedHeadKind::Path
            | jet_foundation::Syntax::TypedHeadKind::DateTime
            | jet_foundation::Syntax::TypedHeadKind::URL => {
                let hole_values = shown.iter().cloned().map(CtValue::Str).collect::<Vec<_>>();
                let canonical =
                    crate::Comptime::evaluate_typed_head(kind, literals, &hole_values, span)?;
                let canonical = crate::Comptime::MirBridge::ct_to_mir_value(canonical, span)?;
                let result_ty = result_ty.ok_or_else(|| {
                    mir_error_at("MIR typed boundary result has no canonical type fact", span)
                })?;
                self.canonical_typed_boundary_value(result_ty, canonical, span)?
            }
        };
        if matches!(
            kind,
            jet_foundation::Syntax::TypedHeadKind::URL
                | jet_foundation::Syntax::TypedHeadKind::Path
                | jet_foundation::Syntax::TypedHeadKind::DateTime
        ) && result_ty.is_none()
        {
            return Err(mir_error_at(
                "MIR typed boundary result has no canonical type fact",
                span,
            ));
        }
        Ok(RuntimeValue::Data(value))
    }

    fn canonical_typed_boundary_value(
        &self,
        result_ty: &MirType,
        value: MirEvalValue,
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        let type_id = result_ty.identity.ok_or_else(|| {
            mir_error_at("MIR typed boundary result has no canonical MirTypeId", span)
        })?;
        let definition = self
            .program
            .types
            .iter()
            .find(|definition| definition.id == type_id)
            .ok_or_else(|| mir_error_at("MIR typed boundary type ID has no type row", span))?;
        match &definition.kind {
            MirTypeDefKind::Struct { fields, .. } => {
                let MirEvalValue::Struct { fields: values, .. } = value else {
                    return Err(mir_error_at(
                        "MIR typed boundary constructor returned a non-struct value",
                        span,
                    ));
                };
                let mut canonical = Vec::with_capacity(values.len());
                let mut seen = BTreeSet::new();
                for (name, value) in values {
                    if !seen.insert(name.clone()) {
                        return Err(mir_error_at(
                            "MIR typed boundary constructor returned a duplicate field",
                            span,
                        ));
                    }
                    let field =
                        fields
                            .iter()
                            .find(|field| field.name == name)
                            .ok_or_else(|| {
                                mir_error_at(
                                    "MIR typed boundary constructor returned an unknown field",
                                    span,
                                )
                            })?;
                    let row = self
                        .program
                        .fields
                        .iter()
                        .find(|row| {
                            row.owner == type_id
                                && row.id == field.id
                                && row.field.name == field.name
                        })
                        .ok_or_else(|| {
                            mir_error_at(
                                "MIR typed boundary field has no canonical MirFieldId row",
                                span,
                            )
                        })?;
                    canonical.push((row.field.name.clone(), value));
                }
                Ok(MirEvalValue::Struct {
                    type_name: definition.name.clone(),
                    fields: canonical,
                })
            }
            MirTypeDefKind::Enum { variants, .. } => {
                let MirEvalValue::Enum {
                    variant,
                    args: values,
                    ..
                } = value
                else {
                    return Err(mir_error_at(
                        "MIR typed boundary constructor returned a non-enum value",
                        span,
                    ));
                };
                let variant_row = variants
                    .iter()
                    .find(|candidate| candidate.name == variant)
                    .ok_or_else(|| {
                        mir_error_at(
                            "MIR typed boundary constructor returned an unknown variant",
                            span,
                        )
                    })?;
                let canonical = match &variant_row.payload {
                    MirVariantPayload::Unit => {
                        if !values.is_empty() {
                            return Err(mir_error_at(
                                "MIR typed boundary unit variant has payload values",
                                span,
                            ));
                        }
                        Vec::new()
                    }
                    MirVariantPayload::Single(_) => {
                        if values.len() != 1 || values[0].0.is_some() {
                            return Err(mir_error_at(
                                "MIR typed boundary single variant has the wrong payload",
                                span,
                            ));
                        }
                        values
                    }
                    MirVariantPayload::Named(fields) => {
                        if values.len() != fields.len() {
                            return Err(mir_error_at(
                                "MIR typed boundary named variant has the wrong arity",
                                span,
                            ));
                        }
                        let mut seen = BTreeSet::new();
                        let mut canonical = Vec::with_capacity(values.len());
                        for (name, value) in values {
                            let Some(name) = name else {
                                return Err(mir_error_at(
                                    "MIR typed boundary named variant has an unnamed payload",
                                    span,
                                ));
                            };
                            if !seen.insert(name.clone()) {
                                return Err(mir_error_at(
                                    "MIR typed boundary named variant has a duplicate field",
                                    span,
                                ));
                            }
                            let field = fields
                                .iter()
                                .find(|field| field.name == *name)
                                .ok_or_else(|| {
                                    mir_error_at(
                                        "MIR typed boundary named variant has an unknown field",
                                        span,
                                    )
                                })?;
                            let row = self
                                .program
                                .fields
                                .iter()
                                .find(|row| {
                                    row.owner == type_id
                                        && row.id == field.id
                                        && row.field.name == field.name
                                })
                                .ok_or_else(|| {
                                    mir_error_at(
                                        "MIR typed boundary variant field has no canonical MirFieldId row",
                                        span,
                                    )
                                })?;
                            canonical.push((Some(row.field.name.clone()), value));
                        }
                        canonical
                    }
                };
                Ok(MirEvalValue::Enum {
                    type_name: definition.name.clone(),
                    variant,
                    args: canonical,
                })
            }
            MirTypeDefKind::Distinct { .. }
            | MirTypeDefKind::Alias { .. }
            | MirTypeDefKind::UnitFamily { .. } => Err(mir_error_at(
                "MIR typed boundary type row is not a struct or enum",
                span,
            )),
        }
    }
    fn eval_gc_semantic(
        &mut self,
        call: MirPreludeCallId,
        root: RuntimeValue,
        edges: Vec<RuntimeValue>,
        edit: RuntimeValue,
        index: Option<RuntimeValue>,
        kind: jet_foundation::MIR::MirGcEditKind,
        site: jet_foundation::MIR::MirGcEditSiteId,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let row = self.prelude_row(call, span)?;
        if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
            return Err(mir_error_at(
                "MIR GC edit route has unsupported Prelude ABI",
                span,
            ));
        }
        let RuntimeValue::GcRoot(root) = root else {
            return Err(mir_error_at(
                "MIR GC edit requires its opaque runtime root handle",
                span,
            ));
        };
        let edges = edges
            .into_iter()
            .map(|value| runtime_to_data(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let index = index
            .map(|value| runtime_to_data(value, span))
            .transpose()?;
        {
            let mut state = root
                .try_borrow_mut()
                .map_err(|_| mir_error_at("MIR GC root is already mutably borrowed", span))?;
            match kind {
                jet_foundation::MIR::MirGcEditKind::Clear => {
                    if !edges.is_empty() || index.is_some() {
                        return Err(mir_error_at(
                            "MIR GC clear edit carries unexpected operands",
                            span,
                        ));
                    }
                    state.edges.clear();
                    state.edge_slots.clear();
                }
                jet_foundation::MIR::MirGcEditKind::Pop => {
                    if !edges.is_empty() || index.is_some() {
                        return Err(mir_error_at(
                            "MIR GC pop edit carries unexpected operands",
                            span,
                        ));
                    }
                    state.edges.pop();
                }
                jet_foundation::MIR::MirGcEditKind::RemoveIndex => {
                    if !edges.is_empty() {
                        return Err(mir_error_at(
                            "MIR GC remove-index edit carries edge operands",
                            span,
                        ));
                    }
                    let index = index.ok_or_else(|| {
                        mir_error_at("MIR GC remove-index edit has no index", span)
                    })?;
                    let index = usize::try_from(int_value(index, span)?)
                        .map_err(|_| mir_error_at("MIR GC index is negative", span))?;
                    if index < state.edges.len() {
                        state.edges.remove(index);
                    }
                }
                jet_foundation::MIR::MirGcEditKind::InsertIndex => {
                    let index = index.ok_or_else(|| {
                        mir_error_at("MIR GC insert-index edit has no index", span)
                    })?;
                    let index = usize::try_from(int_value(index, span)?)
                        .map_err(|_| mir_error_at("MIR GC index is negative", span))?;
                    if index > state.edges.len() {
                        return Err(mir_error_at("MIR GC insert index is out of range", span));
                    }
                    state.edges.splice(index..index, edges);
                }
                jet_foundation::MIR::MirGcEditKind::Prepend => {
                    if index.is_some() {
                        return Err(mir_error_at("MIR GC prepend edit carries an index", span));
                    }
                    let mut updated = edges;
                    updated.extend(std::mem::take(&mut state.edges));
                    state.edges = updated;
                }
                jet_foundation::MIR::MirGcEditKind::Additive => {
                    if index.is_some() {
                        return Err(mir_error_at("MIR GC additive edit carries an index", span));
                    }
                    state.edges.extend(edges);
                }
                jet_foundation::MIR::MirGcEditKind::Plain => {
                    if !edges.is_empty() || index.is_some() {
                        return Err(mir_error_at(
                            "MIR plain GC edit carries unexpected operands",
                            span,
                        ));
                    }
                }
                jet_foundation::MIR::MirGcEditKind::EdgeSlot => {
                    if index.is_some() {
                        return Err(mir_error_at("MIR edge-slot edit carries an index", span));
                    }
                    if edges.is_empty() {
                        state.edge_slots.remove(&site.0);
                    } else {
                        state.edge_slots.insert(site.0, edges);
                    }
                    state.edges = state
                        .edge_slots
                        .values()
                        .flat_map(|values| values.iter().cloned())
                        .collect();
                }
            }
        }
        let value = root
            .try_borrow()
            .map_err(|_| mir_error_at("MIR GC root is already borrowed", span))?
            .value
            .clone();
        self.invoke_callback(edit, RuntimeValue::Data(value), span)
    }

    fn validate_closure(&self, closure: &MirClosure, span: Span) -> Result<(), Diagnostic> {
        let function = program_function(self.program, closure.function)?;
        validate_captures(function, &closure.captures)?;
        let expected = function.captures.clone().unwrap_or_default();
        if !capture_facts_equal(&closure.facts, &expected) {
            return Err(mir_error_at(
                "MIR closure capture facts disagree with its target function row",
                span,
            ));
        }
        Ok(())
    }

    fn closure_handle(
        &self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<Rc<MirClosure>, Diagnostic> {
        match value {
            RuntimeValue::Closure(closure) => {
                self.validate_closure(&closure, span)?;
                Ok(closure)
            }
            RuntimeValue::Data(MirEvalValue::Closure(closure)) => {
                let function = program_function(self.program, closure.function)?;
                let captures = closure
                    .captures
                    .into_iter()
                    .map(RuntimeValue::Data)
                    .collect::<Vec<_>>();
                let capture_cells = function
                    .capture_params
                    .iter()
                    .enumerate()
                    .map(|(slot, parameter)| {
                        (matches!(parameter.access, MirAccess::Read | MirAccess::Write)
                            && matches!(parameter.ownership.mode, MirOwnershipMode::Owned))
                        .then(|| {
                            captures
                                .get(slot)
                                .cloned()
                                .map(|value| Rc::new(RefCell::new(value)))
                        })
                        .flatten()
                    })
                    .collect();
                let closure = Rc::new(MirClosure {
                    function: closure.function,
                    captures,
                    capture_cells,
                    facts: function.captures.clone().unwrap_or_default(),
                });
                self.validate_closure(&closure, span)?;
                Ok(closure)
            }
            _ => Err(mir_error_at("MIR callback value is not callable", span)),
        }
    }

    fn closure_parts(
        &self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<
        (
            MirFunctionId,
            Vec<RuntimeValue>,
            Vec<Option<Rc<RefCell<RuntimeValue>>>>,
        ),
        Diagnostic,
    > {
        let closure = self.closure_handle(value, span)?;
        let captures = closure
            .captures
            .iter()
            .enumerate()
            .map(|(slot, value)| {
                closure
                    .capture_cells
                    .get(slot)
                    .and_then(|cell| cell.as_ref())
                    .map(|cell| cell.borrow().clone())
                    .unwrap_or_else(|| value.clone())
            })
            .collect();
        Ok((closure.function, captures, closure.capture_cells.clone()))
    }

    fn aggregate_or_data(
        &self,
        type_name: String,
        fields: Vec<(MirFieldId, RuntimeValue)>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let names = fields
            .iter()
            .map(|(field, _)| {
                field_name(self.program, *field)
                    .ok_or_else(|| mir_error_at("MIR aggregate field ID is missing", span))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut data_fields = Vec::with_capacity(fields.len());
        let mut needs_aggregate = false;
        for ((_, value), name) in fields.iter().zip(names) {
            if runtime_contains_moved(value) {
                return Err(mir_error_at("MIR value was moved", span));
            }
            let Ok(value) = runtime_to_data(value.clone(), span) else {
                needs_aggregate = true;
                break;
            };
            data_fields.push((name, value));
        }
        if needs_aggregate {
            return Ok(RuntimeValue::Aggregate(fields));
        }
        Ok(RuntimeValue::Data(MirEvalValue::Struct {
            type_name,
            fields: data_fields,
        }))
    }

    fn runtime_to_ct(&mut self, value: RuntimeValue, span: Span) -> Result<CtValue, Diagnostic> {
        match value {
            RuntimeValue::Ambient(value) => Ok(value),
            RuntimeValue::Result { ok, value } => {
                let value = Box::new(self.runtime_to_ct(*value, span)?);
                Ok(if ok {
                    CtValue::Present(value)
                } else {
                    CtValue::failed(value)
                })
            }
            RuntimeValue::Closure(_) | RuntimeValue::Data(MirEvalValue::Closure(_)) => {
                self.standalone_closure_value(value, span)
            }
            RuntimeValue::Aggregate(fields) => {
                let fields = fields
                    .into_iter()
                    .map(|(field, value)| {
                        let name = field_name(self.program, field).ok_or_else(|| {
                            mir_error_at("MIR aggregate field ID is missing", span)
                        })?;
                        Ok((name, self.runtime_to_ct(value, span)?))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                Ok(CtValue::Struct {
                    type_name: "aggregate".to_string(),
                    fields,
                })
            }
            RuntimeValue::Address(address) => {
                require_address_access(&address, MirAccess::Read, span)?;
                let value = self.read_place(address.frame, address.place, span)?;
                self.runtime_to_ct(value, span)
            }
            value => {
                crate::Comptime::MirBridge::mir_to_ct_value(runtime_to_data(value, span)?, span)
            }
        }
    }

    fn standalone_closure_value(
        &mut self,
        value: RuntimeValue,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        if let RuntimeValue::Ambient(value) = value {
            let callable = match &value {
                CtValue::Closure(data) => data.opaque.as_ref().is_some_and(|value| {
                    value
                        .downcast_ref::<crate::Comptime::AmbientStandaloneClosure>()
                        .is_some()
                }),
                _ => false,
            };
            return if callable {
                Ok(value)
            } else {
                Err(mir_error_at("MIR native value has no callable owner", span))
            };
        }
        let closure = self.closure_handle(value, span)?;
        let captures = closure
            .captures
            .iter()
            .enumerate()
            .map(|(slot, value)| {
                closure
                    .capture_cells
                    .get(slot)
                    .and_then(|cell| cell.as_ref())
                    .map(|cell| cell.borrow().clone())
                    .unwrap_or_else(|| value.clone())
            })
            .map(|value| self.runtime_to_ct(value, span))
            .collect::<Result<Vec<_>, _>>()?;
        let host = MirClosureHost {
            program: Arc::new(self.program.clone()),
            function: closure.function,
            captures,
            config: self.config.clone(),
        };
        Ok(CtValue::Closure(Arc::new(ClosureData {
            lambda: Lambda {
                take_names: Vec::new(),
                params: Vec::new(),
                result_type: None,
                error_type: None,
                effects: None,
                body: LambdaBody::Block(Vec::new()),
                span: Span::new(0, 0),
                meta: LambdaMeta::default(),
            },
            captured: HashMap::new(),
            return_type: None,
            opaque: Some(CtOpaque::new(
                crate::Comptime::AmbientStandaloneClosure::new(host),
            )),
        })))
    }

    fn invoke_ambient_callback(
        &mut self,
        callback: CtValue,
        args: Vec<RuntimeValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let CtValue::Closure(data) = &callback else {
            return Err(mir_error_at("MIR native callback is not a closure", span));
        };
        let host = data
            .opaque
            .as_ref()
            .and_then(|value| value.downcast_ref::<crate::Comptime::AmbientStandaloneClosure>())
            .ok_or_else(|| mir_error_at("MIR native callback has no callable owner", span))?;
        let mut writebacks = Vec::new();
        let mut values = Vec::with_capacity(args.len());
        for (index, value) in args.into_iter().enumerate() {
            if let RuntimeValue::Address(address) = &value {
                if address.access == MirAccess::Write {
                    writebacks.push((index, address.clone()));
                }
            }
            values.push(self.runtime_to_ct(value, span)?);
        }
        let result = if writebacks.is_empty() {
            host.invoke(values, span)?
        } else {
            let result = host.invoke_mut(&mut values, span)?;
            for (index, address) in writebacks {
                let value = values
                    .get(index)
                    .ok_or_else(|| mir_error_at("MIR callback lost a writable parameter", span))?;
                let value = runtime_from_ct(value.clone(), span)?;
                self.write_place(address.frame, address.place, value, span)?;
            }
            result
        };
        runtime_from_ct(result, span)
    }

    fn invoke_callback_args(
        &mut self,
        callback: RuntimeValue,
        args: Vec<RuntimeValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if let RuntimeValue::Ambient(callback) = callback {
            return self.invoke_ambient_callback(callback, args, span);
        }
        let (function, captures, capture_cells) = self.closure_parts(callback, span)?;
        self.invoke_function_with_capture_cells(function, args, captures, capture_cells, span)
    }

    fn invoke_callback(
        &mut self,
        callback: RuntimeValue,
        value: RuntimeValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        self.invoke_callback_args(callback, vec![value], span)
    }

    fn invoke_function(
        &mut self,
        function: MirFunctionId,
        args: Vec<RuntimeValue>,
        captures: Vec<RuntimeValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        self.invoke_function_with_capture_cells(function, args, captures, Vec::new(), span)
    }

    fn invoke_function_with_capture_cells(
        &mut self,
        function: MirFunctionId,
        args: Vec<RuntimeValue>,
        captures: Vec<RuntimeValue>,
        capture_cells: Vec<Option<Rc<RefCell<RuntimeValue>>>>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let nested_function = program_function(self.program, function)?.name.clone();
        let frame = Frame::with_capture_cells(
            program_function(self.program, function)?,
            args,
            captures,
            capture_cells,
        )?;

        let nested_debugger = self.debugger.take();
        let nested_debug_depth = self.debug_depth.saturating_add(self.frames.len());
        let mut nested = Machine::new_with_realtime_tasks(
            self.program,
            self.config,
            vec![frame],
            self.execution.clone(),
            self.static_values.clone(),
            self.service_callbacks.clone(),
            false,
            self.dma_transfers.clone(),
            self.realtime_tasks.clone(),
            self.process_stdin_tokens.clone(),
            self.next_dma_transfer_id.clone(),
            self.data_pipeline.as_deref_mut(),
        )
        .with_debugger(nested_debugger, nested_function, nested_debug_depth);
        let result = nested.run_runtime();
        self.debugger = nested.take_debugger();
        self.stdout.push_str(&nested.stdout);
        self.stderr.push_str(&nested.stderr);
        self.exit_code = self.exit_code.max(nested.exit_code);
        if nested.last_runtime_stop.is_some() {
            self.last_runtime_stop = nested.last_runtime_stop.take();
        }
        let (value, _, _) = result?;
        let _ = span;
        Ok(value)
    }
    fn eval_foreign_call(
        &mut self,
        id: jet_foundation::MIR::MirForeignId,
        args: Vec<RuntimeValue>,
        _result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let foreign = self
            .program
            .foreign
            .iter()
            .find(|foreign| foreign.id == id)
            .ok_or_else(|| {
                mir_error_at("MIR foreign call ID has no canonical foreign row", span)
            })?;
        if crate::scheduler::jet_scheduler_current_world().is_some() {
            return Err(mir_error_at(
                "E3404: deterministic world has no controlled foreign provider",
                span,
            ));
        }
        if !foreign.target_applicability.interpreter {
            return Err(mir_error_at(
                "MIR foreign call is not applicable to the interpreter target",
                foreign.span,
            ));
        }
        if args.len() != foreign.params.len() {
            return Err(mir_error_at(
                &format!(
                    "MIR foreign call {id:?} received {} arguments; expected {}",
                    args.len(),
                    foreign.params.len()
                ),
                span,
            ));
        }
        let closes_handle = self
            .program
            .handles
            .iter()
            .any(|lifecycle| lifecycle.close_foreign == Some(id));
        let close_tokens = closes_handle
            .then(|| {
                args.iter()
                    .filter_map(|value| match value {
                        RuntimeValue::ForeignHandle { token } => Some(token.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let args = args
            .into_iter()
            .map(|value| match value {
                RuntimeValue::ForeignHandle { token } => token
                    .raw()
                    .map(MirEvalValue::Int)
                    .ok_or_else(|| mir_error_at("MIR foreign handle was moved", span)),
                value => runtime_to_data(value, span),
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        match crate::Comptime::try_ambient_mir_extern_call(foreign, args, span) {
            Some(Ok(value)) => {
                for token in close_tokens {
                    let _ = token.take_raw();
                }
                let return_handle = foreign.handle.filter(|handle| {
                    foreign
                        .return_type
                        .as_ref()
                        .and_then(|ty| self.handle_id_for_type(ty))
                        == Some(*handle)
                });
                if let Some(handle) = return_handle {
                    match value {
                        MirEvalValue::Int(raw) => Ok(RuntimeValue::ForeignHandle {
                            token: MirHandleToken::new(handle, raw),
                        }),
                        _ => Err(mir_error_at(
                            "MIR foreign handle call returned a non-integer native value",
                            span,
                        )),
                    }
                } else {
                    Ok(RuntimeValue::Data(value))
                }
            }
            Some(Err(error)) => Err(error),
            None => Err(mir_error_at(
                "MIR foreign call has no interpreter ambient host binding",
                span,
            )),
        }
    }

    fn close_foreign_handle(
        &mut self,
        token: MirHandleToken,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some((handle, raw)) = token.take_raw() else {
            return Ok(());
        };
        let lifecycle = self
            .program
            .handles
            .iter()
            .find(|lifecycle| lifecycle.id == handle)
            .cloned()
            .ok_or_else(|| mir_error_at("MIR handle ID has no lifecycle row", span))?;
        if lifecycle.payload.library == "core.process"
            && lifecycle.payload.typedef_name == "ProcessStdin"
        {
            self.forget_process_stdin_token(&token);
        }
        if lifecycle.payload.library == "core.process" {
            if lifecycle.payload.typedef_name == "ProcessChild" {
                let stdin = self.process_stdin_tokens.borrow_mut().remove(&raw);
                if let Some(stdin) = stdin {
                    self.close_foreign_handle(stdin, span)?;
                }
            }
            let result = crate::Comptime::try_ambient_mir_handle(
                &lifecycle.payload.close,
                Some(raw),
                Vec::new(),
                span,
            )
            .ok_or_else(|| {
                mir_error_at(
                    "MIR process handle close has no interpreter ambient host binding",
                    span,
                )
            })??;
            return match result {
                crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Unit) => Ok(()),
                crate::Comptime::AmbientMirHandleResult::Value(_) => Err(mir_error_at(
                    "MIR process handle close returned the wrong result carrier",
                    span,
                )),
                crate::Comptime::AmbientMirHandleResult::Handle(_) => Err(mir_error_at(
                    "MIR process handle close returned an opaque handle",
                    span,
                )),
            };
        }
        let close_foreign = lifecycle.close_foreign.or_else(|| {
            self.program
                .foreign
                .iter()
                .find(|foreign| {
                    foreign.handle == Some(handle) && foreign.name == lifecycle.payload.close
                })
                .map(|foreign| foreign.id)
        });
        let close_function = lifecycle.close;
        if let Some(close) = close_foreign {
            let foreign = self
                .program
                .foreign
                .iter()
                .find(|foreign| foreign.id == close)
                .ok_or_else(|| mir_error_at("MIR handle close foreign ID has no row", span))?;
            match crate::Comptime::try_ambient_mir_extern_call(
                foreign,
                vec![MirEvalValue::Int(raw)],
                span,
            ) {
                Some(Ok(_)) => Ok(()),
                Some(Err(error)) => Err(error),
                None => Err(mir_error_at(
                    "MIR handle close has no interpreter ambient host binding",
                    span,
                )),
            }
        } else if let Some(close) = close_function {
            let _ = self.invoke_function(
                close,
                vec![RuntimeValue::ForeignHandle {
                    token: MirHandleToken::new(handle, raw),
                }],
                Vec::new(),
                span,
            )?;
            Ok(())
        } else {
            Err(mir_error_at(
                "MIR owned handle has no canonical close operation",
                span,
            ))
        }
    }
    fn invoke_zero_callback(
        &mut self,
        callback: RuntimeValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if let RuntimeValue::Ambient(callback) = callback {
            return self.invoke_ambient_callback(callback, Vec::new(), span);
        }
        let (function, captures, capture_cells) = self.closure_parts(callback, span)?;
        self.invoke_function_with_capture_cells(function, Vec::new(), captures, capture_cells, span)
    }

    fn function_row(&self, id: MirFunctionId) -> Result<&MirFunction, Diagnostic> {
        program_function(self.program, id)
    }

    fn dispatch_interpreter_job(
        &mut self,
        job_type: &str,
        payload: &crate::Comptime::ServicesLite::JetJobPayload,
        span: Span,
    ) -> Result<
        crate::Comptime::ServicesLite::JetJobResult,
        crate::Comptime::ServicesLite::JetJobError,
    > {
        let Some(job) = self
            .program
            .jobs
            .iter()
            .find(|job| job.name == job_type)
            .cloned()
        else {
            return Err(crate::Comptime::ServicesLite::JetJobError {
                type_id: payload.type_id.clone(),
                reason: "unknown_job_payload".to_string(),
                detail: Some(format!("no checked #Job `{job_type}` exists")),
            });
        };
        let Some(input) = job.inputs.first() else {
            return Err(crate::Comptime::ServicesLite::JetJobError {
                type_id: payload.type_id.clone(),
                reason: "missing_payload_schema".to_string(),
                detail: Some(format!("checked #Job `{job_type}` has no payload input")),
            });
        };
        let expected_schema =
            input
                .ty
                .nominal_name()
                .ok_or_else(|| crate::Comptime::ServicesLite::JetJobError {
                    type_id: payload.type_id.clone(),
                    reason: "unsupported_payload_schema".to_string(),
                    detail: Some(format!("checked #Job `{job_type}` payload is not nominal")),
                })?;
        if expected_schema != payload.type_id {
            return Err(crate::Comptime::ServicesLite::JetJobError {
                type_id: payload.type_id.clone(),
                reason: "unknown_job_payload".to_string(),
                detail: Some(format!(
                    "checked #Job `{job_type}` expects payload schema `{expected_schema}`"
                )),
            });
        }
        let Some(value) = crate::Comptime::ServicesLite::interpreter_job_payload(payload) else {
            return Err(crate::Comptime::ServicesLite::JetJobError {
                type_id: payload.type_id.clone(),
                reason: "payload_decode".to_string(),
                detail: Some(
                    "interpreter payload was not retained in the scoped queue adapter".to_string(),
                ),
            });
        };
        let function = self
            .function_row(job.function)
            .map_err(|error| crate::Comptime::ServicesLite::JetJobError {
                type_id: payload.type_id.clone(),
                reason: "invalid_job_signature".to_string(),
                detail: Some(format!(
                    "checked #Job `{job_type}` function row is unavailable: {error:?}"
                )),
            })?
            .clone();
        if function.params.len() != 1 {
            return Err(crate::Comptime::ServicesLite::JetJobError {
                type_id: payload.type_id.clone(),
                reason: "invalid_job_signature".to_string(),
                detail: Some(format!(
                    "checked #Job `{job_type}` does not have one payload parameter"
                )),
            });
        }
        let result = self
            .invoke_function(
                job.function,
                vec![RuntimeValue::Data(value)],
                Vec::new(),
                span,
            )
            .map_err(|error| crate::Comptime::ServicesLite::JetJobError {
                type_id: payload.type_id.clone(),
                reason: "callback_failed".to_string(),
                detail: Some(format!("{error:?}")),
            })?;
        let result_type = match function.return_type.kind() {
            MirTypeKind::Result { ok, .. } => ok.display_name(),
            _ => function.return_type.display_name(),
        };
        let value = match result {
            RuntimeValue::Data(value) => value,
            _ => {
                return Err(crate::Comptime::ServicesLite::JetJobError {
                    type_id: payload.type_id.clone(),
                    reason: "invalid_result".to_string(),
                    detail: Some("checked #Job returned a non-data value".to_string()),
                })
            }
        };
        let encode = |value: &MirEvalValue| {
            crate::Comptime::ServicesLite::interpreter_job_cbor_bytes(value, span).map_err(
                |error| crate::Comptime::ServicesLite::JetJobError {
                    type_id: payload.type_id.clone(),
                    reason: "encode".to_string(),
                    detail: Some(format!("{error:?}")),
                },
            )
        };
        match value {
            MirEvalValue::FailedTold(error) => Err(crate::Comptime::ServicesLite::JetJobError {
                type_id: payload.type_id.clone(),
                reason: "callback_failed".to_string(),
                detail: Some(format!("{error:?}")),
            }),
            MirEvalValue::Present(value) => Ok(crate::Comptime::ServicesLite::JetJobResult {
                type_id: result_type,
                bytes: encode(value.as_ref())?,
                publish: false,
            }),
            MirEvalValue::Unit => Ok(crate::Comptime::ServicesLite::JetJobResult {
                type_id: result_type,
                bytes: Vec::new(),
                publish: false,
            }),
            value => Ok(crate::Comptime::ServicesLite::JetJobResult {
                type_id: result_type,
                bytes: encode(&value)?,
                publish: false,
            }),
        }
    }

    fn dispatch_service_worker(
        &mut self,
        handler_name: &str,
        endpoint: &crate::Comptime::ServicesLite::JetServiceEndpoint,
        span: Span,
    ) -> Result<(), crate::Comptime::ServicesLite::JetServiceError> {
        let callback = self.service_callbacks.borrow().get(handler_name).cloned();
        let Some(callback) = callback else {
            return Err(crate::Comptime::ServicesLite::JetServiceError::Unknown(
                format!("checked service worker `{handler_name}` has no registered callback"),
            ));
        };
        let scope = crate::Comptime::ServicesLite::jet_services_execution_scope(endpoint)?;
        let callback_result = self.invoke_zero_callback(callback, span).map_err(|error| {
            crate::Comptime::ServicesLite::JetServiceError::Unknown(format!("{error:?}"))
        });
        let result = match callback_result {
            Ok(_) => crate::Comptime::ServicesLite::jet_job_service_queue_tick_dispatch(
                endpoint,
                16,
                |job_type, payload| self.dispatch_interpreter_job(job_type, payload, span),
            )
            .map(|_| ()),
            Err(error) => Err(error),
        };
        drop(scope);
        result
    }

    fn eval_service_start(
        &mut self,
        receiver: MirEvalValue,
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        crate::Comptime::ServicesLite::apply_runtime_start_with_dispatcher(
            &receiver,
            span,
            |handler_name, endpoint| self.dispatch_service_worker(handler_name, endpoint, span),
        )
    }

    fn shape_type_definition(
        &self,
        target: &MirType,
        span: Span,
    ) -> Result<&jet_foundation::MIR::MirTypeDef, Diagnostic> {
        self.program
            .types
            .iter()
            .find(|definition| {
                target.identity == Some(definition.id)
                    || target
                        .nominal_name()
                        .is_some_and(|name| name == definition.name || name == definition.key)
            })
            .ok_or_else(|| mir_error_at("typed Core call target has no checked MIR type row", span))
    }

    fn shape_name_map(
        &self,
        target: &MirType,
        projection: ShapeProjectionKind,
        span: Span,
    ) -> Result<Vec<(String, String)>, Diagnostic> {
        let definition = self.shape_type_definition(target, span)?;
        let fields = match &definition.kind {
            MirTypeDefKind::Struct { fields, .. } => fields,
            _ => {
                return Err(mir_error_at(
                    "typed shape Core call target is not a checked record",
                    span,
                ))
            }
        };
        fields
            .iter()
            .filter(|field| !field.skip && !field.computed)
            .map(|field| {
                let source = field
                    .shape_names
                    .name_for(projection)
                    .ok_or_else(|| mir_error_at("checked MIR field has no shape name", span))?;
                let decode = field
                    .shape_names
                    .name_for(ShapeProjectionKind::Json)
                    .ok_or_else(|| {
                        mir_error_at("checked MIR field has no JSON shape name", span)
                    })?;
                Ok((source.to_string(), decode.to_string()))
            })
            .collect()
    }

    fn typed_encode_function(
        &self,
        target: &Type,
        span: Span,
    ) -> Result<Option<MirFunctionId>, Diagnostic> {
        let target = crate::Comptime::MirBridge::ast_to_mir_type(target);
        let target_name = target.display_name();
        let mut matches = self.program.functions.iter().filter(|function| {
            let MirFunctionForm::TraitMethod {
                owner,
                self_access,
                serde: Some(MirSerdeCodec::Encode),
                ..
            } = &function.form
            else {
                return false;
            };
            *self_access == Some(MirAccess::Read)
                && function.generic_params.is_empty()
                && (owner.same_checked_type(&target)
                    || function
                        .name
                        .strip_suffix("::encode")
                        .is_some_and(|owner| owner == target_name.as_str()))
        });
        let Some(function) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(mir_error_at(
                "checked target has ambiguous Encode MIR methods",
                span,
            ));
        }
        if function.params.len() != 1
            || !function.capture_params.is_empty()
            || function.generator.is_some()
        {
            return Err(mir_error_at(
                "generated Encode MIR method has an invalid callable shape",
                span,
            ));
        }
        Ok(Some(function.id))
    }

    fn invoke_typed_encode(
        &mut self,
        target: &Type,
        value: &CtValue,
        span: Span,
    ) -> Result<Option<CtValue>, String> {
        let Some(function) = self
            .typed_encode_function(target, span)
            .map_err(|error| error.what)?
        else {
            return Ok(None);
        };
        let value = crate::Comptime::MirBridge::ct_to_mir_value(value.clone(), span)
            .map_err(|error| error.what)?;
        let result = self
            .invoke_function(function, vec![RuntimeValue::Data(value)], Vec::new(), span)
            .map_err(|error| error.what)?;
        let RuntimeValue::Data(value) = result else {
            return Err("generated Encode MIR method returned a non-data value".to_string());
        };
        crate::Comptime::MirBridge::mir_to_ct_value(value, span)
            .map(Some)
            .map_err(|error| error.what)
    }

    fn typed_decode_function(
        &self,
        target: &MirType,
        span: Span,
    ) -> Result<Option<MirFunctionId>, Diagnostic> {
        let mut matches = self
            .program
            .functions
            .iter()
            .filter(|function| function.is_decode_for(target));
        let Some(function) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(mir_error_at(
                "checked target has ambiguous Decode MIR methods",
                span,
            ));
        }
        if function.params.len() != 1
            || !function.capture_params.is_empty()
            || function.generator.is_some()
        {
            return Err(mir_error_at(
                "generated Decode MIR method has an invalid callable shape",
                span,
            ));
        }
        Ok(Some(function.id))
    }

    fn invoke_typed_decode(
        &mut self,
        target: &MirType,
        tree: MirEvalValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let Some(function) = self.typed_decode_function(target, span)? else {
            let target = crate::Comptime::MirBridge::mir_to_ast_type(target);
            let tree = crate::Comptime::MirBridge::mir_to_ct_value(tree, span)?;
            let decoded = crate::Comptime::decode_typed_builtin_value_for_mir(&target, &tree)
                .ok_or_else(|| {
                    mir_error_at("checked target has no generated Decode MIR method", span)
                })?;
            let value = match decoded {
                Ok(value) => CtValue::Present(Box::new(value)),
                Err(error) => CtValue::failed(Box::new(error)),
            };
            return crate::Comptime::MirBridge::ct_to_mir_value(value, span)
                .map(RuntimeValue::Data);
        };
        match self.invoke_function(function, vec![RuntimeValue::Data(tree)], Vec::new(), span)? {
            RuntimeValue::Data(
                value @ (MirEvalValue::Present(_) | MirEvalValue::FailedTold(_)),
            ) => Ok(RuntimeValue::Data(value)),
            result @ RuntimeValue::Result { .. } => {
                let value = runtime_to_data(result, span)?;
                match value {
                    value @ (MirEvalValue::Present(_) | MirEvalValue::FailedTold(_)) => {
                        Ok(RuntimeValue::Data(value))
                    }
                    value => Err(mir_error_at(
                        &format!(
                            "generated Decode MIR method returned an unchecked result carrier: {}",
                            mir_show(&value)
                        ),
                        span,
                    )),
                }
            }
            RuntimeValue::Data(value) => Err(mir_error_at(
                &format!(
                    "generated Decode MIR method returned an unchecked result carrier: {}",
                    mir_show(&value)
                ),
                span,
            )),
            _ => Err(mir_error_at(
                "generated Decode MIR method returned a non-data value",
                span,
            )),
        }
    }

    fn typed_decode_error_fields(
        error: MirEvalValue,
        span: Span,
    ) -> Result<(String, String), Diagnostic> {
        let invalid = || mir_error_at("typed Decode returned an invalid FieldError", span);
        let MirEvalValue::Struct { type_name, fields } = error else {
            return Err(invalid());
        };
        if type_name != "FieldError" || fields.len() != 2 {
            return Err(invalid());
        }
        let mut path = None;
        let mut reason = None;
        for (name, value) in fields {
            match (name.as_str(), value) {
                ("path", MirEvalValue::String(value)) => path = Some(value),
                ("reason", MirEvalValue::String(value)) => reason = Some(value),
                _ => return Err(invalid()),
            }
        }
        path.zip(reason).ok_or_else(invalid)
    }

    fn shape_error_result(errors: impl IntoIterator<Item = (String, String)>) -> RuntimeValue {
        let errors = MirEvalValue::List(
            errors
                .into_iter()
                .map(|(path, reason)| MirEvalValue::Struct {
                    type_name: "FieldError".to_string(),
                    fields: vec![
                        ("path".to_string(), MirEvalValue::String(path)),
                        ("reason".to_string(), MirEvalValue::String(reason)),
                    ],
                })
                .collect(),
        );
        RuntimeValue::Data(MirEvalValue::FailedTold(Box::new(errors)))
    }

    fn data_tree_object_map(fields: Vec<(MirConstKey, MirEvalValue)>) -> MirEvalValue {
        MirEvalValue::Enum {
            type_name: "DataTree".to_string(),
            variant: "Object".to_string(),
            args: vec![(None, MirEvalValue::Map(fields))],
        }
    }

    fn data_tree_object(fields: Vec<(String, MirEvalValue)>) -> MirEvalValue {
        Self::data_tree_object_map(
            fields
                .into_iter()
                .map(|(name, value)| (MirConstKey::String(name), value))
                .collect(),
        )
    }

    fn args_shape_tree(values: Vec<(String, mir_args_prelude::JetArgsShapeValue)>) -> MirEvalValue {
        fn convert(value: mir_args_prelude::JetArgsShapeValue) -> MirEvalValue {
            match value {
                mir_args_prelude::JetArgsShapeValue::Bool(value) => MirEvalValue::Bool(value),
                mir_args_prelude::JetArgsShapeValue::Int(value) => MirEvalValue::Int(value),
                mir_args_prelude::JetArgsShapeValue::Float(value) => {
                    MirEvalValue::Float { value, f32: false }
                }
                mir_args_prelude::JetArgsShapeValue::Text(value) => MirEvalValue::String(value),
                mir_args_prelude::JetArgsShapeValue::Array(values) => {
                    MirEvalValue::List(values.into_iter().map(convert).collect())
                }
            }
        }

        Self::data_tree_object(
            values
                .into_iter()
                .map(|(name, value)| (name, convert(value)))
                .collect(),
        )
    }

    fn env_shape_tree(entries: Vec<mir_env_prelude::JetEnvConfigEntry>) -> MirEvalValue {
        fn insert(
            fields: &mut Vec<(MirConstKey, MirEvalValue)>,
            segments: &[String],
            value: MirEvalValue,
        ) {
            let Some(segment) = segments.first() else {
                return;
            };
            if segments.len() == 1 {
                if let Some((_, existing)) = fields.iter_mut().find(|(key, _)| {
                    matches!(key, MirConstKey::String(name) if name.eq_ignore_ascii_case(segment))
                }) {
                    *existing = value;
                } else {
                    fields.push((MirConstKey::String(segment.clone()), value));
                }
                return;
            }
            let existing = fields.iter_mut().find(|(key, child)| {
                matches!(key, MirConstKey::String(name) if name.eq_ignore_ascii_case(segment))
                    && matches!(child, MirEvalValue::Map(_))
            });
            if let Some((_, MirEvalValue::Map(child))) = existing {
                insert(child, &segments[1..], value);
            } else {
                let mut child = Vec::new();
                insert(&mut child, &segments[1..], value);
                fields.push((
                    MirConstKey::String(segment.clone()),
                    MirEvalValue::Map(child),
                ));
            }
        }

        let mut fields = Vec::new();
        for entry in entries {
            insert(
                &mut fields,
                &entry.segments,
                MirEvalValue::String(entry.value),
            );
        }
        Self::data_tree_object_map(fields)
    }

    fn db_value_tree(value: &MirEvalValue, span: Span) -> Result<MirEvalValue, Diagnostic> {
        let MirEvalValue::Enum {
            type_name,
            variant,
            args,
        } = value
        else {
            return Err(mir_error_at(
                "core.db.decode row contains a non-DBValue",
                span,
            ));
        };
        let type_name = type_name
            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(type_name);
        if type_name != "DBValue" {
            return Err(mir_error_at(
                "core.db.decode row contains an unknown value type",
                span,
            ));
        }
        match (variant.as_str(), args.as_slice()) {
            ("Null", []) => Ok(MirEvalValue::Enum {
                type_name: "DataTree".to_string(),
                variant: "Null".to_string(),
                args: Vec::new(),
            }),
            ("Int", [(_, MirEvalValue::Int(value))]) => Ok(MirEvalValue::Enum {
                type_name: "DataTree".to_string(),
                variant: "Int".to_string(),
                args: vec![(None, MirEvalValue::Int(*value))],
            }),
            ("Float", [(_, MirEvalValue::Float { value, .. })]) => Ok(MirEvalValue::Enum {
                type_name: "DataTree".to_string(),
                variant: "Float".to_string(),
                args: vec![(
                    None,
                    MirEvalValue::Float {
                        value: *value,
                        f32: false,
                    },
                )],
            }),
            ("Text", [(_, MirEvalValue::String(value))]) => Ok(MirEvalValue::Enum {
                type_name: "DataTree".to_string(),
                variant: "Text".to_string(),
                args: vec![(None, MirEvalValue::String(value.clone()))],
            }),
            ("Bool", [(_, MirEvalValue::Bool(value))]) => Ok(MirEvalValue::Enum {
                type_name: "DataTree".to_string(),
                variant: "Bool".to_string(),
                args: vec![(None, MirEvalValue::Bool(*value))],
            }),
            ("Blob", [(_, MirEvalValue::Bytes(value))]) => Ok(MirEvalValue::Enum {
                type_name: "DataTree".to_string(),
                variant: "Bytes".to_string(),
                args: vec![(None, MirEvalValue::Bytes(value.clone()))],
            }),
            _ => Err(mir_error_at(
                "core.db.decode row contains an invalid DBValue",
                span,
            )),
        }
    }

    fn db_shape_tree(
        row: MirEvalValue,
        names: &[(String, String)],
        span: Span,
    ) -> Result<MirEvalValue, Diagnostic> {
        let MirEvalValue::Map(row) = row else {
            return Err(mir_error_at(
                "core.db.decode expects a checked database row",
                span,
            ));
        };
        let mut fields = Vec::new();
        for (key, value) in row {
            let MirConstKey::String(key) = key else {
                return Err(mir_error_at("core.db.decode row key is not Text", span));
            };
            let Some((_, decode_name)) = names.iter().find(|(source, _)| source == &key) else {
                continue;
            };
            fields.push((decode_name.clone(), Self::db_value_tree(&value, span)?));
        }
        Ok(Self::data_tree_object(fields))
    }

    fn eval_typed_core_call(
        &mut self,
        row: &jet_foundation::MIR::MirCoreCall,
        args: &[RuntimeValue],
        type_args: &[MirType],
        span: Span,
    ) -> Result<Option<RuntimeValue>, Diagnostic> {
        let typed = matches!(
            (row.module.as_str(), row.member.as_str()),
            ("core.args", "decode")
                | ("core.args", "merge")
                | ("core.sys", "decode")
                | ("core.encoding.json", "decode")
                | ("core.encoding.csv", "decode")
                | ("core.data", "csv")
                | ("core.db", "decode")
        );
        if !typed {
            return Ok(None);
        }
        let [target] = type_args else {
            return Err(mir_error_at(
                "typed Core decode requires one checked target type",
                span,
            ));
        };
        let names = match (row.module.as_str(), row.member.as_str()) {
            ("core.args", _) => self.shape_name_map(target, ShapeProjectionKind::Args, span)?,
            ("core.sys", _) => self.shape_name_map(target, ShapeProjectionKind::Env, span)?,
            ("core.db", _) => self.shape_name_map(target, ShapeProjectionKind::Db, span)?,
            ("core.encoding.json", _) => Vec::new(),
            _ => Vec::new(),
        };
        let argv = std::env::args().collect::<Vec<_>>();
        let program = argv.first().map(String::as_str).unwrap_or("");
        match (row.module.as_str(), row.member.as_str()) {
            ("core.args", "decode") => {
                if !args.is_empty() {
                    return Err(mir_error_at(
                        "core.args.decode takes no explicit arguments",
                        span,
                    ));
                }
                let entry = self
                    .shape_type_definition(target, span)?
                    .cli
                    .clone()
                    .ok_or_else(|| {
                        mir_error_at("typed args target has no checked CLI schema", span)
                    })?;
                let result = mir_args_prelude::shape(&entry, program, &argv, &names);
                Ok(Some(match result {
                    Ok(values) => {
                        self.invoke_typed_decode(target, Self::args_shape_tree(values), span)?
                    }
                    Err(errors) => Self::shape_error_result(
                        errors.into_iter().map(|error| (error.path, error.reason)),
                    ),
                }))
            }
            ("core.args", "merge") => {
                let [flags, settings] = args else {
                    return Err(mir_error_at(
                        "core.args.merge needs flags and settings",
                        span,
                    ));
                };
                let entry = self
                    .shape_type_definition(target, span)?
                    .cli
                    .clone()
                    .ok_or_else(|| {
                        mir_error_at("typed args target has no checked CLI schema", span)
                    })?;
                let flags = runtime_to_data(self.materialize_runtime(flags.clone(), span)?, span)?;
                let settings =
                    runtime_to_data(self.materialize_runtime(settings.clone(), span)?, span)?;
                let MirEvalValue::Struct { fields: flags, .. } = flags else {
                    return Err(mir_error_at(
                        "core.args.merge flags layer is not a record",
                        span,
                    ));
                };
                let MirEvalValue::Struct {
                    fields: settings, ..
                } = settings
                else {
                    return Err(mir_error_at(
                        "core.args.merge settings layer is not a record",
                        span,
                    ));
                };
                let project_fields = |fields: Vec<(String, MirEvalValue)>| {
                    fields
                        .into_iter()
                        .map(|(name, value)| {
                            let name = names
                                .iter()
                                .find(|(source, _)| source == &name)
                                .map(|(_, decode)| decode.clone())
                                .unwrap_or(name);
                            (name, value)
                        })
                        .collect::<Vec<_>>()
                };
                let flags = project_fields(flags);
                let settings = project_fields(settings);
                let result =
                    mir_args_prelude::merge(&entry, program, &argv, &names, flags, settings);
                Ok(Some(match result {
                    Ok(fields) => {
                        self.invoke_typed_decode(target, Self::data_tree_object(fields), span)?
                    }
                    Err(errors) => Self::shape_error_result(
                        errors.into_iter().map(|error| (error.path, error.reason)),
                    ),
                }))
            }
            ("core.sys", "decode") => {
                let [prefix, file, allow] = args else {
                    return Err(mir_error_at(
                        "core.sys.decode needs prefix, file, and allow",
                        span,
                    ));
                };
                let prefix =
                    runtime_to_data(self.materialize_runtime(prefix.clone(), span)?, span)?;
                let file = runtime_to_data(self.materialize_runtime(file.clone(), span)?, span)?;
                let allow = runtime_to_data(self.materialize_runtime(allow.clone(), span)?, span)?;
                let (
                    MirEvalValue::String(prefix),
                    MirEvalValue::String(file),
                    MirEvalValue::List(allow),
                ) = (prefix, file, allow)
                else {
                    return Err(mir_error_at(
                        "core.sys.decode received invalid environment arguments",
                        span,
                    ));
                };
                let allow = allow
                    .into_iter()
                    .map(|value| match value {
                        MirEvalValue::String(value) => Ok(value),
                        _ => Err(mir_error_at(
                            "core.sys.decode allowlist contains non-Text",
                            span,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let result = mir_env_prelude::entries(&prefix, &file, &allow, &names);
                Ok(Some(match result {
                    Ok(entries) => {
                        self.invoke_typed_decode(target, Self::env_shape_tree(entries), span)?
                    }
                    Err(reason) => Self::shape_error_result([(String::new(), reason)]),
                }))
            }
            ("core.encoding.json", "decode") => {
                let [text] = args else {
                    return Err(mir_error_at(
                        "core.encoding.json.decode needs one text argument",
                        span,
                    ));
                };
                let text = runtime_to_data(self.materialize_runtime(text.clone(), span)?, span)?;
                let MirEvalValue::String(text) = text else {
                    return Err(mir_error_at("core.encoding.json.decode expects Text", span));
                };
                let parsed = crate::Comptime::parse_ordered_json_for_tir(&text);
                Ok(Some(match parsed {
                    CtValue::Present(tree) => {
                        let tree = crate::Comptime::MirBridge::ct_to_mir_value(*tree, span)?;
                        self.invoke_typed_decode(target, tree, span)?
                    }
                    CtValue::Failed(report) => RuntimeValue::Data(
                        crate::Comptime::MirBridge::ct_to_mir_value(CtValue::Failed(report), span)?,
                    ),
                    _ => {
                        return Err(mir_error_at(
                            "JSON parser returned an invalid result carrier",
                            span,
                        ))
                    }
                }))
            }
            ("core.encoding.csv", "decode") | ("core.data", "csv") => {
                let [text] = args else {
                    return Err(mir_error_at(
                        "typed CSV decode needs one text argument",
                        span,
                    ));
                };
                let text = runtime_to_data(self.materialize_runtime(text.clone(), span)?, span)?;
                let MirEvalValue::String(text) = text else {
                    return Err(mir_error_at("typed CSV decode expects Text", span));
                };
                let rows = match jet_foundation::CsvKernel::parse(
                    &text,
                    jet_foundation::CsvKernel::CsvOptions::default(),
                ) {
                    Ok(rows) => rows,
                    Err(reason) => {
                        return Ok(Some(Self::shape_error_result([(String::new(), reason)])));
                    }
                };
                let decoder = self.typed_decode_function(target, span)?.ok_or_else(|| {
                    mir_error_at(
                        "checked CSV target has no generated Decode MIR method",
                        span,
                    )
                })?;
                let mut callback_fault = None;
                let decoded = mir_csv_prelude::jet_enc_csv_decode_rows(
                    rows.into_iter().map(|row| row.fields),
                    |tree| {
                        if callback_fault.is_some() {
                            return Err(Vec::new());
                        }
                        let result = self
                            .invoke_function(
                                decoder,
                                vec![RuntimeValue::Data(tree)],
                                Vec::new(),
                                span,
                            )
                            .and_then(|result| match runtime_to_data(result, span)? {
                                MirEvalValue::Present(value) => Ok(Ok(*value)),
                                MirEvalValue::FailedTold(errors) => {
                                    let MirEvalValue::List(errors) = *errors else {
                                        return Err(mir_error_at(
                                            "typed CSV Decode returned an invalid error list",
                                            span,
                                        ));
                                    };
                                    errors
                                        .into_iter()
                                        .map(|error| Self::typed_decode_error_fields(error, span))
                                        .collect::<Result<Vec<_>, _>>()
                                        .map(Err)
                                }
                                _ => Err(mir_error_at(
                                    "typed CSV Decode returned an invalid Result",
                                    span,
                                )),
                            });
                        match result {
                            Ok(result) => result,
                            Err(error) => {
                                callback_fault = Some(error);
                                Err(Vec::new())
                            }
                        }
                    },
                    |row, mut errors| {
                        for (path, _) in &mut errors {
                            *path = mir_csv_prelude::jet_field_error_kernel_under(row, path);
                        }
                        errors
                    },
                    |text| MirEvalValue::Enum {
                        type_name: "DataTree".to_string(),
                        variant: "Text".to_string(),
                        args: vec![(None, MirEvalValue::String(text))],
                    },
                    |fields| Self::data_tree_object(fields),
                );
                if let Some(error) = callback_fault {
                    return Err(error);
                }
                Ok(Some(match decoded {
                    Ok(values) => RuntimeValue::Data(MirEvalValue::Present(Box::new(
                        MirEvalValue::List(values),
                    ))),
                    Err(errors) => Self::shape_error_result(errors),
                }))
            }
            ("core.db", "decode") => {
                let [row] = args else {
                    return Err(mir_error_at("core.db.decode needs one database row", span));
                };
                let row = runtime_to_data(self.materialize_runtime(row.clone(), span)?, span)?;
                let tree = Self::db_shape_tree(row, &names, span)?;
                Ok(Some(self.invoke_typed_decode(target, tree, span)?))
            }
            _ => unreachable!("typed Core route checked above"),
        }
    }

    fn eval_http_router_dispatch(
        &mut self,
        receiver: RuntimeValue,
        request: RuntimeValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let request = self.materialize_runtime(request, span)?;
        let carrier = self.eval_http_router_ambient(
            "http_router.dispatch",
            Some(receiver),
            vec![request],
            None,
            span,
        )?;
        let carrier = runtime_to_data(carrier, span)?;
        let MirEvalValue::Struct { type_name, fields } = carrier else {
            return Err(mir_error_at(
                "MIR HTTPRouter dispatch adapter returned a non-invocation carrier",
                span,
            ));
        };
        if type_name == "HTTPResponse" {
            // Native/Web routers turn transport failures into an HTTP response
            // carried by the dispatch Result.  Keep that boundary in the
            // interpreter too; only a selected route produces an invocation.
            return Ok(RuntimeValue::Result {
                ok: true,
                value: Box::new(RuntimeValue::Data(MirEvalValue::Struct {
                    type_name,
                    fields,
                })),
            });
        }
        if type_name != "HTTPRouteInvocation" {
            return Err(mir_error_at(
                "MIR HTTPRouter dispatch adapter returned an unknown invocation carrier",
                span,
            ));
        }
        let handler = fields
            .iter()
            .find_map(|(name, value)| (name == "handler").then_some(value.clone()))
            .ok_or_else(|| {
                mir_error_at("MIR HTTPRouter dispatch invocation has no handler", span)
            })?;
        let args = fields
            .iter()
            .find_map(|(name, value)| (name == "args").then_some(value.clone()))
            .ok_or_else(|| {
                mir_error_at("MIR HTTPRouter dispatch invocation has no arguments", span)
            })?;
        let MirEvalValue::List(args) = args else {
            return Err(mir_error_at(
                "MIR HTTPRouter dispatch invocation arguments are not a list",
                span,
            ));
        };
        let handler = mir_http_handler_value(&handler).unwrap_or(RuntimeValue::Data(handler));
        let (function, captures, capture_cells) = self.closure_parts(handler, span)?;
        self.invoke_function_with_capture_cells(
            function,
            args.into_iter().map(RuntimeValue::Data).collect(),
            captures,
            capture_cells,
            span,
        )
    }

    fn eval_clock_handle(
        &mut self,
        member: &str,
        receiver: RuntimeValue,
        args: &[MirValueId],
        frame_index: usize,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        use crate::Comptime::ClockRuntime;
        let receiver = self.runtime_to_ct(receiver, span)?;
        let clock = mir_runtime_owner::<MirClock>(&receiver)
            .ok_or_else(|| mir_error_at("MIR Clock receiver has no native owner", span))?;
        let mut clock = clock.lock().unwrap_or_else(|error| error.into_inner());
        let argument = match args {
            [] => None,
            [arg] => {
                let value = self.value(frame_index, *arg, span)?;
                let value = self.materialize_runtime(value, span)?;
                Some(runtime_to_data(value, span)?)
            }
            _ => return Err(mir_error_at("MIR Clock method has extra arguments", span)),
        };
        let integer = |value: MirEvalValue| {
            let value = crate::Comptime::MirBridge::mir_to_ct_value(value, span)?;
            crate::Comptime::Builtins::as_int(&value, span)
        };
        let now = match (member, argument) {
            ("clock.now", None) => ClockRuntime::jet_clock_now(&clock),
            ("clock.tick", Some(value)) => {
                ClockRuntime::jet_clock_tick(&mut clock, integer(value)?)
            }
            ("clock.advance", Some(value)) => {
                ClockRuntime::jet_clock_advance(&mut clock, integer(value)?)
            }
            ("clock.wait", Some(value)) => {
                let ns = mir_time_field(&value, crate::Syntax::DURATION_TYPE, "ns", span)?;
                ClockRuntime::jet_clock_tick(
                    &mut clock,
                    crate::scheduler::jet_std_time_duration_to_millis(ns),
                )
            }
            _ => {
                return Err(mir_error_at(
                    "MIR Clock method has an invalid checked call",
                    span,
                ))
            }
        };
        Ok(RuntimeValue::Data(MirEvalValue::Int(now)))
    }

    fn eval_channel_constructor(
        &mut self,
        route_id: MirPreludeCallId,
        member: &str,
        args: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let route = self.prelude_row(route_id, span)?;
        let bounded = match member {
            member if member == crate::Syntax::INTERNAL_CHANNEL_NEW_METHOD => false,
            member if member == crate::Syntax::INTERNAL_CHANNEL_BOUNDED_METHOD => true,
            _ => {
                return Err(mir_error_at(
                    "MIR channel constructor has an unknown Core member",
                    span,
                ))
            }
        };
        let expected_symbol = if bounded {
            "jet_std::channel_bounded"
        } else {
            "jet_std::channel"
        };
        if route.family != jet_foundation::MIR::MirPreludeFamily::StaticPrelude
            || route.module != "core.tasks"
            || route.abi != jet_foundation::MIR::MirPreludeAbi::Value
            || route.symbol.name() != expected_symbol
        {
            return Err(mir_error_at(
                "MIR channel constructor has a non-canonical Prelude route",
                span,
            ));
        }
        let channel = match (bounded, args.as_slice()) {
            (false, []) => crate::scheduler::JetSchedulerChannel::<CtValue>::new(),
            (false, _) => {
                return Err(mir_error_at(
                    "MIR unbounded channel constructor received arguments",
                    span,
                ))
            }
            (true, [capacity]) => {
                let capacity =
                    runtime_to_data(self.materialize_runtime(capacity.clone(), span)?, span)?;
                let capacity = int_value(capacity, span)?;
                crate::scheduler::JetSchedulerChannel::<CtValue>::bounded(capacity)
            }
            (true, _) => {
                return Err(mir_error_at(
                    "MIR bounded channel constructor expects one capacity",
                    span,
                ))
            }
        };
        let fields = tuple_field_ids(self.program, result_ty, 2, span)?;
        let sender = channel.sender();
        Ok(RuntimeValue::Aggregate(vec![
            (
                fields[0],
                RuntimeValue::Ambient(mir_runtime_owner_value(MirChannelEndpoint::Sender(sender))),
            ),
            (
                fields[1],
                RuntimeValue::Ambient(mir_runtime_owner_value(MirChannelEndpoint::Receiver(
                    channel,
                ))),
            ),
        ]))
    }

    fn eval_channel_handle(
        &mut self,
        route: &MirPreludeCall,
        receiver: RuntimeValue,
        args: &[MirValueId],
        frame_index: usize,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let expected_symbol = match route.member.as_str() {
            "receiver.receive" => "jet_std::JetReceiver::receive",
            "receiver.close" => "jet_std::JetReceiver::close",
            "sender.send" => "jet_std::JetSender::send",
            "sender.close" => "jet_std::JetSender::close",
            _ => {
                return Err(mir_error_at(
                    "MIR channel method has an unknown member",
                    span,
                ))
            }
        };
        if route.family != jet_foundation::MIR::MirPreludeFamily::HandleMethod
            || route.module != "core.channels"
            || route.abi != jet_foundation::MIR::MirPreludeAbi::Value
            || route.symbol.name() != expected_symbol
        {
            return Err(mir_error_at(
                "MIR channel method has a non-canonical Prelude route",
                span,
            ));
        }
        let receiver = self.materialize_runtime(receiver, span)?;
        let receiver = self.runtime_to_ct(receiver, span)?;
        match route.member.as_str() {
            "receiver.receive" => {
                if !args.is_empty() {
                    return Err(mir_error_at(
                        "MIR receiver.receive expects no arguments",
                        span,
                    ));
                }
                let channel = mir_channel_receiver(&receiver).ok_or_else(|| {
                    mir_error_at("MIR receiver.receive receiver is not a Receiver", span)
                })?;
                match channel.receive() {
                    Some(value) => Ok(RuntimeValue::Result {
                        ok: true,
                        value: Box::new(runtime_from_ct(value, span)?),
                    }),
                    None => Ok(RuntimeValue::Result {
                        ok: false,
                        value: Box::new(RuntimeValue::Data(MirEvalValue::Enum {
                            type_name: "Closed".to_string(),
                            variant: "Closed".to_string(),
                            args: Vec::new(),
                        })),
                    }),
                }
            }
            "receiver.close" => {
                if !args.is_empty() {
                    return Err(mir_error_at(
                        "MIR receiver.close expects no arguments",
                        span,
                    ));
                }
                let channel = mir_channel_receiver(&receiver).ok_or_else(|| {
                    mir_error_at("MIR receiver.close receiver is not a Receiver", span)
                })?;
                channel.close();
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            "sender.send" => {
                let [value] = args else {
                    return Err(mir_error_at("MIR sender.send expects one payload", span));
                };
                let sender = mir_channel_sender(&receiver).ok_or_else(|| {
                    mir_error_at("MIR sender.send receiver is not a Sender", span)
                })?;
                let value = self.runtime_to_ct(self.value(frame_index, *value, span)?, span)?;
                sender.send(value);
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            "sender.close" => {
                if !args.is_empty() {
                    return Err(mir_error_at("MIR sender.close expects no arguments", span));
                }
                let sender = mir_channel_sender(&receiver).ok_or_else(|| {
                    mir_error_at("MIR sender.close receiver is not a Sender", span)
                })?;
                sender.close();
                Ok(RuntimeValue::Data(MirEvalValue::Unit))
            }
            _ => unreachable!("channel member validated above"),
        }
    }

    fn eval_channel_select(
        &mut self,
        call: MirPreludeCallId,
        values: Vec<RuntimeValue>,
        result_ty: Option<&MirType>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let route = self.prelude_row(call, span)?.clone();
        let blocking = match route.member.as_str() {
            "select_wait_tagged" => true,
            "select_try_wait_tagged" => false,
            _ => return self.eval_prelude_runtime_values(call, values, result_ty, span),
        };
        let expected_symbol = if blocking {
            "jet_std::jet_select_wait_tagged"
        } else {
            "jet_std::jet_select_try_wait_tagged"
        };
        if route.family != jet_foundation::MIR::MirPreludeFamily::StaticPrelude
            || route.module != "core.tasks"
            || route.abi != jet_foundation::MIR::MirPreludeAbi::Value
            || route.symbol.name() != expected_symbol
        {
            return Err(mir_error_at(
                "MIR channel select has a non-canonical Prelude route",
                span,
            ));
        }
        let [receiver_values, timer_values] = values.as_slice() else {
            return Err(mir_error_at(
                "MIR channel select expects receiver and timer lists",
                span,
            ));
        };
        let receiver_values = self.runtime_to_ct(receiver_values.clone(), span)?;
        let CtValue::List(receiver_values) = receiver_values else {
            return Err(mir_error_at(
                "MIR channel select receiver operand is not a List",
                span,
            ));
        };
        let timer_values = self.runtime_to_ct(timer_values.clone(), span)?;
        let CtValue::List(timer_values) = timer_values else {
            return Err(mir_error_at(
                "MIR channel select timer operand is not a List",
                span,
            ));
        };
        let channels = receiver_values
            .iter()
            .map(|value| {
                mir_channel_receiver(value)
                    .cloned()
                    .ok_or_else(|| mir_error_at("MIR select list contains a non-Receiver", span))
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let receiver_count = channels.len();
        let after_ms = timer_values
            .into_iter()
            .map(|value| mir_channel_timer_ms(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        if channels.is_empty() && after_ms.is_empty() {
            return Err(mir_error_at(
                "MIR channel select has no registered arms",
                span,
            ));
        }
        let recvs = channels
            .iter()
            .map(|channel| channel.select_inner())
            .collect();
        let outcome = if blocking {
            Some(crate::scheduler::jet_scheduler_select(recvs, after_ms))
        } else {
            crate::scheduler::jet_scheduler_try_select(recvs, after_ms)
        };
        let (arm, payload) = match outcome {
            Some(crate::scheduler::JetSelectOutcome::Recv { arm, value }) => (
                i64::try_from(arm)
                    .map_err(|_| mir_error_at("MIR select receive arm exceeds Int", span))?,
                Some(value),
            ),
            Some(crate::scheduler::JetSelectOutcome::After { arm }) => {
                let arm = receiver_count.checked_add(arm).ok_or_else(|| {
                    mir_error_at("MIR select timer arm exceeds the checked range", span)
                })?;
                (
                    i64::try_from(arm)
                        .map_err(|_| mir_error_at("MIR select timer arm exceeds Int", span))?,
                    None,
                )
            }
            Some(crate::scheduler::JetSelectOutcome::Closed) if blocking => {
                return Err(mir_error_at("MIR channel select closed", span))
            }
            Some(crate::scheduler::JetSelectOutcome::Closed) | None => (-1, None),
        };
        self.eval_channel_select_result(result_ty, arm, payload, span)
    }

    fn eval_channel_select_result(
        &self,
        result_ty: Option<&MirType>,
        arm: i64,
        payload: Option<CtValue>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let result_ty =
            result_ty.ok_or_else(|| mir_error_at("MIR channel select has no result type", span))?;
        let fields = result_ty
            .tuple_fields()
            .ok_or_else(|| mir_error_at("MIR channel select result is not a tuple", span))?;
        let option_element = fields
            .get(1)
            .and_then(|(_, ty)| ty.option_inner())
            .cloned()
            .ok_or_else(|| mir_error_at("MIR channel select result has no Option payload", span))?;
        let field_ids = tuple_field_ids(self.program, Some(result_ty), 2, span)?;
        let value = match payload {
            Some(value) => runtime_from_ct(CtValue::Present(Box::new(value)), span)?,
            None => RuntimeValue::Absent {
                element: option_element,
            },
        };
        Ok(RuntimeValue::Aggregate(vec![
            (field_ids[0], RuntimeValue::Data(MirEvalValue::Int(arm))),
            (field_ids[1], value),
        ]))
    }

    fn eval_core_call(
        &mut self,
        id: MirCoreCallId,
        route: MirPreludeCallId,
        args: Vec<RuntimeValue>,
        type_args: &[MirType],
        result_ty: Option<&MirType>,
        data_plan: Option<&MirDataPlan>,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let row = self
            .program
            .core_calls
            .iter()
            .find(|row| row.id == id)
            .cloned()
            .ok_or_else(|| mir_error_at("MIR CoreCallId has no canonical row", span))?;
        if args.len() < row.arity || args.len() > row.max_arity {
            return Err(mir_error_at(
                &format!(
                    "MIR CoreCallId {id:?} received {} arguments; expected {}..={}",
                    args.len(),
                    row.arity,
                    row.max_arity
                ),
                span,
            ));
        }
        if !self.is_runtime_invocation() {
            if jet_foundation::Effects::is_nondeterministic_core(&row.module, &row.member) {
                let api = format!(
                    "{}.{}",
                    row.module.rsplit('.').next().unwrap_or(&row.module),
                    row.member
                );
                return Err(Diagnostic::e3403(&api, Some(span)));
            }
            if jet_foundation::Effects::core_requires_comptime_gate(&row.module, &row.member) {
                return Err(Diagnostic::error(
                    "E3410",
                    format!(
                        "`{}.{}` is a Tier-2 comptime effect — it requires a `#Impure` gate",
                        row.module, row.member
                    ),
                    "effectful Core APIs are not allowed in pure comptime evaluation".to_string(),
                    "wrap the comptime binding in `#Impure(\"reason\") { … }` and pass `--gate impure=allow` to the build, or keep the call at runtime".to_string(),
                    Some(span),
                ));
            }
        }
        if row.module == "core.term" && row.member == "progress_iter" {
            self.ensure_core_route(id, route, span)?;
            let mut args = args.into_iter();
            let receiver = args
                .next()
                .ok_or_else(|| mir_error_at("MIR progress iterator has no source", span))?;
            let description = match runtime_to_data(
                args.next().ok_or_else(|| {
                    mir_error_at("MIR progress iterator has no description", span)
                })?,
                span,
            )? {
                MirEvalValue::String(value) => value,
                _ => {
                    return Err(mir_error_at(
                        "MIR progress description is not a String",
                        span,
                    ))
                }
            };
            let format = match runtime_to_data(
                args.next()
                    .ok_or_else(|| mir_error_at("MIR progress iterator has no format", span))?,
                span,
            )? {
                MirEvalValue::String(value) => value,
                _ => return Err(mir_error_at("MIR progress format is not a String", span)),
            };
            if args.next().is_some() {
                return Err(mir_error_at(
                    "MIR progress iterator received extra arguments",
                    span,
                ));
            }
            let source = match receiver {
                RuntimeValue::Stream(source) => source,
                RuntimeValue::Data(MirEvalValue::List(values)) => {
                    Rc::new(RefCell::new(MirInterpreterStream::Source {
                        values: values.into(),
                    }))
                }
                RuntimeValue::Moved => {
                    return Err(mir_error_at("MIR progress iterator source was moved", span));
                }
                _ => {
                    return Err(mir_error_at(
                        "MIR progress iterator requires a checked Iter source",
                        span,
                    ));
                }
            };
            let total = mir_stream_exact_len(&source);
            return Ok(RuntimeValue::Stream(Rc::new(RefCell::new(
                MirInterpreterStream::Progress {
                    source,
                    state: mir_human_output_semantics::JetOutputProgressIterState::new(
                        &description,
                        &format,
                        total,
                    ),
                },
            ))));
        }
        if crate::scheduler::jet_scheduler_current_world().is_some() {
            let controlled = match row.effect {
                Some(jet_foundation::Authority::Effect::Time)
                    if row.module == "core.time" && row.member != "start" =>
                {
                    true
                }
                Some(jet_foundation::Authority::Effect::Rand)
                    if row.module != "core.crypto.random" =>
                {
                    true
                }
                _ => row.effect.is_none(),
            };
            if !controlled {
                return Err(mir_error_at(
                    &format!(
                        "E3404: deterministic world has no controlled provider for `{}.{}`",
                        row.module, row.member
                    ),
                    span,
                ));
            }
        }
        if row.module == "core.tasks"
            && (row.member == crate::Syntax::INTERNAL_CHANNEL_NEW_METHOD
                || row.member == crate::Syntax::INTERNAL_CHANNEL_BOUNDED_METHOD)
        {
            self.ensure_core_route(id, route, span)?;
            return self.eval_channel_constructor(route, &row.member, args, result_ty, span);
        }
        if let Some(result) = self.eval_typed_core_call(&row, &args, type_args, span)? {
            return Ok(result);
        }
        if row.module == "core.compute"
            && matches!(
                row.member.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            )
        {
            self.ensure_core_route(id, route, span)?;
            let [function_type] = type_args else {
                return Err(mir_error_at(
                    "MIR autodiff call has no checked base function type",
                    span,
                ));
            };
            let result_type = result_ty.ok_or_else(|| {
                mir_error_at("MIR autodiff call has no checked result type", span)
            })?;
            let values = args
                .into_iter()
                .map(|value| self.runtime_to_ct(value, span))
                .collect::<Result<Vec<_>, _>>()?;
            let value = crate::Comptime::ComputeLite::autodiff_transform(
                &row.member,
                values,
                &crate::Comptime::MirBridge::mir_to_ast_type(function_type),
                &crate::Comptime::MirBridge::mir_to_ast_type(result_type),
                span,
            )?;
            return runtime_from_ct(value, span);
        }
        if row.module == "core.web" && row.member == "app" {
            if !args.is_empty() {
                return Err(mir_error_at("MIR App constructor received arguments", span));
            }
            self.ensure_core_route(id, route, span)?;
            return Ok(RuntimeValue::App(
                crate::Comptime::AppLite::app_new_runtime(),
            ));
        }
        if row.module == "core.testing" && row.member == "world" {
            self.ensure_core_route(id, route, span)?;
            let [callback] = args.as_slice() else {
                return Err(mir_error_at(
                    "MIR testing.world expects one checked callback",
                    span,
                ));
            };
            let (function, captures, capture_cells) = self.closure_parts(callback.clone(), span)?;
            return crate::Comptime::with_world_rng_provider(
                crate::scheduler::jet_scheduler_world_rng_next,
                crate::scheduler::jet_scheduler_world_rng_seed,
                || {
                    crate::scheduler::jet_testing_world(|world| {
                        let raw = register_deterministic_world(world);
                        let callback = RuntimeValue::ForeignHandle {
                            token: MirHandleToken::new(MIR_DETERMINISTIC_WORLD_HANDLE, raw),
                        };
                        let result = self.invoke_function_with_capture_cells(
                            function,
                            vec![callback],
                            captures,
                            capture_cells.clone(),
                            span,
                        );
                        unregister_deterministic_world(raw);
                        result
                    })
                },
            );
        }
        if row.module == "core.game" && row.member == "run" {
            self.ensure_core_route(id, route, span)?;
            return self.eval_game_run(args, span);
        }
        if row.module == "core.http" && row.member == "router" {
            if !args.is_empty() {
                return Err(mir_error_at(
                    "MIR HTTPRouter constructor received arguments",
                    span,
                ));
            }
            return self.eval_http_router_ambient(
                "http_router.new",
                None,
                Vec::new(),
                result_ty,
                span,
            );
        }
        if row.module == "core.http.server" && row.member == "mux" {
            if !args.is_empty() {
                return Err(mir_error_at(
                    "MIR HTTP mux constructor received arguments",
                    span,
                ));
            }
            return self.eval_http_router_ambient(
                "http_mux.new",
                None,
                Vec::new(),
                result_ty,
                span,
            );
        }
        if row.module == "core.net" && row.member == "tcp_listen" {
            let [address] = args.as_slice() else {
                return Err(mir_error_at("MIR TCP listener expects one address", span));
            };
            let address = match self.materialize_runtime(address.clone(), span)? {
                RuntimeValue::Data(MirEvalValue::String(address)) => address,
                _ => {
                    return Err(mir_error_at(
                        "MIR TCP listener address is not a String",
                        span,
                    ))
                }
            };
            let result = crate::Comptime::try_ambient_mir_handle(
                "tcp_listener.new",
                None,
                vec![MirEvalValue::String(address)],
                span,
            )
            .ok_or_else(|| {
                mir_error_at("MIR TCP listener has no interpreter host binding", span)
            })??;
            let crate::Comptime::AmbientMirHandleResult::Handle(raw) = result else {
                return Err(mir_error_at("MIR TCP listener host returned a value", span));
            };
            return Ok(RuntimeValue::Data(MirEvalValue::Struct {
                type_name: "TcpListener".to_string(),
                fields: vec![("id".to_string(), MirEvalValue::Int(raw))],
            }));
        }
        if row.module == "core.http.server" && row.member == "serve_once_listener" {
            let [listener, mux] = args.as_slice() else {
                return Err(mir_error_at(
                    "MIR HTTP serve_once_listener expects listener and mux",
                    span,
                ));
            };
            let listener = match self.materialize_runtime(listener.clone(), span)? {
                RuntimeValue::Data(MirEvalValue::Struct { fields, .. }) => fields
                    .iter()
                    .find_map(|(name, value)| (name == "id").then_some(value))
                    .and_then(|value| match value {
                        MirEvalValue::Int(raw) => Some(*raw),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        mir_error_at("MIR TCP listener carrier has no identity", span)
                    })?,
                _ => {
                    return Err(mir_error_at(
                        "MIR HTTP serve listener is not a listener carrier",
                        span,
                    ))
                }
            };
            let mux = match mux {
                RuntimeValue::ForeignHandle { token } => token
                    .raw()
                    .ok_or_else(|| mir_error_at("MIR HTTP mux handle was moved", span))?,
                _ => return Err(mir_error_at("MIR HTTP serve mux is not a mux handle", span)),
            };
            let invocation = crate::Comptime::try_ambient_mir_handle(
                "http_mux.serve_once",
                Some(listener),
                vec![MirEvalValue::Int(mux)],
                span,
            )
            .ok_or_else(|| {
                mir_error_at("MIR HTTP server has no interpreter host binding", span)
            })??;
            let crate::Comptime::AmbientMirHandleResult::Value(MirEvalValue::Struct {
                fields, ..
            }) = invocation
            else {
                return Err(mir_error_at(
                    "MIR HTTP server returned an invalid route invocation",
                    span,
                ));
            };
            let handler = fields
                .iter()
                .find_map(|(name, value)| (name == "handler").then_some(value.clone()))
                .ok_or_else(|| mir_error_at("MIR HTTP invocation has no handler", span))?;
            let call_args = fields
                .iter()
                .find_map(|(name, value)| (name == "args").then_some(value.clone()))
                .and_then(|value| match value {
                    MirEvalValue::List(args) => Some(args),
                    _ => None,
                })
                .ok_or_else(|| mir_error_at("MIR HTTP invocation has no arguments", span))?;
            let stream = fields
                .iter()
                .find_map(|(name, value)| (name == "__stream").then_some(value.clone()))
                .and_then(|value| match value {
                    MirEvalValue::Int(raw) => Some(raw),
                    _ => None,
                })
                .ok_or_else(|| mir_error_at("MIR HTTP invocation has no stream", span))?;
            let handler = mir_http_handler_value(&handler).unwrap_or(RuntimeValue::Data(handler));
            let (function, captures, capture_cells) = self.closure_parts(handler, span)?;
            let result = self.invoke_function_with_capture_cells(
                function,
                call_args.into_iter().map(RuntimeValue::Data).collect(),
                captures,
                capture_cells,
                span,
            )?;
            let response = match result {
                RuntimeValue::Data(MirEvalValue::Present(value)) => *value,
                RuntimeValue::Data(value) => value,
                _ => MirEvalValue::Struct {
                    type_name: "HTTPResponse".to_string(),
                    fields: vec![
                        ("status".to_string(), MirEvalValue::Int(500)),
                        (
                            "body".to_string(),
                            MirEvalValue::Bytes(b"500 internal server error".to_vec()),
                        ),
                    ],
                },
            };
            let _ = crate::Comptime::try_ambient_mir_handle(
                "http_mux.respond",
                Some(stream),
                vec![response],
                span,
            )
            .ok_or_else(|| mir_error_at("MIR HTTP server response host is unavailable", span))??;
            return Ok(RuntimeValue::Data(MirEvalValue::Unit));
        }
        if row.module == "core.http" && row.member == "parse" {
            let [raw] = args.as_slice() else {
                return Err(mir_error_at(
                    "MIR HTTP request parsing requires one raw request string",
                    span,
                ));
            };
            let raw = match self.materialize_runtime(raw.clone(), span)? {
                RuntimeValue::Data(MirEvalValue::String(raw)) => raw,
                _ => {
                    return Err(mir_error_at(
                        "MIR HTTP request parsing expects a checked String",
                        span,
                    ))
                }
            };
            return self.eval_http_router_ambient(
                "http_router.parse",
                None,
                vec![RuntimeValue::Data(MirEvalValue::String(raw))],
                result_ty,
                span,
            );
        }
        if row.module == "core.http" && row.member == "dispatch" {
            let [receiver, request] = args.as_slice() else {
                return Err(mir_error_at(
                    "MIR HTTPRouter dispatch requires a router and request",
                    span,
                ));
            };
            return self.eval_http_router_dispatch(receiver.clone(), request.clone(), span);
        }
        if row.module == "core.web" && row.member == "openapi" {
            let [receiver] = args.as_slice() else {
                return Err(mir_error_at(
                    "MIR HTTPRouter OpenAPI export requires one router receiver",
                    span,
                ));
            };
            return self.eval_http_router_ambient(
                "http_router.openapi",
                Some(receiver.clone()),
                Vec::new(),
                None,
                span,
            );
        }
        if row.module == "core.process" && row.member == "cmd" {
            let [value] = args.as_slice() else {
                return Err(mir_error_at(
                    "MIR process.cmd requires one checked command-list argument",
                    span,
                ));
            };
            let Some(result_ty) = result_ty else {
                return Err(mir_error_at(
                    "MIR process.cmd has no checked ProcessSpec result type",
                    span,
                ));
            };
            if result_ty.nominal_name() != Some("ProcessSpec") {
                return Err(mir_error_at(
                    "MIR process.cmd result is not the checked ProcessSpec type",
                    span,
                ));
            }
            let value = runtime_to_data(value.clone(), span)?;
            let result =
                crate::Comptime::try_ambient_mir_handle("process.cmd", None, vec![value], span)
                    .ok_or_else(|| {
                    mir_error_at(
                        "MIR process.cmd has no interpreter ambient host binding",
                        span,
                    )
                })??;
            return match result {
                crate::Comptime::AmbientMirHandleResult::Handle(raw) => {
                    if raw == 0 {
                        return Err(mir_error_at(
                            "MIR ProcessSpec ambient binding returned an invalid identity",
                            span,
                        ));
                    }
                    Ok(RuntimeValue::ProcessSpec { raw })
                }
                crate::Comptime::AmbientMirHandleResult::Value(_) => Err(mir_error_at(
                    "MIR process.cmd adapter returned a value instead of a ProcessSpec carrier",
                    span,
                )),
            };
        }
        if row.module == "core.service" && row.member == "worker" && args.len() == 5 {
            let handler_name = match self.materialize_runtime(args[3].clone(), span)? {
                RuntimeValue::Data(MirEvalValue::String(name)) => name,
                _ => {
                    return Err(mir_error_at(
                        "MIR service worker handler identity is not a checked String",
                        span,
                    ))
                }
            };
            self.service_callbacks
                .borrow_mut()
                .insert(handler_name, args[2].clone());
        }
        if row.module == "core.service" && row.member == "start" {
            self.ensure_core_route(id, route, span)?;
            let [receiver] = args.as_slice() else {
                return Err(mir_error_at(
                    "MIR service.start requires one checked ServiceTree receiver",
                    span,
                ));
            };
            let receiver = self.materialize_runtime(receiver.clone(), span)?;
            let receiver = runtime_to_data(receiver, span)?;
            let result = self.eval_service_start(receiver, span)?;
            return Ok(RuntimeValue::Data(result));
        }
        self.prelude_row(route, span)?;
        let mut runtime_args = args
            .into_iter()
            .map(|value| self.materialize_runtime(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        if row.module.is_empty() && row.member == "attach" {
            let [payload_ty] = type_args else {
                return Err(mir_error_at(
                    "MIR receipt attach requires one checked payload type argument",
                    span,
                ));
            };
            let payload = runtime_to_data(
                runtime_args
                    .first()
                    .cloned()
                    .ok_or_else(|| mir_error_at("MIR receipt attach has no payload value", span))?,
                span,
            )?;
            let encoded =
                super::Receipt::encode_interpreter_value(self.program, &payload, payload_ty)
                    .map_err(|error| mir_error_at(&format!("receipt.attach: {error}"), span))?;
            if let Some(first) = runtime_args.first_mut() {
                *first = RuntimeValue::Data(encoded);
            }
        }
        let values = runtime_args
            .into_iter()
            .map(|value| self.runtime_to_ct(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let ast_type_args = type_args
            .iter()
            .map(crate::Comptime::MirBridge::mir_to_ast_type)
            .collect::<Vec<_>>();
        let resolved_ret = result_ty.map(crate::Comptime::MirBridge::mir_to_ast_type);
        let history_schema = if row.module == "core.testing" && row.member == "histories" {
            let provenance =
                history_provenance_for_execution(self.program, self.execution.as_ref());
            type_args.first().and_then(|command_type| {
                crate::Comptime::history_command_schema_from_mir(
                    self.program,
                    command_type,
                    provenance,
                )
            })
        } else {
            None
        };
        let runtime = self.is_runtime_invocation();
        let mut sink = if runtime {
            Some(crate::Comptime::DevSink::default())
        } else {
            None
        };
        let result = eval_core_call_binding(
            self.data_pipeline.as_deref_mut(),
            &row.module,
            &row.member,
            values,
            &ast_type_args,
            resolved_ret.as_ref(),
            data_plan,
            span,
            runtime,
            &self.config.base_dir,
            sink.as_mut(),
            history_schema.as_ref(),
        );
        self.merge_runtime_sink(sink);
        let result = result?;
        runtime_from_ct(result, span)
    }

    fn resolve_callee(
        &mut self,
        frame_index: usize,
        callee: &MirCallee,
        values: &[RuntimeValue],
        span: Span,
    ) -> Result<
        (
            MirFunctionId,
            Vec<RuntimeValue>,
            Vec<Option<Rc<RefCell<RuntimeValue>>>>,
        ),
        Diagnostic,
    > {
        match callee {
            MirCallee::User(function) => {
                let _ = program_function(self.program, *function)?;
                Ok((*function, Vec::new(), Vec::new()))
            }
            MirCallee::Associated { function, .. } | MirCallee::Method { function, .. } => {
                let _ = program_function(self.program, *function)?;
                Ok((*function, Vec::new(), Vec::new()))
            }
            MirCallee::TraitMethod {
                method,
                trait_ref,
                receiver,
            } => {
                let function =
                    self.resolve_trait_method(values, *method, trait_ref, receiver, span)?;
                Ok((function, Vec::new(), Vec::new()))
            }
            MirCallee::Indirect(value) => {
                let value = self.value(frame_index, *value, span)?;
                self.closure_parts(value, span)
            }
            MirCallee::Core(_) => Err(mir_error_at("MIR Core callee must use CoreCall", span)),
            MirCallee::Prelude(_) => Err(mir_error_at(
                "MIR Prelude callee must use direct Prelude dispatch",
                span,
            )),
            MirCallee::Foreign(_) => Err(mir_error_at(
                "MIR foreign callee must use direct foreign dispatch",
                span,
            )),
        }
    }

    fn resolve_trait_method(
        &mut self,
        values: &[RuntimeValue],
        method_id: MirTraitMethodId,
        trait_ref: &MirTraitRef,
        _receiver: &MirType,
        span: Span,
    ) -> Result<MirFunctionId, Diagnostic> {
        let receiver = values
            .first()
            .cloned()
            .ok_or_else(|| mir_error_at("MIR trait method call has no receiver", span))?;
        let receiver = match receiver {
            RuntimeValue::Address(address) => {
                require_address_access(&address, MirAccess::Read, span)?;
                self.read_place(address.frame, address.place, span)?
            }
            value => value,
        };
        let concrete_name = match runtime_to_data(receiver, span)? {
            MirEvalValue::Struct { type_name, .. } | MirEvalValue::Enum { type_name, .. } => {
                type_name
            }
            _ => {
                return Err(mir_error_at(
                    "MIR trait method receiver is not a nominal value",
                    span,
                ))
            }
        };
        let mut concrete_types = self.program.type_instances.iter().filter(|ty| {
            matches!(ty.kind(), MirTypeKind::Apply { .. })
                && mir_type_display_name(self.program, ty) == concrete_name
        });
        let concrete_type = concrete_types.next().ok_or_else(|| {
            mir_error_at(
                &format!(
                    "MIR trait method receiver `{concrete_name}` has no canonical nominal type"
                ),
                span,
            )
        })?;
        if concrete_types.next().is_some() {
            return Err(mir_error_at(
                &format!(
                    "MIR trait method receiver `{concrete_name}` has an ambiguous canonical nominal type"
                ),
                span,
            ));
        }
        let trait_row = self
            .program
            .traits
            .iter()
            .find(|row| row.id == trait_ref.id)
            .ok_or_else(|| mir_error_at("MIR trait method references a missing trait", span))?;
        let method_row = trait_row
            .methods
            .iter()
            .find(|method| method.id == method_id)
            .ok_or_else(|| mir_error_at("MIR trait method references a missing method", span))?;
        let mut candidates = Vec::new();
        for impl_row in &self.program.impls {
            let Some(impl_trait) = impl_row.trait_ref.as_ref() else {
                continue;
            };
            if impl_trait.id != trait_ref.id
                || !impl_row.self_type.same_checked_type(concrete_type)
            {
                continue;
            }
            for function_id in &impl_row.methods {
                let function = program_function(self.program, *function_id)?;
                if function.name != method_row.name {
                    continue;
                }
                if matches!(
                    &function.form,
                    MirFunctionForm::TraitMethod {
                        trait_ref: function_trait,
                        ..
                    } if function_trait.id == trait_ref.id
                ) {
                    candidates.push(*function_id);
                }
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        match candidates.as_slice() {
            [function] => Ok(*function),
            [] => {
                let default = method_row.default.ok_or_else(|| {
                    mir_error_at(
                        &format!(
                            "MIR trait method `{}` has no implementation for `{}`",
                            method_row.name, concrete_name
                        ),
                        span,
                    )
                })?;
                let _ = program_function(self.program, default)?;
                Ok(default)
            }
            _ => Err(mir_error_at(
                &format!(
                    "MIR trait method `{}` has ambiguous implementations for `{}`",
                    method_row.name, concrete_name
                ),
                span,
            )),
        }
    }

    fn call_args(
        &mut self,
        frame_index: usize,
        args: &[jet_foundation::MIR::MirCallArg],
        span: Span,
    ) -> Result<Vec<RuntimeValue>, Diagnostic> {
        let mut values = Vec::new();
        for arg in args {
            let value = if arg.access == MirAccess::Write {
                let Some(place) = arg.place else {
                    return Err(mir_error_at(
                        "MIR write call argument has no checked place",
                        arg.span,
                    ));
                };
                RuntimeValue::Address(Address {
                    frame: frame_index,
                    place,
                    access: MirAccess::Write,
                })
            } else {
                self.value(frame_index, arg.value, arg.span)?
            };
            let value = match arg.access {
                MirAccess::Read | MirAccess::Write => value,
                MirAccess::Move => {
                    let _ = self.frames[frame_index].values.remove(&arg.value);
                    value
                }
            };
            let value = self.marshal_call_arg(value, arg, span)?;
            if arg.spread {
                match value {
                    RuntimeValue::Data(MirEvalValue::List(items)) => {
                        values.extend(items.into_iter().map(RuntimeValue::Data));
                    }
                    _ => return Err(mir_error_at("MIR spread argument is not a List", span)),
                }
            } else {
                values.push(value);
            }
        }
        Ok(values)
    }

    fn marshal_call_arg(
        &self,
        mut value: RuntimeValue,
        arg: &jet_foundation::MIR::MirCallArg,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if let Some(coercion) = &arg.fn_coercion {
            let callable = matches!(
                &value,
                RuntimeValue::Closure(_) | RuntimeValue::Data(MirEvalValue::Closure(_))
            );
            if !callable {
                return Err(mir_error_at(
                    "MIR function coercion requires a callable value",
                    arg.span,
                ));
            }
            let _ = self.closure_handle(value.clone(), arg.span)?;
            let _ = (&coercion.ty, coercion.already_boxed);
        }
        if arg.widen_fixed_to_list && !matches!(value, RuntimeValue::Data(MirEvalValue::List(_))) {
            return Err(mir_error_at(
                "MIR fixed-list widening requires a List value",
                arg.span,
            ));
        }
        if let Some(coercion) = &arg.widen_to_union {
            let type_name = type_instance_name(self.program, coercion.union, arg.span)?;
            let data = runtime_to_data(value, arg.span)?;
            let data = match data {
                MirEvalValue::Enum {
                    type_name: existing_name,
                    variant: existing_variant,
                    args,
                } if existing_name == type_name
                    && variant_name_matches(&existing_variant, &coercion.variant) =>
                {
                    MirEvalValue::Enum {
                        type_name: existing_name,
                        variant: existing_variant,
                        args,
                    }
                }
                data => MirEvalValue::Enum {
                    type_name,
                    variant: coercion.variant.clone(),
                    args: vec![(None, data)],
                },
            };
            value = RuntimeValue::Data(data);
        }
        if let Some(trait_id) = arg.box_as_trait {
            let target = self
                .program
                .type_instances
                .iter()
                .find(|instance| instance.identity == Some(trait_id))
                .ok_or_else(|| {
                    mir_error_at(
                        "MIR trait coercion target has no canonical instance row",
                        arg.span,
                    )
                })?;
            if !matches!(target.kind(), MirTypeKind::TraitObject(_)) {
                return Err(mir_error_at(
                    "MIR trait coercion target is not a trait object",
                    arg.span,
                ));
            }
        }
        let _ = span;
        Ok(value)
    }

    fn place_data_mut(
        &mut self,
        frame_index: usize,
        place_id: MirPlaceId,
        span: Span,
    ) -> Result<Option<&mut MirEvalValue>, Diagnostic> {
        let function_id = self.frames[frame_index].function;
        let place = program_function(self.program, function_id)?
            .places
            .iter()
            .find(|place| place.id == place_id)
            .ok_or_else(|| mir_error_at("MIR place is unavailable", span))?
            .clone();
        if !place.projections.is_empty() {
            return Ok(None);
        }
        if matches!(
            place.base,
            MirPlaceBase::Capture(_) | MirPlaceBase::Static(_)
        ) {
            return Ok(None);
        }
        if let Some(value) = self.frames[frame_index].place_overrides.remove(&place_id) {
            self.store_base_value(frame_index, &place.base, value, span)?;
        } else {
            self.invalidate_base_overrides(frame_index, &place.base)?;
        }
        let frame = &mut self.frames[frame_index];
        let value = match place.base {
            MirPlaceBase::Local(local) => frame.locals.get_mut(&local),
            MirPlaceBase::Parameter(value) | MirPlaceBase::Temporary(value) => {
                frame.values.get_mut(&value)
            }
            MirPlaceBase::Capture(_) | MirPlaceBase::Static(_) => None,
        };
        Ok(match value {
            Some(RuntimeValue::Data(data)) => Some(data),
            _ => None,
        })
    }

    fn value(
        &self,
        frame_index: usize,
        value: MirValueId,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let value = self.frames[frame_index]
            .values
            .get(&value)
            .cloned()
            .ok_or_else(|| mir_error_at("MIR value is unavailable at its use", span))?;
        if runtime_contains_moved(&value) {
            return Err(mir_error_at("MIR value was moved", span));
        }
        Ok(value)
    }
    fn capture_slot_for_value(&self, frame_index: usize, value: MirValueId) -> Option<usize> {
        let function = self
            .program
            .functions
            .iter()
            .find(|function| function.id == self.frames[frame_index].function)?;
        function.blocks.iter().find_map(|block| {
            block.instructions.iter().find_map(|instruction| {
                (instruction.result == Some(value)).then(|| match &instruction.operation {
                    MirOperation::Capture { slot } => Some(*slot),
                    _ => None,
                })?
            })
        })
    }

    fn capture_cell_for_value(
        &self,
        frame_index: usize,
        value: MirValueId,
    ) -> Option<Rc<RefCell<RuntimeValue>>> {
        let slot = self.capture_slot_for_value(frame_index, value)?;
        self.frames[frame_index]
            .capture_cells
            .get(slot)
            .and_then(|cell| cell.clone())
    }

    fn bool_value(
        &self,
        frame_index: usize,
        value: MirValueId,
        span: Span,
    ) -> Result<bool, Diagnostic> {
        match runtime_to_data(self.value(frame_index, value, span)?, span)? {
            MirEvalValue::Bool(value) => Ok(value),
            _ => Err(mir_error_at("MIR branch condition is not Bool", span)),
        }
    }

    fn jump(
        &mut self,
        frame_index: usize,
        target: MirBlockId,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        if !function.blocks.iter().any(|block| block.id == target) {
            return Err(mir_error_at("MIR branch targets a missing block", span));
        }
        let predecessor = self.frames[frame_index].block;
        self.frames[frame_index].predecessor = Some(predecessor);
        self.frames[frame_index].block = target;
        self.frames[frame_index].ip = 0;
        Ok(())
    }

    fn read_static_value(&self, name: &str, span: Span) -> Result<RuntimeValue, Diagnostic> {
        if let Some(value) = self.static_values.borrow().get(name).cloned() {
            if matches!(value, RuntimeValue::Moved) {
                return Err(mir_error_at("MIR static value was moved", span));
            }
            return Ok(value);
        }
        let value = self
            .config
            .globals
            .get(name)
            .cloned()
            .map(RuntimeValue::Data)
            .ok_or_else(|| mir_error_at(&format!("MIR static `{name}` is not provided"), span))?;
        if matches!(value, RuntimeValue::Moved) {
            return Err(mir_error_at("MIR static value was moved", span));
        }
        self.static_values
            .borrow_mut()
            .insert(name.to_string(), value.clone());
        Ok(value)
    }

    fn take_static_value(&self, name: &str, span: Span) -> Result<RuntimeValue, Diagnostic> {
        let value = self.static_values.borrow_mut().remove(name).or_else(|| {
            self.config
                .globals
                .get(name)
                .cloned()
                .map(RuntimeValue::Data)
        });
        value.ok_or_else(|| mir_error_at(&format!("MIR static `{name}` is not provided"), span))
    }

    fn store_static_value(&self, name: &str, value: RuntimeValue) {
        self.static_values
            .borrow_mut()
            .insert(name.to_string(), value);
    }

    fn take_base_value(
        &mut self,
        frame_index: usize,
        base: &MirPlaceBase,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match base {
            MirPlaceBase::Local(local) => self.frames[frame_index]
                .locals
                .remove(local)
                .ok_or_else(|| mir_error_at("MIR move place base is unavailable", span)),
            MirPlaceBase::Parameter(source)
            | MirPlaceBase::Temporary(source)
            | MirPlaceBase::Capture(source) => self.frames[frame_index]
                .values
                .remove(source)
                .ok_or_else(|| mir_error_at("MIR move place base is unavailable", span)),
            MirPlaceBase::Static(name) => self.take_static_value(name, span),
        }
    }

    fn invalidate_base_overrides(
        &mut self,
        frame_index: usize,
        base: &MirPlaceBase,
    ) -> Result<(), Diagnostic> {
        if self.frames[frame_index].place_overrides.is_empty() {
            return Ok(());
        }
        let function = program_function(self.program, self.frames[frame_index].function)?;
        let overrides = &mut self.frames[frame_index].place_overrides;
        for place in &function.places {
            if &place.base == base {
                overrides.remove(&place.id);
            }
        }
        Ok(())
    }

    fn store_base_value(
        &mut self,
        frame_index: usize,
        base: &MirPlaceBase,
        value: RuntimeValue,
        _span: Span,
    ) -> Result<(), Diagnostic> {
        self.invalidate_base_overrides(frame_index, base)?;
        match base {
            MirPlaceBase::Local(local) => {
                self.frames[frame_index].locals.insert(*local, value);
                Ok(())
            }
            MirPlaceBase::Parameter(source) | MirPlaceBase::Temporary(source) => {
                self.frames[frame_index].values.insert(*source, value);
                Ok(())
            }
            MirPlaceBase::Capture(source) => {
                if let Some(cell) = self.capture_cell_for_value(frame_index, *source) {
                    *cell.borrow_mut() = value.clone();
                }
                self.frames[frame_index].values.insert(*source, value);
                Ok(())
            }
            MirPlaceBase::Static(name) => {
                self.store_static_value(name, value);
                Ok(())
            }
        }
    }
    fn move_place_path(
        &mut self,
        frame_index: usize,
        place_id: jet_foundation::MIR::MirPlaceId,
        extra_steps: &[MoveStep],
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        let place = function
            .places
            .iter()
            .find(|place| place.id == place_id)
            .ok_or_else(|| mir_error_at("MIR move target place is unavailable", span))?
            .clone();
        let override_value = self.frames[frame_index].place_overrides.remove(&place_id);
        if override_value
            .as_ref()
            .is_some_and(|value| matches!(value, RuntimeValue::Moved))
        {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        if let &MirPlaceBase::Capture(source) = &place.base {
            if let Some(RuntimeValue::Address(address)) =
                self.frames[frame_index].values.get(&source).cloned()
            {
                require_address_access(&address, MirAccess::Move, span)?;
                let _ = self.take_base_value(frame_index, &place.base, span)?;
                self.store_base_value(frame_index, &place.base, RuntimeValue::Moved, span)?;
                let mut steps = self.move_steps_for_frame(frame_index, &place.projections, span)?;
                steps.extend_from_slice(extra_steps);
                return if steps.is_empty() {
                    self.move_place(address.frame, address.place, span)
                } else {
                    self.move_place_path(address.frame, address.place, &steps, span)
                };
            }
        }
        let base = self.take_base_value(frame_index, &place.base, span)?;
        if matches!(base, RuntimeValue::Moved) {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        let mut steps = self.move_steps_for_frame(frame_index, &place.projections, span)?;
        steps.extend_from_slice(extra_steps);
        let (updated, moved) = self.take_runtime_steps(base, &steps, span)?;
        self.store_base_value(frame_index, &place.base, updated, span)?;
        Ok(moved)
    }

    fn move_place(
        &mut self,
        frame_index: usize,
        place_id: jet_foundation::MIR::MirPlaceId,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        let place = function
            .places
            .iter()
            .find(|place| place.id == place_id)
            .ok_or_else(|| mir_error_at("MIR move targets an unavailable place", span))?
            .clone();
        let override_value = self.frames[frame_index].place_overrides.remove(&place_id);
        if override_value
            .as_ref()
            .is_some_and(|value| matches!(value, RuntimeValue::Moved))
        {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        if let &MirPlaceBase::Capture(source) = &place.base {
            let captured = self.frames[frame_index].values.get(&source).cloned();
            if let Some(RuntimeValue::Address(address)) = captured {
                require_address_access(&address, MirAccess::Move, span)?;
                let _ = self.take_base_value(frame_index, &place.base, span)?;
                self.store_base_value(frame_index, &place.base, RuntimeValue::Moved, span)?;
                let steps = self.move_steps_for_frame(frame_index, &place.projections, span)?;
                self.frames[frame_index]
                    .place_overrides
                    .insert(place_id, RuntimeValue::Moved);
                return if steps.is_empty() {
                    self.move_place(address.frame, address.place, span)
                } else {
                    self.move_place_path(address.frame, address.place, &steps, span)
                };
            }
        }
        let base = self.take_base_value(frame_index, &place.base, span)?;
        if matches!(base, RuntimeValue::Moved) {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        if place.projections.is_empty() && runtime_contains_moved(&base) {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        if place.projections.is_empty() {
            self.store_base_value(frame_index, &place.base, RuntimeValue::Moved, span)?;
            self.frames[frame_index]
                .place_overrides
                .insert(place_id, RuntimeValue::Moved);
            return Ok(override_value.unwrap_or(base));
        }
        let steps = self.move_steps_for_frame(frame_index, &place.projections, span)?;
        let (updated, moved) = self.take_runtime_steps(base, &steps, span)?;
        self.store_base_value(frame_index, &place.base, updated, span)?;
        self.frames[frame_index]
            .place_overrides
            .insert(place_id, RuntimeValue::Moved);
        Ok(override_value.unwrap_or(moved))
    }

    fn move_steps_for_frame(
        &self,
        frame_index: usize,
        projections: &[MirProjection],
        span: Span,
    ) -> Result<Vec<MoveStep>, Diagnostic> {
        projections
            .iter()
            .map(|projection| match projection {
                MirProjection::Field { field, .. } => {
                    let name = field_name(self.program, *field)
                        .ok_or_else(|| mir_error_at("MIR field ID is missing", span))?;
                    Ok(MoveStep::Field {
                        field: *field,
                        name,
                    })
                }
                MirProjection::Index { index, kind, .. } => Ok(MoveStep::Index {
                    value: runtime_to_data(self.value(frame_index, *index, span)?, span)?,
                    kind: *kind,
                }),
                MirProjection::Deref { .. } => Ok(MoveStep::Deref),
            })
            .collect()
    }

    fn take_runtime_steps(
        &mut self,
        value: RuntimeValue,
        steps: &[MoveStep],
        span: Span,
    ) -> Result<(RuntimeValue, RuntimeValue), Diagnostic> {
        if steps.is_empty() {
            if runtime_contains_moved(&value) {
                return Err(mir_error_at("MIR place value was moved", span));
            }
            return Ok((RuntimeValue::Moved, value));
        }
        let (step, rest) = steps
            .split_first()
            .ok_or_else(|| mir_error_at("MIR move projection is empty", span))?;
        match step {
            MoveStep::Field { field, name } => match value {
                RuntimeValue::Aggregate(mut fields) => {
                    let field_index = fields
                        .iter()
                        .position(|(candidate, _)| candidate == field)
                        .ok_or_else(|| {
                            mir_error_at("MIR aggregate field ID is not present", span)
                        })?;
                    if matches!(&fields[field_index].1, RuntimeValue::Moved) {
                        return Err(mir_error_at("MIR place value was moved", span));
                    }
                    let child = std::mem::replace(&mut fields[field_index].1, RuntimeValue::Moved);
                    let (replacement, moved) = if rest.is_empty() {
                        if runtime_contains_moved(&child) {
                            return Err(mir_error_at("MIR place value was moved", span));
                        }
                        (RuntimeValue::Moved, child)
                    } else {
                        self.take_runtime_steps(child, rest, span)?
                    };
                    fields[field_index].1 = replacement;
                    Ok((RuntimeValue::Aggregate(fields), moved))
                }
                RuntimeValue::Data(data) => {
                    let (updated, moved) = take_data_steps(data, steps, span)?;
                    Ok((RuntimeValue::Data(updated), RuntimeValue::Data(moved)))
                }
                RuntimeValue::Moved => Err(mir_error_at("MIR place value was moved", span)),
                RuntimeValue::Result { .. }
                | RuntimeValue::Stream(_)
                | RuntimeValue::StreamCursor(_)
                | RuntimeValue::Address(_)
                | RuntimeValue::Absent { .. }
                | RuntimeValue::Closure(_)
                | RuntimeValue::App(_)
                | RuntimeValue::Ambient(_)
                | RuntimeValue::Atomic(_)
                | RuntimeValue::ForeignHandle { .. }
                | RuntimeValue::ProcessSpec { .. }
                | RuntimeValue::DmaTransfer { .. }
                | RuntimeValue::SharedGuard(_)
                | RuntimeValue::SharedCell(_)
                | RuntimeValue::SharedSnapshot(_)
                | RuntimeValue::SharedTransaction(_)
                | RuntimeValue::CellGuard(_)
                | RuntimeValue::GcRoot(_)
                | RuntimeValue::Game(_) => Err(mir_error_at(
                    &format!("MIR field `{name}` cannot be moved from this value"),
                    span,
                )),
            },
            MoveStep::Index { value: _, kind } => match value {
                RuntimeValue::Data(data) => {
                    let (updated, moved) = take_data_steps(data, steps, span)?;
                    Ok((RuntimeValue::Data(updated), RuntimeValue::Data(moved)))
                }
                RuntimeValue::Moved => Err(mir_error_at("MIR place value was moved", span)),
                RuntimeValue::Result { .. }
                | RuntimeValue::Aggregate(_)
                | RuntimeValue::Stream(_)
                | RuntimeValue::StreamCursor(_)
                | RuntimeValue::Address(_)
                | RuntimeValue::Absent { .. }
                | RuntimeValue::Closure(_)
                | RuntimeValue::App(_)
                | RuntimeValue::Ambient(_)
                | RuntimeValue::Atomic(_)
                | RuntimeValue::ForeignHandle { .. }
                | RuntimeValue::ProcessSpec { .. }
                | RuntimeValue::DmaTransfer { .. }
                | RuntimeValue::SharedGuard(_)
                | RuntimeValue::SharedCell(_)
                | RuntimeValue::SharedSnapshot(_)
                | RuntimeValue::SharedTransaction(_)
                | RuntimeValue::CellGuard(_)
                | RuntimeValue::GcRoot(_)
                | RuntimeValue::Game(_) => Err(mir_error_at(
                    &format!("MIR {kind:?} index cannot be moved from this value"),
                    span,
                )),
            },
            MoveStep::Deref => match value {
                RuntimeValue::Address(address) => {
                    require_address_access(&address, MirAccess::Move, span)?;
                    let moved = if rest.is_empty() {
                        self.move_place(address.frame, address.place, span)?
                    } else {
                        self.move_place_path(address.frame, address.place, rest, span)?
                    };
                    Ok((RuntimeValue::Address(address), moved))
                }
                RuntimeValue::Moved => Err(mir_error_at("MIR place value was moved", span)),
                RuntimeValue::SharedGuard(guard) if rest.is_empty() => {
                    let value = self.deref_shared_guard(guard, span)?;
                    if runtime_contains_moved(&value) {
                        return Err(mir_error_at("MIR place value was moved", span));
                    }
                    Ok((RuntimeValue::Moved, value))
                }
                RuntimeValue::SharedCell(cell) if rest.is_empty() => {
                    if let Some(storage) = &cell.guard.storage {
                        storage.refresh_scalar_shadow(span)?;
                    }
                    let value = RuntimeValue::Data(shared_payload_read(
                        &cell.guard.payload,
                        &cell.path,
                        span,
                    )?);
                    Ok((RuntimeValue::Moved, value))
                }
                RuntimeValue::CellGuard(guard) if rest.is_empty() => {
                    let value = RuntimeValue::Data(cell_payload_read(
                        self.program,
                        &guard.payload,
                        &guard.path,
                        span,
                    )?);
                    Ok((RuntimeValue::Moved, value))
                }
                RuntimeValue::SharedGuard(_)
                | RuntimeValue::Result { .. }
                | RuntimeValue::SharedCell(_)
                | RuntimeValue::SharedSnapshot(_)
                | RuntimeValue::SharedTransaction(_)
                | RuntimeValue::CellGuard(_)
                | RuntimeValue::Data(_)
                | RuntimeValue::Atomic(_)
                | RuntimeValue::Stream(_)
                | RuntimeValue::StreamCursor(_)
                | RuntimeValue::Absent { .. }
                | RuntimeValue::Closure(_)
                | RuntimeValue::App(_)
                | RuntimeValue::Ambient(_)
                | RuntimeValue::ForeignHandle { .. }
                | RuntimeValue::ProcessSpec { .. }
                | RuntimeValue::DmaTransfer { .. }
                | RuntimeValue::Aggregate(_)
                | RuntimeValue::GcRoot(_)
                | RuntimeValue::Game(_) => Err(mir_error_at(
                    "MIR place dereference move requires an address or guard",
                    span,
                )),
            },
        }
    }

    fn read_place(
        &mut self,
        frame_index: usize,
        place_id: jet_foundation::MIR::MirPlaceId,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if let Some(value) = self.frames[frame_index].place_overrides.get(&place_id) {
            if runtime_contains_moved(value) {
                return Err(mir_error_at("MIR place value was moved", span));
            }
            return Ok(value.clone());
        }
        let function_id = self.frames[frame_index].function;
        let function = program_function(self.program, function_id)?;
        let place = function
            .places
            .iter()
            .find(|place| place.id == place_id)
            .ok_or_else(|| mir_error_at("MIR place is unavailable", span))?
            .clone();
        let mut value = self.read_base(frame_index, &place.base, span)?;
        for projection in &place.projections {
            value = self.project(frame_index, value, projection, span)?;
        }
        if runtime_contains_moved(&value) {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        Ok(value)
    }

    fn read_base(
        &mut self,
        frame_index: usize,
        base: &MirPlaceBase,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match base {
            MirPlaceBase::Local(local) => self.frames[frame_index]
                .locals
                .get(local)
                .cloned()
                .ok_or_else(|| mir_error_at("MIR local is unavailable", span)),
            MirPlaceBase::Temporary(value) => self.value(frame_index, *value, span),
            MirPlaceBase::Parameter(value) | MirPlaceBase::Capture(value) => {
                let value = self.value(frame_index, *value, span)?;
                match value {
                    RuntimeValue::Address(address) => {
                        require_address_access(&address, MirAccess::Read, span)?;
                        self.read_place(address.frame, address.place, span)
                    }
                    RuntimeValue::Moved => Err(mir_error_at("MIR value was moved", span)),
                    value => Ok(value),
                }
            }
            MirPlaceBase::Static(name) => self.read_static_value(name, span),
        }
    }
    fn read_base_for_write(
        &self,
        frame_index: usize,
        base: &MirPlaceBase,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match base {
            MirPlaceBase::Local(local) => self.frames[frame_index]
                .locals
                .get(local)
                .cloned()
                .ok_or_else(|| mir_error_at("MIR local is unavailable", span)),
            MirPlaceBase::Parameter(value) | MirPlaceBase::Temporary(value) => self.frames
                [frame_index]
                .values
                .get(value)
                .cloned()
                .ok_or_else(|| mir_error_at("MIR value is unavailable at its use", span)),
            MirPlaceBase::Capture(value) => self.frames[frame_index]
                .values
                .get(value)
                .cloned()
                .ok_or_else(|| mir_error_at("MIR capture value is unavailable", span)),
            MirPlaceBase::Static(name) => self.read_static_value(name, span),
        }
    }

    fn write_base(
        &mut self,
        frame_index: usize,
        base: &MirPlaceBase,
        value: RuntimeValue,
        span: Span,
    ) -> Result<(), Diagnostic> {
        self.invalidate_base_overrides(frame_index, base)?;
        match base {
            MirPlaceBase::Local(local) => {
                self.frames[frame_index].locals.insert(*local, value);
            }
            MirPlaceBase::Parameter(source) | MirPlaceBase::Temporary(source) => {
                let current = self.frames[frame_index].values.get(source).cloned();
                match current {
                    Some(RuntimeValue::Address(address))
                        if matches!(base, MirPlaceBase::Parameter(_)) =>
                    {
                        require_address_access(&address, MirAccess::Write, span)?;
                        self.write_place(address.frame, address.place, value, span)?;
                    }
                    Some(RuntimeValue::SharedCell(cell)) => {
                        write_shared_cell(&cell, value, span)?;
                    }
                    Some(RuntimeValue::SharedGuard(_)) => {
                        return Err(mir_error_at(
                            "MIR write requires dereferencing the SharedGuard value",
                            span,
                        ));
                    }
                    None
                    | Some(RuntimeValue::Moved)
                    | Some(RuntimeValue::Data(_))
                    | Some(RuntimeValue::Stream(_))
                    | Some(RuntimeValue::StreamCursor(_))
                    | Some(RuntimeValue::Absent { .. })
                    | Some(RuntimeValue::Closure(_))
                    | Some(RuntimeValue::App(_))
                    | Some(RuntimeValue::Result { .. })
                    | Some(RuntimeValue::Ambient(_))
                    | Some(RuntimeValue::SharedSnapshot(_))
                    | Some(RuntimeValue::SharedTransaction(_))
                    | Some(RuntimeValue::ForeignHandle { .. })
                    | Some(RuntimeValue::ProcessSpec { .. })
                    | Some(RuntimeValue::DmaTransfer { .. })
                    | Some(RuntimeValue::Atomic(_))
                    | Some(RuntimeValue::Game(_))
                    | Some(RuntimeValue::Address(_))
                    | Some(RuntimeValue::Aggregate(_))
                    | Some(RuntimeValue::CellGuard(_))
                    | Some(RuntimeValue::GcRoot(_)) => {
                        self.frames[frame_index].values.insert(*source, value);
                    }
                }
            }
            MirPlaceBase::Capture(source) => {
                if let Some(cell) = self.capture_cell_for_value(frame_index, *source) {
                    *cell.borrow_mut() = value.clone();
                    self.frames[frame_index].values.insert(*source, value);
                    return Ok(());
                }
                let captured = self.frames[frame_index]
                    .values
                    .get(source)
                    .cloned()
                    .ok_or_else(|| mir_error_at("MIR capture value is unavailable", span))?;
                match &captured {
                    RuntimeValue::SharedCell(cell) => {
                        write_shared_cell(cell.as_ref(), value, span)?;
                    }
                    RuntimeValue::SharedGuard(_) => {
                        return Err(mir_error_at(
                            "MIR write requires dereferencing the SharedGuard value",
                            span,
                        ));
                    }
                    RuntimeValue::Moved => {
                        self.frames[frame_index].values.insert(*source, value);
                    }
                    RuntimeValue::Address(address) => {
                        require_address_access(address, MirAccess::Write, span)?;
                        self.write_place(address.frame, address.place, value, span)?;
                    }
                    RuntimeValue::Data(_)
                    | RuntimeValue::Stream(_)
                    | RuntimeValue::StreamCursor(_)
                    | RuntimeValue::Absent { .. }
                    | RuntimeValue::Closure(_)
                    | RuntimeValue::App(_)
                    | RuntimeValue::Result { .. }
                    | RuntimeValue::Ambient(_)
                    | RuntimeValue::SharedSnapshot(_)
                    | RuntimeValue::SharedTransaction(_)
                    | RuntimeValue::ForeignHandle { .. }
                    | RuntimeValue::ProcessSpec { .. }
                    | RuntimeValue::DmaTransfer { .. }
                    | RuntimeValue::Atomic(_)
                    | RuntimeValue::Game(_)
                    | RuntimeValue::Aggregate(_)
                    | RuntimeValue::CellGuard(_)
                    | RuntimeValue::GcRoot(_) => {
                        self.frames[frame_index].values.insert(*source, value);
                    }
                }
            }
            MirPlaceBase::Static(name) => self.store_static_value(name, value),
        }
        Ok(())
    }

    fn project(
        &mut self,
        frame_index: usize,
        value: RuntimeValue,
        projection: &MirProjection,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        match projection {
            MirProjection::Field { field, .. } => self.project_field_id(value, *field, span),
            MirProjection::Index {
                index,
                call,
                write_call: _,
                kind,
                location,
                context,
                span: _,
            } => {
                if *kind == MirIndexKind::Pool {
                    let index = self.value(frame_index, *index, span)?;
                    return self.eval_pool_index(value, index, *location, context.as_ref(), span);
                }
                let base = runtime_to_data(value, span)?;
                let index = runtime_to_data(self.value(frame_index, *index, span)?, span)?;
                let args =
                    self.index_prelude_args(*call, base, index, *location, context.as_ref(), span)?;
                self.eval_prelude(*call, args, None, span)
                    .map(RuntimeValue::Data)
            }
            MirProjection::Deref { .. } => match value {
                RuntimeValue::SharedGuard(guard) => self.deref_shared_guard(guard, span),
                RuntimeValue::Address(address) => {
                    require_address_access(&address, MirAccess::Read, span)?;
                    self.read_place(address.frame, address.place, span)
                }
                RuntimeValue::SharedCell(cell) => {
                    if let Some(storage) = &cell.guard.storage {
                        storage.refresh_scalar_shadow(span)?;
                    }
                    Ok(RuntimeValue::Data(shared_payload_read(
                        &cell.guard.payload,
                        &cell.path,
                        span,
                    )?))
                }
                RuntimeValue::CellGuard(guard) => Ok(RuntimeValue::Data(cell_payload_read(
                    self.program,
                    &guard.payload,
                    &guard.path,
                    span,
                )?)),
                RuntimeValue::Moved
                | RuntimeValue::Stream(_)
                | RuntimeValue::StreamCursor(_)
                | RuntimeValue::Data(_)
                | RuntimeValue::Atomic(_)
                | RuntimeValue::Absent { .. }
                | RuntimeValue::ForeignHandle { .. }
                | RuntimeValue::ProcessSpec { .. }
                | RuntimeValue::DmaTransfer { .. }
                | RuntimeValue::Aggregate(_)
                | RuntimeValue::Closure(_)
                | RuntimeValue::App(_)
                | RuntimeValue::Result { .. }
                | RuntimeValue::Ambient(_)
                | RuntimeValue::SharedSnapshot(_)
                | RuntimeValue::SharedTransaction(_)
                | RuntimeValue::GcRoot(_)
                | RuntimeValue::Game(_) => Err(mir_error_at(
                    "MIR place dereference requires an address or guard",
                    span,
                )),
            },
        }
    }

    fn publish_live_value_update(
        &self,
        place: &jet_foundation::MIR::MirPlace,
        value: &RuntimeValue,
        span: Span,
    ) {
        let Some(key) = place.persist_key.as_deref() else {
            return;
        };
        let type_identity = place.ty.canonical_key();
        let rendered = runtime_to_data(value.clone(), span)
            .ok()
            .and_then(|value| crate::Comptime::MirBridge::mir_to_ct_value(value, span).ok())
            .map(|value| value.jet_show());
        crate::scheduler::jet_observe_live_value_update(key, &type_identity, rendered.as_deref());
    }

    fn write_moved_projected_place(
        &mut self,
        frame_index: usize,
        place: &jet_foundation::MIR::MirPlace,
        value: RuntimeValue,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let steps = self.move_steps_for_frame(frame_index, &place.projections, span)?;
        let base = self.read_base_for_write(frame_index, &place.base, span)?;
        let updated = self.replace_runtime_steps(base, &steps, value.clone(), span)?;
        self.write_base(frame_index, &place.base, updated, span)?;
        self.publish_live_value_update(place, &value, span);
        Ok(())
    }
    fn write_pool_place_path(
        &mut self,
        frame_index: usize,
        place: &jet_foundation::MIR::MirPlace,
        extra_steps: &[MoveStep],
        value: RuntimeValue,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some(pool_position) = place.projections.iter().position(|projection| {
            matches!(
                projection,
                MirProjection::Index {
                    kind: MirIndexKind::Pool,
                    ..
                }
            )
        }) else {
            return Err(mir_error_at("MIR Pool place has no checked index", span));
        };
        if pool_position != 0 {
            return Err(mir_error_at(
                "MIR Pool place has an unsupported projected pool base",
                span,
            ));
        }
        let MirProjection::Index {
            index,
            location,
            context,
            ..
        } = &place.projections[pool_position]
        else {
            unreachable!("checked Pool projection position");
        };
        let base = self.read_base_for_write(frame_index, &place.base, span)?;
        let base = self.runtime_to_ct(base, span)?;
        let pool = mir_runtime_owner::<MirPool>(&base)
            .ok_or_else(|| mir_error_at("MIR Pool place base has no native owner", span))?;
        let index = self.value(frame_index, *index, span)?;
        let index = match runtime_to_data(index, span)? {
            MirEvalValue::Int(index) => MirPoolId::from_word(index),
            _ => None,
        };
        let Some(index) = index else {
            return Err(self.pool_index_stop(*location, context.as_ref(), span));
        };

        let mut steps =
            Vec::with_capacity(place.projections.len().saturating_sub(1) + extra_steps.len());
        for projection in place.projections.iter().skip(1) {
            match projection {
                MirProjection::Field { field, .. } => {
                    let name = field_name(self.program, *field)
                        .ok_or_else(|| mir_error_at("MIR field ID is missing", span))?;
                    steps.push(MoveStep::Field {
                        field: *field,
                        name,
                    });
                }
                MirProjection::Index { index, kind, .. } => {
                    let index = runtime_to_data(self.value(frame_index, *index, span)?, span)?;
                    steps.push(MoveStep::Index {
                        value: index,
                        kind: *kind,
                    });
                }
                MirProjection::Deref { .. } => {
                    return Err(mir_error_at(
                        "MIR Pool place cannot project through a dereference",
                        span,
                    ));
                }
            }
        }
        steps.extend_from_slice(extra_steps);

        let current = {
            let pool = pool.lock().unwrap_or_else(|error| error.into_inner());
            pool.checked_get(index).cloned()
        };
        let Some(current) = current else {
            return Err(self.pool_index_stop(*location, context.as_ref(), span));
        };
        let updated = if steps.is_empty() {
            self.runtime_to_ct(value, span)?
        } else {
            let current = runtime_from_ct(current, span)?;
            let current = self.replace_runtime_steps(current, &steps, value, span)?;
            self.runtime_to_ct(current, span)?
        };
        let mut pool = pool.lock().unwrap_or_else(|error| error.into_inner());
        if pool.checked_get(index).is_none() {
            drop(pool);
            return Err(self.pool_index_stop(*location, context.as_ref(), span));
        }
        *pool
            .checked_get_mut(index)
            .expect("Pool slot remained live after checked_get") = updated;
        Ok(())
    }

    fn write_place_path(
        &mut self,
        frame_index: usize,
        place_id: jet_foundation::MIR::MirPlaceId,
        extra_steps: &[MoveStep],
        value: RuntimeValue,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        let place = function
            .places
            .iter()
            .find(|place| place.id == place_id)
            .ok_or_else(|| mir_error_at("MIR write target place is unavailable", span))?
            .clone();
        let override_value = self.frames[frame_index].place_overrides.remove(&place_id);
        if override_value
            .as_ref()
            .is_some_and(|current| matches!(current, RuntimeValue::Moved))
        {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        if let &(MirPlaceBase::Capture(source) | MirPlaceBase::Parameter(source)) = &place.base {
            if let Some(RuntimeValue::Address(address)) =
                self.frames[frame_index].values.get(&source).cloned()
            {
                require_address_access(&address, MirAccess::Write, span)?;
                let mut steps = self.move_steps_for_frame(frame_index, &place.projections, span)?;
                steps.extend_from_slice(extra_steps);
                if extra_steps.is_empty() && !place.projections.is_empty() {
                    self.publish_live_value_update(&place, &value, span);
                }
                if steps.is_empty() {
                    return self.write_place(address.frame, address.place, value, span);
                }
                return self.write_place_path(address.frame, address.place, &steps, value, span);
            }
        }
        if place.projections.iter().any(|projection| {
            matches!(
                projection,
                MirProjection::Index {
                    kind: MirIndexKind::Pool,
                    ..
                }
            )
        }) {
            self.write_pool_place_path(frame_index, &place, extra_steps, value.clone(), span)?;
            if extra_steps.is_empty() {
                self.publish_live_value_update(&place, &value, span);
            }
            return Ok(());
        }
        if place.projections.is_empty() && extra_steps.is_empty() {
            self.write_base(frame_index, &place.base, value.clone(), span)?;
            self.publish_live_value_update(&place, &value, span);
            return Ok(());
        }
        let base = self.read_base_for_write(frame_index, &place.base, span)?;
        if matches!(base, RuntimeValue::Moved) {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        let mut steps = self.move_steps_for_frame(frame_index, &place.projections, span)?;
        steps.extend_from_slice(extra_steps);
        let updated = self.replace_runtime_steps(base, &steps, value.clone(), span)?;
        self.store_base_value(frame_index, &place.base, updated, span)?;
        if extra_steps.is_empty() {
            self.publish_live_value_update(&place, &value, span);
        }
        Ok(())
    }

    fn replace_runtime_steps(
        &mut self,
        value: RuntimeValue,
        steps: &[MoveStep],
        replacement: RuntimeValue,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        if steps.is_empty() {
            if runtime_contains_moved(&replacement) {
                return Err(mir_error_at("MIR write value was moved", span));
            }
            return Ok(replacement);
        }
        if let RuntimeValue::Ambient(mut ambient) = value {
            let replacement = self.runtime_to_ct(replacement, span)?;
            replace_ct_steps(&mut ambient, steps, replacement, span)?;
            return Ok(RuntimeValue::Ambient(ambient));
        }
        let (step, rest) = steps
            .split_first()
            .ok_or_else(|| mir_error_at("MIR write projection is empty", span))?;
        match step {
            MoveStep::Field { field, name } => match value {
                RuntimeValue::Aggregate(mut fields) => {
                    let Some((_, child)) =
                        fields.iter_mut().find(|(candidate, _)| candidate == field)
                    else {
                        return Err(mir_error_at("MIR aggregate field ID is not present", span));
                    };
                    if rest.is_empty() {
                        *child = replacement;
                    } else {
                        if matches!(child, RuntimeValue::Moved) {
                            return Err(mir_error_at("MIR write parent is moved", span));
                        }
                        let child = std::mem::replace(child, RuntimeValue::Moved);
                        let updated = self.replace_runtime_steps(child, rest, replacement, span)?;
                        fields
                            .iter_mut()
                            .find(|(candidate, _)| candidate == field)
                            .expect("MIR aggregate field was found")
                            .1 = updated;
                    }
                    Ok(RuntimeValue::Aggregate(fields))
                }
                RuntimeValue::Data(mut data) => {
                    let replacement = runtime_to_data(replacement, span)?;
                    replace_data_steps(&mut data, steps, replacement, span)?;
                    Ok(RuntimeValue::Data(data))
                }
                RuntimeValue::Moved => Err(mir_error_at("MIR write parent is moved", span)),
                RuntimeValue::Stream(_)
                | RuntimeValue::StreamCursor(_)
                | RuntimeValue::Address(_)
                | RuntimeValue::Absent { .. }
                | RuntimeValue::Closure(_)
                | RuntimeValue::App(_)
                | RuntimeValue::Result { .. }
                | RuntimeValue::Ambient(_)
                | RuntimeValue::SharedSnapshot(_)
                | RuntimeValue::SharedTransaction(_)
                | RuntimeValue::Atomic(_)
                | RuntimeValue::ForeignHandle { .. }
                | RuntimeValue::ProcessSpec { .. }
                | RuntimeValue::DmaTransfer { .. }
                | RuntimeValue::SharedGuard(_)
                | RuntimeValue::SharedCell(_)
                | RuntimeValue::CellGuard(_)
                | RuntimeValue::GcRoot(_)
                | RuntimeValue::Game(_) => Err(mir_error_at(
                    &format!("MIR field `{name}` cannot be reinitialized in this value"),
                    span,
                )),
            },
            MoveStep::Index { value: _, kind } => match value {
                RuntimeValue::Data(mut data) => {
                    let replacement = runtime_to_data(replacement, span)?;
                    replace_data_steps(&mut data, steps, replacement, span)?;
                    Ok(RuntimeValue::Data(data))
                }
                RuntimeValue::Moved => Err(mir_error_at("MIR write parent is moved", span)),
                RuntimeValue::Aggregate(_)
                | RuntimeValue::Stream(_)
                | RuntimeValue::StreamCursor(_)
                | RuntimeValue::Address(_)
                | RuntimeValue::Absent { .. }
                | RuntimeValue::Closure(_)
                | RuntimeValue::App(_)
                | RuntimeValue::Result { .. }
                | RuntimeValue::Ambient(_)
                | RuntimeValue::SharedSnapshot(_)
                | RuntimeValue::SharedTransaction(_)
                | RuntimeValue::Atomic(_)
                | RuntimeValue::ForeignHandle { .. }
                | RuntimeValue::ProcessSpec { .. }
                | RuntimeValue::DmaTransfer { .. }
                | RuntimeValue::SharedGuard(_)
                | RuntimeValue::SharedCell(_)
                | RuntimeValue::CellGuard(_)
                | RuntimeValue::GcRoot(_)
                | RuntimeValue::Game(_) => Err(mir_error_at(
                    &format!("MIR {kind:?} index cannot be reinitialized in this value"),
                    span,
                )),
            },
            MoveStep::Deref => match value {
                RuntimeValue::Address(address) => {
                    require_address_access(&address, MirAccess::Write, span)?;
                    if rest.is_empty() {
                        self.write_place(address.frame, address.place, replacement, span)?;
                    } else {
                        self.write_place_path(
                            address.frame,
                            address.place,
                            rest,
                            replacement,
                            span,
                        )?;
                    }
                    Ok(RuntimeValue::Address(address))
                }
                RuntimeValue::Moved => Err(mir_error_at("MIR write parent is moved", span)),
                RuntimeValue::Stream(_)
                | RuntimeValue::StreamCursor(_)
                | RuntimeValue::Data(_)
                | RuntimeValue::Atomic(_)
                | RuntimeValue::Absent { .. }
                | RuntimeValue::Closure(_)
                | RuntimeValue::App(_)
                | RuntimeValue::Result { .. }
                | RuntimeValue::Ambient(_)
                | RuntimeValue::SharedSnapshot(_)
                | RuntimeValue::SharedTransaction(_)
                | RuntimeValue::ForeignHandle { .. }
                | RuntimeValue::ProcessSpec { .. }
                | RuntimeValue::DmaTransfer { .. }
                | RuntimeValue::Aggregate(_)
                | RuntimeValue::SharedGuard(_)
                | RuntimeValue::SharedCell(_)
                | RuntimeValue::CellGuard(_)
                | RuntimeValue::GcRoot(_)
                | RuntimeValue::Game(_) => Err(mir_error_at(
                    "MIR projected dereference write requires an address",
                    span,
                )),
            },
        }
    }

    fn write_value_alias(
        &mut self,
        frame_index: usize,
        alias: ValueAlias,
        place: &jet_foundation::MIR::MirPlace,
        value: RuntimeValue,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let mut steps = alias.steps;
        steps.extend(self.move_steps_for_frame(frame_index, &place.projections, span)?);
        self.write_place_path(alias.frame, alias.place, &steps, value, span)
    }

    fn write_place(
        &mut self,
        frame_index: usize,
        place_id: jet_foundation::MIR::MirPlaceId,
        value: RuntimeValue,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        let place = function
            .places
            .iter()
            .find(|place| place.id == place_id)
            .ok_or_else(|| mir_error_at("MIR write targets an unavailable place", span))?
            .clone();
        let temporary_alias = match &place.base {
            MirPlaceBase::Temporary(source) | MirPlaceBase::Parameter(source) => {
                self.frames[frame_index].value_aliases.get(source).cloned()
            }
            MirPlaceBase::Local(_) | MirPlaceBase::Capture(_) | MirPlaceBase::Static(_) => None,
        };
        if place.projections.is_empty() {
            self.write_base(frame_index, &place.base, value.clone(), span)?;
            if let Some(alias) = temporary_alias.clone() {
                self.write_value_alias(frame_index, alias, &place, value.clone(), span)?;
            }
            self.publish_live_value_update(&place, &value, span);
            return Ok(());
        }
        let capture_address = match &place.base {
            &MirPlaceBase::Capture(source) | &MirPlaceBase::Parameter(source) => matches!(
                self.frames[frame_index].values.get(&source),
                Some(RuntimeValue::Address(_))
            ),
            MirPlaceBase::Local(_) | MirPlaceBase::Temporary(_) | MirPlaceBase::Static(_) => false,
        };
        let has_deref = place
            .projections
            .iter()
            .any(|projection| matches!(projection, MirProjection::Deref { .. }));
        if capture_address || has_deref {
            return self.write_place_path(frame_index, place_id, &[], value, span);
        }

        if place.projections.iter().any(|projection| {
            matches!(
                projection,
                MirProjection::Index {
                    kind: MirIndexKind::Pool,
                    ..
                }
            )
        }) {
            self.write_pool_place_path(frame_index, &place, &[], value.clone(), span)?;
            if let Some(alias) = temporary_alias {
                self.write_value_alias(frame_index, alias, &place, value.clone(), span)?;
            }
            self.publish_live_value_update(&place, &value, span);
            return Ok(());
        }

        let mut current = self.read_base(frame_index, &place.base, span)?;
        if runtime_contains_moved(&current) {
            self.write_moved_projected_place(frame_index, &place, value.clone(), span)?;
            if let Some(alias) = temporary_alias.clone() {
                self.write_value_alias(frame_index, alias, &place, value.clone(), span)?;
            }
            return Ok(());
        }
        let mut updates = Vec::with_capacity(place.projections.len());
        for (projection_index, projection) in place.projections.iter().enumerate() {
            match projection {
                MirProjection::Field { field, .. } => {
                    let parent = current.clone();
                    current = self.project_field_id(current, *field, span)?;
                    updates.push(PlaceUpdate::Field {
                        parent,
                        field: *field,
                    });
                }
                MirProjection::Index {
                    index,
                    kind,
                    write_call,
                    location,
                    context,
                    ..
                } => {
                    let parent = runtime_to_data(current.clone(), span)?;
                    let index = runtime_to_data(self.value(frame_index, *index, span)?, span)?;
                    if *kind != MirIndexKind::Map || projection_index + 1 != place.projections.len()
                    {
                        current = self.project(frame_index, current, projection, span)?;
                    }
                    updates.push(PlaceUpdate::Index {
                        parent,
                        index,
                        kind: *kind,
                        write_call: *write_call,
                        location: *location,
                        context: context.clone(),
                    });
                }
                MirProjection::Deref { .. } => {
                    return Err(mir_error_at(
                        "MIR projected dereference write requires an address root",
                        span,
                    ));
                }
            }
        }

        let mut updated = value.clone();
        for update in updates.into_iter().rev() {
            match update {
                PlaceUpdate::Field { parent, field } => {
                    updated = replace_runtime_field(self.program, parent, field, updated, span)?;
                }
                PlaceUpdate::Index {
                    mut parent,
                    index,
                    kind,
                    write_call,
                    location,
                    context,
                } => {
                    let write_call = write_call.ok_or_else(|| {
                        mir_error_at(
                            "MIR indexed write has no canonical setter Prelude row",
                            span,
                        )
                    })?;
                    let updated_data = runtime_to_data(updated, span)?;
                    let row = self.prelude_row(write_call, span)?;
                    let structural_setter = matches!(
                        (row.module.as_str(), row.member.as_str(), kind),
                        ("core.index", "index_map_set", MirIndexKind::Map)
                            | (
                                "core.index",
                                "index_list_set",
                                MirIndexKind::List | MirIndexKind::FixedListProof
                            )
                    );
                    if !structural_setter {
                        let args = self.index_setter_args(
                            write_call,
                            parent.clone(),
                            index.clone(),
                            updated_data.clone(),
                            location,
                            context.as_ref(),
                            span,
                        )?;
                        let _ = self.eval_prelude(write_call, args, None, span)?;
                    }
                    replace_path(
                        &mut parent,
                        &[PathStep::Index { value: index, kind }],
                        updated_data,
                        span,
                    )?;
                    updated = RuntimeValue::Data(parent);
                }
            }
        }
        self.write_base(frame_index, &place.base, updated, span)?;
        if let Some(alias) = temporary_alias {
            self.write_value_alias(frame_index, alias, &place, value.clone(), span)?;
        }
        self.publish_live_value_update(&place, &value, span);
        Ok(())
    }

    fn take_drop_place(
        &mut self,
        frame_index: usize,
        place: &jet_foundation::MIR::MirPlace,
    ) -> Option<RuntimeValue> {
        if let Some(value) = self.frames[frame_index].place_overrides.remove(&place.id) {
            return (!matches!(value, RuntimeValue::Moved)).then_some(value);
        }
        if !place.projections.is_empty() {
            return None;
        }
        match &place.base {
            jet_foundation::MIR::MirPlaceBase::Local(local) => {
                self.frames[frame_index].locals.remove(local)
            }
            jet_foundation::MIR::MirPlaceBase::Parameter(value)
            | jet_foundation::MIR::MirPlaceBase::Temporary(value) => {
                self.frames[frame_index].values.remove(value)
            }
            jet_foundation::MIR::MirPlaceBase::Capture(_)
            | jet_foundation::MIR::MirPlaceBase::Static(_) => None,
        }
    }
    fn handle_id_for_type(&self, ty: &MirType) -> Option<jet_foundation::MIR::MirHandleId> {
        self.program.handles.iter().find_map(|handle| {
            if handle.ty.same_checked_type(ty)
                || matches!(
                    (&handle.ty.kind(), &ty.kind()),
                    (
                        MirTypeKind::Apply { name: left, .. },
                        MirTypeKind::Apply { name: right, .. }
                    ) if left.id == right.id
                )
            {
                Some(handle.id)
            } else {
                None
            }
        })
    }
    fn process_handle_id(
        &self,
        name: &str,
        span: Span,
    ) -> Result<jet_foundation::MIR::MirHandleId, Diagnostic> {
        self.program
            .handles
            .iter()
            .find(|handle| {
                handle.payload.library == "core.process" && handle.payload.typedef_name == name
            })
            .map(|handle| handle.id)
            .ok_or_else(|| {
                mir_error_at(
                    &format!("MIR process handle `{name}` has no canonical lifecycle row"),
                    span,
                )
            })
    }
    fn project_process_stdin(
        &mut self,
        child: &MirHandleToken,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let child_raw = child
            .raw()
            .ok_or_else(|| mir_error_at("MIR process child handle was moved", span))?;
        if let Some(token) = self.process_stdin_tokens.borrow().get(&child_raw).cloned() {
            return Ok(RuntimeValue::ForeignHandle { token });
        }
        let result = crate::Comptime::try_ambient_mir_handle(
            "process.child.stdin",
            Some(child_raw),
            Vec::new(),
            span,
        )
        .ok_or_else(|| {
            mir_error_at(
                "MIR process child stdin projection has no interpreter ambient host binding",
                span,
            )
        })??;
        let stdin_raw = match result {
            crate::Comptime::AmbientMirHandleResult::Handle(raw) => {
                if raw == 0 {
                    return Err(mir_error_at(
                        "MIR ProcessStdin ambient binding returned an invalid identity",
                        span,
                    ));
                }
                raw
            }
            crate::Comptime::AmbientMirHandleResult::Value(_) => {
                return Err(mir_error_at(
                    "MIR process child stdin projection returned a value",
                    span,
                ))
            }
        };
        let handle = self.process_handle_id("ProcessStdin", span)?;
        let token = MirHandleToken::new(handle, stdin_raw);
        self.process_stdin_tokens
            .borrow_mut()
            .insert(child_raw, token.clone());
        Ok(RuntimeValue::ForeignHandle { token })
    }
    fn forget_process_stdin_token(&mut self, token: &MirHandleToken) {
        let identity = token.identity();
        self.process_stdin_tokens
            .borrow_mut()
            .retain(|_, cached| cached.identity() != identity);
    }

    fn project_field_id(
        &mut self,
        value: RuntimeValue,
        field: MirFieldId,
        span: Span,
    ) -> Result<RuntimeValue, Diagnostic> {
        let name = field_name(self.program, field)
            .ok_or_else(|| mir_error_at("MIR field ID is missing", span))?;
        if name == "stdin" {
            if let RuntimeValue::ForeignHandle { token } = &value {
                let process_child = self.program.handles.iter().find(|handle| {
                    handle.payload.library == "core.process"
                        && handle.payload.typedef_name == "ProcessChild"
                });
                if process_child.is_some_and(|handle| token.handle_id() == handle.id) {
                    return self.project_process_stdin(token, span);
                }
            }
        }
        project_field_id(self.program, value, field, span)
    }
    fn run_drops(
        &mut self,
        frame_index: usize,
        edge: DropEdge,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let function = program_function(self.program, self.frames[frame_index].function)?;
        let drops = function.drops.clone();
        let places = function.places.clone();
        for drop in &drops {
            let applies = match (&drop.edge, edge) {
                (jet_foundation::MIR::MirDropEdge::Normal, DropEdge::Normal)
                | (jet_foundation::MIR::MirDropEdge::Return, DropEdge::Return) => true,
                (jet_foundation::MIR::MirDropEdge::Normal, DropEdge::Return)
                | (jet_foundation::MIR::MirDropEdge::Return, DropEdge::Normal)
                | (
                    jet_foundation::MIR::MirDropEdge::Failure(_),
                    DropEdge::Normal | DropEdge::Return,
                )
                | (
                    jet_foundation::MIR::MirDropEdge::Unwind(_),
                    DropEdge::Normal | DropEdge::Return,
                ) => false,
            };
            if !applies {
                continue;
            }
            let place = places
                .iter()
                .find(|place| place.id == drop.place)
                .ok_or_else(|| mir_error_at("MIR drop targets an unavailable place", span))?;
            if self.handle_id_for_type(&place.ty).is_some() {
                if let Some(RuntimeValue::ForeignHandle { token }) =
                    self.take_drop_place(frame_index, place)
                {
                    self.close_foreign_handle(token, span)?;
                }
            } else {
                self.frames[frame_index].place_overrides.remove(&drop.place);
            }
        }
        Ok(())
    }
}

fn mutate_mir_receiver(
    receiver: &mut MirEvalValue,
    member: &str,
    args: Vec<MirEvalValue>,
    result_ty: Option<&MirType>,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    if let Some(result) =
        direct_mutating_collection(receiver, member, args.clone(), result_ty, span)?
    {
        return Ok(result);
    }

    let comptime_member = member.strip_prefix("byte_buffer_").unwrap_or(member);
    let mut comptime_receiver =
        crate::Comptime::MirBridge::mir_to_ct_value(receiver.clone(), span)?;
    let comptime_args = args
        .into_iter()
        .map(|value| crate::Comptime::MirBridge::mir_to_ct_value(value, span))
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let result = crate::Comptime::Builtins::apply_mutating_with_type(
        &mut comptime_receiver,
        comptime_member,
        comptime_args,
        span,
        None,
    )?;
    *receiver = crate::Comptime::MirBridge::ct_to_mir_value(comptime_receiver, span)?;
    crate::Comptime::MirBridge::ct_to_mir_value(result, span)
}

fn direct_mutating_collection(
    receiver: &mut MirEvalValue,
    member: &str,
    args: Vec<MirEvalValue>,
    result_ty: Option<&MirType>,
    span: Span,
) -> Result<Option<MirEvalValue>, Diagnostic> {
    let mut args = args.into_iter();
    match member {
        "list_push" => {
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR list push is missing its value", span))?;
            match receiver {
                MirEvalValue::List(items) => {
                    items.push(value);
                    Ok(Some(MirEvalValue::Unit))
                }
                _ => Err(mir_error_at("MIR list push receiver is not a List", span)),
            }
        }
        "list_extend" => {
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR list extend is missing its source", span))?;
            match (receiver, value) {
                (MirEvalValue::List(items), MirEvalValue::List(other)) => {
                    items.extend(other);
                    Ok(Some(MirEvalValue::Unit))
                }
                _ => Err(mir_error_at("MIR list extend requires two Lists", span)),
            }
        }
        "list_reverse" => match receiver {
            MirEvalValue::List(items) => {
                items.reverse();
                Ok(Some(MirEvalValue::Unit))
            }
            _ => Err(mir_error_at(
                "MIR list reverse receiver is not a List",
                span,
            )),
        },
        "list_sort" | "list_sort_desc" => match receiver {
            MirEvalValue::List(items) => {
                let descending = member == "list_sort_desc";
                let mut sort_error = None;
                items.sort_by(|left, right| {
                    let ordering = mir_compare(left, right, span);
                    match ordering {
                        Ok(ordering) if descending => ordering.reverse(),
                        Ok(ordering) => ordering,
                        Err(error) => {
                            sort_error.get_or_insert(error);
                            std::cmp::Ordering::Equal
                        }
                    }
                });
                if let Some(error) = sort_error {
                    Err(error)
                } else {
                    Ok(Some(MirEvalValue::Unit))
                }
            }
            _ => Err(mir_error_at("MIR list sort receiver is not a List", span)),
        },
        "list_clear" => match receiver {
            MirEvalValue::List(items) => {
                items.clear();
                Ok(Some(MirEvalValue::Unit))
            }
            MirEvalValue::Map(entries) => {
                entries.clear();
                Ok(Some(MirEvalValue::Unit))
            }
            _ => Err(mir_error_at(
                "MIR collection clear receiver is not a collection",
                span,
            )),
        },
        "list_try_push" => {
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR list try_push is missing its value", span))?;
            match receiver {
                MirEvalValue::List(items) => {
                    if items.try_reserve(1).is_err() {
                        return Ok(Some(MirEvalValue::FailedTold(Box::new(
                            MirEvalValue::String("List allocation failed".to_string()),
                        ))));
                    }
                    items.push(value);
                    Ok(Some(MirEvalValue::Present(Box::new(MirEvalValue::Unit))))
                }
                _ => Err(mir_error_at(
                    "MIR list try_push receiver is not a List",
                    span,
                )),
            }
        }
        "list_try_reserve" => {
            let additional = match args.next() {
                Some(MirEvalValue::Int(value)) => usize::try_from(value).ok(),
                _ => None,
            }
            .ok_or_else(|| {
                mir_error_at("MIR list try_reserve amount is not a nonnegative Int", span)
            })?;
            match receiver {
                MirEvalValue::List(items) => {
                    if items.try_reserve(additional).is_err() {
                        return Ok(Some(MirEvalValue::FailedTold(Box::new(
                            MirEvalValue::String("List allocation failed".to_string()),
                        ))));
                    }
                    Ok(Some(MirEvalValue::Present(Box::new(MirEvalValue::Unit))))
                }
                _ => Err(mir_error_at(
                    "MIR list try_reserve receiver is not a List",
                    span,
                )),
            }
        }
        "list_pop" => match receiver {
            MirEvalValue::List(items) => Ok(Some(match items.pop() {
                Some(value) => MirEvalValue::Present(Box::new(value)),
                None => direct_absent_value(result_ty, span)?,
            })),
            _ => Err(mir_error_at("MIR list pop receiver is not a List", span)),
        },
        "list_insert" => {
            let index = match args.next() {
                Some(MirEvalValue::Int(value)) => value,
                _ => return Err(mir_error_at("MIR list insert index is not an Int", span)),
            };
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR list insert is missing its value", span))?;
            match receiver {
                MirEvalValue::List(items) => {
                    let position = usize::try_from(index).ok();
                    if position.is_none_or(|position| position > items.len()) {
                        return Err(mir_error_at("MIR list insert index is out of bounds", span));
                    }
                    items.insert(position.expect("checked list insert index"), value);
                    Ok(Some(MirEvalValue::Unit))
                }
                _ => Err(mir_error_at("MIR list insert receiver is not a List", span)),
            }
        }
        "list_remove_value" => {
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR list remove value is missing its value", span))?;
            match receiver {
                MirEvalValue::List(items) => {
                    Ok(Some(match items.iter().position(|item| *item == value) {
                        Some(index) => MirEvalValue::Present(Box::new(items.remove(index))),
                        None => direct_absent_value(result_ty, span)?,
                    }))
                }
                _ => Err(mir_error_at("MIR list remove receiver is not a List", span)),
            }
        }
        "list_remove_slot" => {
            let index = match args.next() {
                Some(MirEvalValue::Int(value)) => usize::try_from(value).ok(),
                _ => None,
            }
            .ok_or_else(|| mir_error_at("MIR list remove index is not a nonnegative Int", span))?;
            match receiver {
                MirEvalValue::List(items) if index < items.len() => {
                    Ok(Some(MirEvalValue::Present(Box::new(items.remove(index)))))
                }
                MirEvalValue::List(_) => {
                    Err(mir_error_at("MIR list remove index is out of bounds", span))
                }
                _ => Err(mir_error_at("MIR list remove receiver is not a List", span)),
            }
        }
        "priority_queue_pop" => match receiver {
            MirEvalValue::Struct { type_name, fields } if type_name == "PriorityQueue" => {
                let items = fields
                    .iter_mut()
                    .find_map(|(name, value)| (name == "items").then_some(value));
                match items {
                    Some(MirEvalValue::List(items)) => Ok(Some(match items.is_empty() {
                        false => MirEvalValue::Present(Box::new(items.remove(0))),
                        true => direct_absent_value(result_ty, span)?,
                    })),
                    _ => Err(mir_error_at(
                        "MIR priority queue has no List items field",
                        span,
                    )),
                }
            }
            _ => Err(mir_error_at(
                "MIR priority queue pop receiver is not a PriorityQueue",
                span,
            )),
        },
        "set_pop" => {
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR set pop is missing its value", span))?;
            match receiver {
                MirEvalValue::Struct { type_name, fields }
                    if type_name == "Set" || type_name == "Rank" =>
                {
                    let items = fields
                        .iter_mut()
                        .find_map(|(name, field)| (name == "items").then_some(field));
                    match items {
                        Some(MirEvalValue::List(items)) => {
                            Ok(Some(match items.iter().position(|item| item == &value) {
                                Some(index) => MirEvalValue::Present(Box::new(items.remove(index))),
                                None => direct_absent_value(result_ty, span)?,
                            }))
                        }
                        _ => Err(mir_error_at("MIR set has no List items field", span)),
                    }
                }
                _ => Err(mir_error_at("MIR set pop receiver is not a Set", span)),
            }
        }
        "get_disjoint_write" => Err(mir_error_at(
            "MIR disjoint write requires the view adapter",
            span,
        )),
        "map_insert" => {
            let key = args
                .next()
                .ok_or_else(|| mir_error_at("MIR map insert is missing its key", span))
                .and_then(|value| mir_const_key(value, span))?;
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR map insert is missing its value", span))?;
            match receiver {
                MirEvalValue::Map(entries) => {
                    if let Some((_, existing)) =
                        entries.iter_mut().find(|(candidate, _)| *candidate == key)
                    {
                        *existing = value;
                    } else {
                        entries.push((key, value));
                    }
                    Ok(Some(MirEvalValue::Unit))
                }
                _ => Err(mir_error_at("MIR map insert receiver is not a Map", span)),
            }
        }
        "map_add_new" => {
            let key = args
                .next()
                .ok_or_else(|| mir_error_at("MIR map add_new is missing its key", span))
                .and_then(|value| mir_const_key(value, span))?;
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR map add_new is missing its value", span))?;
            match receiver {
                MirEvalValue::Map(entries) => {
                    if entries.iter().any(|(candidate, _)| *candidate == key) {
                        Ok(Some(MirEvalValue::Bool(false)))
                    } else {
                        entries.push((key, value));
                        Ok(Some(MirEvalValue::Bool(true)))
                    }
                }
                _ => Err(mir_error_at("MIR map add_new receiver is not a Map", span)),
            }
        }
        "map_try_insert" => {
            let key = args
                .next()
                .ok_or_else(|| mir_error_at("MIR map try_insert is missing its key", span))
                .and_then(|value| mir_const_key(value, span))?;
            let value = args
                .next()
                .ok_or_else(|| mir_error_at("MIR map try_insert is missing its value", span))?;
            match receiver {
                MirEvalValue::Map(entries) => {
                    let previous = if let Some((_, existing)) =
                        entries.iter_mut().find(|(candidate, _)| *candidate == key)
                    {
                        MirEvalValue::Present(Box::new(std::mem::replace(existing, value)))
                    } else {
                        entries.push((key, value));
                        direct_absent_value(result_ty, span)?
                    };
                    Ok(Some(MirEvalValue::Present(Box::new(previous))))
                }
                _ => Err(mir_error_at(
                    "MIR map try_insert receiver is not a Map",
                    span,
                )),
            }
        }
        "map_remove" => {
            let key = args
                .next()
                .ok_or_else(|| mir_error_at("MIR map remove is missing its key", span))
                .and_then(|value| mir_const_key(value, span))?;
            match receiver {
                MirEvalValue::Map(entries) => Ok(Some(
                    match entries.iter().position(|(candidate, _)| *candidate == key) {
                        Some(index) => MirEvalValue::Present(Box::new(entries.remove(index).1)),
                        None => direct_absent_value(result_ty, span)?,
                    },
                )),
                _ => Err(mir_error_at("MIR map remove receiver is not a Map", span)),
            }
        }
        "map_pop_first" => match receiver {
            MirEvalValue::Map(entries) => {
                let index = entries
                    .iter()
                    .enumerate()
                    .min_by(|(_, left), (_, right)| left.0.cmp(&right.0))
                    .map(|(index, _)| index);
                Ok(Some(match index {
                    Some(index) => MirEvalValue::Present(Box::new(entries.remove(index).1)),
                    None => direct_absent_value(result_ty, span)?,
                }))
            }
            _ => Err(mir_error_at(
                "MIR map pop_first receiver is not a Map",
                span,
            )),
        },
        "string_try_push" => {
            let addition = match args.next() {
                Some(MirEvalValue::String(value)) => value,
                _ => {
                    return Err(mir_error_at(
                        "MIR string try_push value is not a String",
                        span,
                    ))
                }
            };
            match receiver {
                MirEvalValue::String(text) => {
                    if text.try_reserve(addition.len()).is_err() {
                        return Ok(Some(MirEvalValue::FailedTold(Box::new(
                            MirEvalValue::String("String allocation failed".to_string()),
                        ))));
                    }
                    text.push_str(&addition);
                    Ok(Some(MirEvalValue::Present(Box::new(MirEvalValue::Unit))))
                }
                _ => Err(mir_error_at(
                    "MIR string try_push receiver is not a String",
                    span,
                )),
            }
        }
        _ => Ok(None),
    }
}

fn direct_absent_value(
    result_ty: Option<&MirType>,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    let mut value_ty = result_ty.ok_or_else(|| {
        mir_error_at(
            "MIR mutating collection outcome has no result type fact",
            span,
        )
    })?;
    if let Some((ok, _)) = value_ty.result_parts() {
        value_ty = ok;
    }
    let element = value_ty.option_inner().ok_or_else(|| {
        mir_error_at(
            "MIR mutating collection outcome is not an Option carrier",
            span,
        )
    })?;
    Ok(MirEvalValue::Absent {
        element: element.clone(),
    })
}

fn mir_const_key_value(key: &MirConstKey) -> MirEvalValue {
    match key {
        MirConstKey::Int(value) => MirEvalValue::Int(*value),
        MirConstKey::String(value) => MirEvalValue::String(value.clone()),
        MirConstKey::Bool(value) => MirEvalValue::Bool(*value),
        MirConstKey::Char(value) => MirEvalValue::Char(*value),
        MirConstKey::Enum { type_name, variant } => MirEvalValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: Vec::new(),
        },
        MirConstKey::Struct { type_name, fields } => MirEvalValue::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| (name.clone(), mir_const_key_value(value)))
                .collect(),
        },
        MirConstKey::Tuple(fields) => MirEvalValue::Struct {
            type_name: "Tuple".to_string(),
            fields: fields
                .iter()
                .map(|(name, value)| (name.clone(), mir_const_key_value(value)))
                .collect(),
        },
    }
}

fn mir_const_key(value: MirEvalValue, span: Span) -> Result<MirConstKey, Diagnostic> {
    match value {
        MirEvalValue::Int(value) => Ok(MirConstKey::Int(value)),
        MirEvalValue::String(value) => Ok(MirConstKey::String(value)),
        MirEvalValue::Bool(value) => Ok(MirConstKey::Bool(value)),
        MirEvalValue::Char(value) => Ok(MirConstKey::Char(value)),
        MirEvalValue::Struct { type_name, fields } => Ok(MirConstKey::Struct {
            type_name,
            fields: fields
                .into_iter()
                .map(|(name, value)| Ok((name, mir_const_key(value, span)?)))
                .collect::<Result<Vec<_>, Diagnostic>>()?,
        }),
        MirEvalValue::Enum {
            type_name, variant, ..
        } => Ok(MirConstKey::Enum { type_name, variant }),
        _ => Err(mir_error_at(
            "MIR map key is not a canonical constant key",
            span,
        )),
    }
}
#[derive(Debug, Clone)]
struct Frame {
    function: MirFunctionId,
    block: MirBlockId,
    ip: usize,
    predecessor: Option<MirBlockId>,
    params: Vec<RuntimeValue>,
    captures: Vec<RuntimeValue>,
    capture_cells: Vec<Option<Rc<RefCell<RuntimeValue>>>>,
    values: BTreeMap<MirValueId, RuntimeValue>,
    value_aliases: BTreeMap<MirValueId, ValueAlias>,
    locals: BTreeMap<jet_foundation::MIR::MirLocalId, RuntimeValue>,
    place_overrides: BTreeMap<jet_foundation::MIR::MirPlaceId, RuntimeValue>,
    scopes: Vec<jet_foundation::MIR::MirScopeId>,
    completed_scope_stops: BTreeMap<MirScopeId, String>,
    timeout_starts: BTreeMap<MirScopeId, (i64, i64)>,
    shared_transactions: BTreeMap<MirScopeId, Rc<MirSharedTransaction>>,
    pending_break: Option<RuntimeValue>,
}

impl Frame {
    fn new(
        function: &MirFunction,
        params: Vec<RuntimeValue>,
        captures: Vec<RuntimeValue>,
    ) -> Result<Self, Diagnostic> {
        Self::with_capture_cells(function, params, captures, Vec::new())
    }

    fn with_capture_cells(
        function: &MirFunction,
        params: Vec<RuntimeValue>,
        captures: Vec<RuntimeValue>,
        capture_cells: Vec<Option<Rc<RefCell<RuntimeValue>>>>,
    ) -> Result<Self, Diagnostic> {
        validate_captures(function, &captures)?;
        let capture_cells = if capture_cells.is_empty() {
            vec![None; captures.len()]
        } else if capture_cells.len() == captures.len() {
            capture_cells
        } else {
            return Err(mir_error_at(
                "MIR closure environment storage does not match capture slots",
                function.span,
            ));
        };
        let locals = BTreeMap::new();
        Ok(Self {
            function: function.id,
            block: function.entry,
            ip: 0,
            predecessor: None,
            params,
            captures,
            capture_cells,
            values: BTreeMap::new(),
            locals,
            value_aliases: BTreeMap::new(),
            place_overrides: BTreeMap::new(),
            scopes: Vec::new(),
            completed_scope_stops: BTreeMap::new(),
            timeout_starts: BTreeMap::new(),
            shared_transactions: BTreeMap::new(),
            pending_break: None,
        })
    }
}

fn validate_captures(function: &MirFunction, captures: &[RuntimeValue]) -> Result<(), Diagnostic> {
    if captures.len() != function.capture_params.len() {
        return Err(mir_error_at(
            &format!(
                "MIR closure for `{}` has {} captures but declares {} capture parameters",
                function.key,
                captures.len(),
                function.capture_params.len()
            ),
            function.span,
        ));
    }
    for (slot, capture) in function.capture_params.iter().enumerate() {
        if capture.slot != slot {
            return Err(mir_error_at(
                &format!(
                    "MIR capture `{}` has non-canonical slot {} (expected {})",
                    capture.name, capture.slot, slot
                ),
                capture.span,
            ));
        }
        let Some(value) = captures.get(capture.slot) else {
            return Err(mir_error_at(
                "MIR capture slot is outside the closure environment",
                capture.span,
            ));
        };
        let valid = match (capture.access, value) {
            (MirAccess::Read | MirAccess::Write, value)
                if matches!(capture.ownership.mode, MirOwnershipMode::Owned)
                    && !matches!(value, RuntimeValue::Moved | RuntimeValue::Address(_)) =>
            {
                true
            }
            (MirAccess::Move, RuntimeValue::Data(_))
            | (MirAccess::Move, RuntimeValue::Stream(_))
            | (MirAccess::Move, RuntimeValue::StreamCursor(_))
            | (MirAccess::Move, RuntimeValue::Absent { .. })
            | (MirAccess::Move, RuntimeValue::Closure(_))
            | (MirAccess::Move, RuntimeValue::ForeignHandle { .. })
            | (MirAccess::Move, RuntimeValue::ProcessSpec { .. })
            | (MirAccess::Move, RuntimeValue::DmaTransfer { .. })
            | (MirAccess::Move, RuntimeValue::Game(_))
            | (MirAccess::Move, RuntimeValue::App(_))
            | (MirAccess::Move, RuntimeValue::Result { .. })
            | (MirAccess::Move, RuntimeValue::Ambient(_))
            | (MirAccess::Move, RuntimeValue::Aggregate(_))
            | (MirAccess::Move, RuntimeValue::SharedGuard(_))
            | (MirAccess::Move, RuntimeValue::SharedCell(_))
            | (MirAccess::Move, RuntimeValue::SharedSnapshot(_))
            | (MirAccess::Move, RuntimeValue::SharedTransaction(_))
            | (MirAccess::Move, RuntimeValue::CellGuard(_))
            | (MirAccess::Move, RuntimeValue::GcRoot(_))
            | (MirAccess::Move, RuntimeValue::Atomic(_))
            | (MirAccess::Read, RuntimeValue::Address(_))
            | (MirAccess::Read, RuntimeValue::Aggregate(_))
            | (MirAccess::Read, RuntimeValue::SharedCell(_))
            | (MirAccess::Read, RuntimeValue::SharedSnapshot(_))
            | (MirAccess::Read, RuntimeValue::SharedTransaction(_))
            | (MirAccess::Read, RuntimeValue::CellGuard(_))
            | (MirAccess::Read, RuntimeValue::GcRoot(_))
            | (MirAccess::Read, RuntimeValue::Atomic(_))
            | (MirAccess::Read, RuntimeValue::Game(_))
            | (MirAccess::Read, RuntimeValue::App(_))
            | (MirAccess::Read, RuntimeValue::Result { .. })
            | (MirAccess::Read, RuntimeValue::Ambient(_)) => true,
            (MirAccess::Move, RuntimeValue::Moved)
            | (MirAccess::Read, RuntimeValue::Moved)
            | (MirAccess::Write, RuntimeValue::Moved) => false,
            (MirAccess::Move, RuntimeValue::Address(_))
            | (MirAccess::Read, RuntimeValue::Data(_))
            | (MirAccess::Read, RuntimeValue::Stream(_))
            | (MirAccess::Read, RuntimeValue::StreamCursor(_))
            | (MirAccess::Read, RuntimeValue::Absent { .. })
            | (MirAccess::Read, RuntimeValue::Closure(_))
            | (MirAccess::Read, RuntimeValue::ForeignHandle { .. })
            | (MirAccess::Read, RuntimeValue::ProcessSpec { .. })
            | (MirAccess::Read, RuntimeValue::DmaTransfer { .. })
            | (MirAccess::Read, RuntimeValue::SharedGuard(_))
            | (MirAccess::Write, RuntimeValue::Data(_))
            | (MirAccess::Write, RuntimeValue::Stream(_))
            | (MirAccess::Write, RuntimeValue::StreamCursor(_))
            | (MirAccess::Write, RuntimeValue::Absent { .. })
            | (MirAccess::Write, RuntimeValue::Closure(_))
            | (MirAccess::Write, RuntimeValue::ForeignHandle { .. })
            | (MirAccess::Write, RuntimeValue::ProcessSpec { .. })
            | (MirAccess::Write, RuntimeValue::DmaTransfer { .. })
            | (MirAccess::Write, RuntimeValue::Game(_))
            | (MirAccess::Write, RuntimeValue::App(_))
            | (MirAccess::Write, RuntimeValue::Result { .. })
            | (MirAccess::Write, RuntimeValue::Ambient(_))
            | (MirAccess::Write, RuntimeValue::SharedGuard(_))
            | (MirAccess::Write, RuntimeValue::Address(_))
            | (MirAccess::Write, RuntimeValue::Aggregate(_))
            | (MirAccess::Write, RuntimeValue::SharedCell(_))
            | (MirAccess::Write, RuntimeValue::SharedSnapshot(_))
            | (MirAccess::Write, RuntimeValue::SharedTransaction(_))
            | (MirAccess::Write, RuntimeValue::CellGuard(_))
            | (MirAccess::Write, RuntimeValue::Atomic(_))
            | (MirAccess::Write, RuntimeValue::GcRoot(_)) => false,
        };
        if !valid {
            return Err(mir_error_at(
                "MIR closure capture value does not match target access",
                capture.span,
            ));
        }
    }
    Ok(())
}
fn capture_facts_equal(left: &MirCaptureFacts, right: &MirCaptureFacts) -> bool {
    left.escapes == right.escapes
        && left.needs_fn_mut == right.needs_fn_mut
        && left.mutable == right.mutable
        && left.cloned == right.cloned
        && left.frozen == right.frozen
        && left.materialized == right.materialized
        && left.moved == right.moved
        && left.frame_schedule == right.frame_schedule
        && left.frame_schedule_derivation == right.frame_schedule_derivation
}

fn map_shared_guard_state(
    mut state: Arc<shared_protocol::JetSharedGuardState>,
    path: &[MirFieldId],
    editable: bool,
    span: Span,
) -> Result<Arc<shared_protocol::JetSharedGuardState>, Diagnostic> {
    for field in path {
        let field = i64::try_from(field.0).map_err(|_| {
            mir_error_at(
                "MIR SharedGuard field ID does not fit protocol metadata",
                span,
            )
        })?;
        state = shared_protocol::jet_shared_guard_map(&state, field, editable)
            .map_err(|message| mir_error_at(message, span))?;
    }
    Ok(state)
}

fn tuple_field_ids(
    program: &jet_foundation::MIR::MirProgram,
    result_ty: Option<&MirType>,
    expected: usize,
    span: Span,
) -> Result<Vec<MirFieldId>, Diagnostic> {
    let result_ty =
        result_ty.ok_or_else(|| mir_error_at("MIR tuple result has no type fact", span))?;
    let fields = result_ty
        .tuple_fields()
        .ok_or_else(|| mir_error_at("MIR tuple result is not a tuple type", span))?;
    if fields.len() != expected {
        return Err(mir_error_at(
            "MIR tuple result field count does not match semantic operation",
            span,
        ));
    }
    let owner = result_ty
        .identity
        .ok_or_else(|| mir_error_at("MIR tuple result has no canonical type ID", span))?;
    fields
        .iter()
        .map(|(name, _)| {
            program
                .fields
                .iter()
                .find(|row| row.owner == owner && row.field.name == *name)
                .map(|row| row.id)
                .ok_or_else(|| mir_error_at("MIR tuple field ID is missing", span))
        })
        .collect()
}

fn aggregate_field_ids(
    program: &jet_foundation::MIR::MirProgram,
    result_ty: Option<&MirType>,
    expected: usize,
    span: Span,
) -> Result<Vec<MirFieldId>, Diagnostic> {
    let result_ty =
        result_ty.ok_or_else(|| mir_error_at("MIR aggregate result has no type fact", span))?;
    let type_id = result_ty
        .identity
        .ok_or_else(|| mir_error_at("MIR aggregate result has no canonical type ID", span))?;
    let type_def = program
        .types
        .iter()
        .find(|type_def| type_def.id == type_id)
        .ok_or_else(|| mir_error_at("MIR aggregate result type ID has no type row", span))?;
    let fields = match &type_def.kind {
        MirTypeDefKind::Struct { fields, .. } => fields,
        MirTypeDefKind::Enum { .. } => {
            return Err(mir_error_at(
                "MIR aggregate result type is an enum, not a struct-like aggregate",
                span,
            ))
        }
        MirTypeDefKind::Distinct { .. }
        | MirTypeDefKind::Alias { .. }
        | MirTypeDefKind::UnitFamily { .. } => {
            return Err(mir_error_at(
                "MIR aggregate result type has no canonical field order",
                span,
            ))
        }
    };
    if fields.len() != expected {
        return Err(mir_error_at(
            "MIR aggregate result field count does not match semantic operation",
            span,
        ));
    }
    Ok(fields.iter().map(|field| field.id).collect())
}

#[derive(Debug, Clone)]
enum PlaceUpdate {
    Field {
        parent: RuntimeValue,
        field: MirFieldId,
    },
    Index {
        parent: MirEvalValue,
        index: MirEvalValue,
        kind: MirIndexKind,
        write_call: Option<MirPreludeCallId>,
        location: MirPanicLoc,
        context: Option<MirPanicContext>,
    },
}
#[derive(Debug, Clone)]
struct RuntimeSnapshot {
    frames: Vec<Frame>,
}

#[derive(Debug, Clone)]
struct MirClosure {
    function: MirFunctionId,
    captures: Vec<RuntimeValue>,
    capture_cells: Vec<Option<Rc<RefCell<RuntimeValue>>>>,
    facts: MirCaptureFacts,
}

#[derive(Debug, Clone)]
struct Address {
    frame: usize,
    place: jet_foundation::MIR::MirPlaceId,
    access: MirAccess,
}

#[derive(Debug, Clone)]
struct ValueAlias {
    frame: usize,
    place: MirPlaceId,
    steps: Vec<MoveStep>,
}
#[derive(Clone)]
struct MirSharedStorage {
    protocol: Arc<shared_protocol::JetSharedProtocol>,
    scalar: Option<Arc<shared_protocol::JetSharedAtomic>>,
    revision: Arc<AtomicU64>,
    payload: Rc<RefCell<MirEvalValue>>,
}

impl std::fmt::Debug for MirSharedStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MirSharedStorage")
            .field(
                "scalar_kind",
                &self.scalar.as_ref().map(|scalar| scalar.kind()),
            )
            .finish_non_exhaustive()
    }
}
#[derive(Clone)]
struct MirSharedSnapshot {
    owner: Rc<MirSharedStorage>,
    revision: u64,
    valid: Arc<AtomicBool>,
    consumed: Arc<AtomicBool>,
    value: MirEvalValue,
}

impl std::fmt::Debug for MirSharedSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MirSharedSnapshot")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct MirSharedTransaction {
    transaction: Rc<RefCell<Option<shared_protocol::JetSharedTransaction>>>,
    errors: Rc<RefCell<Option<Diagnostic>>>,
}

impl std::fmt::Debug for MirSharedTransaction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MirSharedTransaction")
            .field("active", &self.transaction.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl MirSharedTransaction {
    fn begin(parent_errors: Option<Rc<RefCell<Option<Diagnostic>>>>) -> Rc<Self> {
        Rc::new(Self {
            transaction: Rc::new(RefCell::new(Some(
                shared_protocol::jet_shared_transaction_begin(),
            ))),
            errors: parent_errors.unwrap_or_else(|| Rc::new(RefCell::new(None))),
        })
    }

    fn with_mut<R>(
        &self,
        span: Span,
        callback: impl FnOnce(&mut shared_protocol::JetSharedTransaction) -> Result<R, Diagnostic>,
    ) -> Result<R, Diagnostic> {
        let mut transaction = self
            .transaction
            .try_borrow_mut()
            .map_err(|_| mir_error_at("MIR Shared transaction is already borrowed", span))?;
        let transaction = transaction
            .as_mut()
            .ok_or_else(|| mir_error_at("MIR Shared transaction is already completed", span))?;
        callback(transaction)
    }

    fn commit(&self, span: Span) -> Result<(), Diagnostic> {
        let transaction = self
            .transaction
            .try_borrow_mut()
            .map_err(|_| mir_error_at("MIR Shared transaction is already borrowed", span))?
            .take();
        if let Some(transaction) = transaction {
            transaction.commit();
        }
        self.errors
            .try_borrow_mut()
            .map_err(|_| mir_error_at("MIR Shared transaction error is already borrowed", span))?
            .take()
            .map_or(Ok(()), Err)
    }

    fn abort(&self) {
        if let Ok(mut transaction) = self.transaction.try_borrow_mut() {
            transaction.take();
        }
    }

    fn record_error(&self, error: Diagnostic) {
        if let Ok(mut errors) = self.errors.try_borrow_mut() {
            if errors.is_none() {
                *errors = Some(error);
            }
        }
    }
}

#[derive(Clone)]
struct MirSharedGuard {
    state: Option<Arc<shared_protocol::JetSharedGuardState>>,
    payload: Rc<RefCell<MirEvalValue>>,
    storage: Option<Rc<MirSharedStorage>>,
}

impl std::fmt::Debug for MirSharedGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MirSharedGuard")
            .field("owned", &self.state.is_some())
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct MirSharedCell {
    guard: Rc<MirSharedGuard>,
    path: Vec<String>,
}

fn mir_shared_scalar_parts(
    value: &MirEvalValue,
) -> Option<(shared_protocol::JetSharedScalarKind, u64)> {
    match value {
        MirEvalValue::Int(value) => {
            Some((shared_protocol::JetSharedScalarKind::I64, *value as u64))
        }
        MirEvalValue::Float { value, f32: true } => Some((
            shared_protocol::JetSharedScalarKind::F32,
            (*value as f32).to_bits() as u64,
        )),
        MirEvalValue::Float { value, f32: false } => {
            Some((shared_protocol::JetSharedScalarKind::F64, value.to_bits()))
        }
        MirEvalValue::Bool(value) => Some((
            shared_protocol::JetSharedScalarKind::Bool,
            u64::from(*value),
        )),
        MirEvalValue::Char(value) => Some((
            shared_protocol::JetSharedScalarKind::Char,
            u64::from(*value as u32),
        )),
        _ => None,
    }
}

fn mir_shared_scalar_bits(
    value: &MirEvalValue,
    kind: shared_protocol::JetSharedScalarKind,
    span: Span,
) -> Result<u64, Diagnostic> {
    let Some((actual, bits)) = mir_shared_scalar_parts(value) else {
        return Err(mir_error_at(
            "MIR Shared scalar carrier received a structured value",
            span,
        ));
    };
    if actual != kind {
        return Err(mir_error_at(
            "MIR Shared scalar carrier value type does not match its cell",
            span,
        ));
    }
    Ok(bits)
}

fn mir_shared_scalar_value(
    kind: shared_protocol::JetSharedScalarKind,
    bits: u64,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    match kind {
        shared_protocol::JetSharedScalarKind::I64 => Ok(MirEvalValue::Int(bits as i64)),
        shared_protocol::JetSharedScalarKind::F32 => Ok(MirEvalValue::Float {
            value: f32::from_bits(bits as u32) as f64,
            f32: true,
        }),
        shared_protocol::JetSharedScalarKind::F64 => Ok(MirEvalValue::Float {
            value: f64::from_bits(bits),
            f32: false,
        }),
        shared_protocol::JetSharedScalarKind::Bool => Ok(MirEvalValue::Bool(bits != 0)),
        shared_protocol::JetSharedScalarKind::Char => {
            shared_protocol::jet_shared_guard_validate_char(bits as i32)
                .map(MirEvalValue::Char)
                .map_err(|message| mir_error_at(message, span))
        }
        _ => Err(mir_error_at(
            "MIR Shared scalar carrier kind is not representable in MIR",
            span,
        )),
    }
}

impl MirSharedStorage {
    fn new(value: MirEvalValue) -> Rc<Self> {
        let scalar = mir_shared_scalar_parts(&value)
            .map(|(kind, bits)| Arc::new(shared_protocol::JetSharedAtomic::new(kind, bits)));
        Rc::new(Self {
            protocol: shared_protocol::JetSharedProtocol::new(),
            scalar,
            revision: Arc::new(AtomicU64::new(0)),
            payload: Rc::new(RefCell::new(value)),
        })
    }

    // A transaction-local payload still uses the canonical JetSharedTransaction
    // as its ownership/commit protocol. This adapter only gives a closure the
    // same SharedCell view without publishing the staged value.
    fn from_payload(payload: Rc<RefCell<MirEvalValue>>) -> Rc<Self> {
        Rc::new(Self {
            protocol: shared_protocol::JetSharedProtocol::new(),
            scalar: None,
            revision: Arc::new(AtomicU64::new(0)),
            payload,
        })
    }

    fn next_revision(&self, span: Span) -> Result<u64, Diagnostic> {
        self.revision
            .load(Ordering::Acquire)
            .checked_add(1)
            .ok_or_else(|| mir_error_at("SharedRevisionError.GenerationExhausted", span))
    }

    /// Publish while the caller holds the canonical participant permit.
    fn publish_at_revision(
        &self,
        value: MirEvalValue,
        next: u64,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if let Some(scalar) = &self.scalar {
            let bits = mir_shared_scalar_bits(&value, scalar.kind(), span)?;
            scalar.store(bits);
        } else {
            *self
                .payload
                .try_borrow_mut()
                .map_err(|_| mir_error_at("MIR Shared payload is already borrowed", span))? = value;
        }
        self.revision.store(next, Ordering::Release);
        Ok(())
    }

    fn read(&self, span: Span) -> Result<MirEvalValue, Diagnostic> {
        if let Some(scalar) = &self.scalar {
            return mir_shared_scalar_value(scalar.kind(), scalar.load(), span);
        }
        let _permit = shared_protocol::jet_shared_acquire(&self.protocol, false, || false)
            .ok_or_else(|| mir_error_at("MIR Shared read protocol acquisition failed", span))?;
        self.payload
            .try_borrow()
            .map(|value| value.clone())
            .map_err(|_| mir_error_at("MIR Shared payload is already mutably borrowed", span))
    }

    fn set(&self, value: MirEvalValue, span: Span) -> Result<(), Diagnostic> {
        let _permit = shared_protocol::jet_shared_acquire(&self.protocol, true, || false)
            .ok_or_else(|| mir_error_at("MIR Shared edit protocol acquisition failed", span))?;
        let next = self.next_revision(span)?;
        if let Some(scalar) = &self.scalar {
            let bits = mir_shared_scalar_bits(&value, scalar.kind(), span)?;
            scalar.store(bits);
        } else {
            *self
                .payload
                .try_borrow_mut()
                .map_err(|_| mir_error_at("MIR Shared payload is already borrowed", span))? = value;
        }
        self.revision.store(next, Ordering::Release);
        Ok(())
    }

    fn replace(&self, value: MirEvalValue, span: Span) -> Result<MirEvalValue, Diagnostic> {
        let _permit = shared_protocol::jet_shared_acquire(&self.protocol, true, || false)
            .ok_or_else(|| mir_error_at("MIR Shared edit protocol acquisition failed", span))?;
        let next = self.next_revision(span)?;
        if let Some(scalar) = &self.scalar {
            let bits = mir_shared_scalar_bits(&value, scalar.kind(), span)?;
            let previous = mir_shared_scalar_value(scalar.kind(), scalar.swap(bits), span)?;
            self.revision.store(next, Ordering::Release);
            return Ok(previous);
        }
        let mut payload = self
            .payload
            .try_borrow_mut()
            .map_err(|_| mir_error_at("MIR Shared payload is already borrowed", span))?;
        let previous = std::mem::replace(&mut *payload, value);
        self.revision.store(next, Ordering::Release);
        Ok(previous)
    }

    fn capture(&self, span: Span) -> Result<(u64, MirEvalValue), Diagnostic> {
        let _permit = shared_protocol::jet_shared_acquire(&self.protocol, false, || false)
            .ok_or_else(|| mir_error_at("MIR Shared capture protocol acquisition failed", span))?;
        let revision = self.revision.load(Ordering::Acquire);
        let value = if let Some(scalar) = &self.scalar {
            mir_shared_scalar_value(scalar.kind(), scalar.load(), span)?
        } else {
            self.payload
                .try_borrow()
                .map(|value| value.clone())
                .map_err(|_| mir_error_at("MIR Shared payload is already mutably borrowed", span))?
        };
        Ok((revision, value))
    }

    fn snapshot(self: &Rc<Self>, span: Span) -> Result<Rc<MirSharedSnapshot>, Diagnostic> {
        let (revision, value) = self.capture(span)?;
        Ok(Rc::new(MirSharedSnapshot {
            owner: self.clone(),
            revision,
            valid: Arc::new(AtomicBool::new(true)),
            consumed: Arc::new(AtomicBool::new(false)),
            value,
        }))
    }
    fn refresh_scalar_shadow(&self, span: Span) -> Result<(), Diagnostic> {
        let Some(scalar) = &self.scalar else {
            return Ok(());
        };
        let value = mir_shared_scalar_value(scalar.kind(), scalar.load(), span)?;
        *self
            .payload
            .try_borrow_mut()
            .map_err(|_| mir_error_at("MIR Shared payload is already borrowed", span))? = value;
        Ok(())
    }

    fn commit_scalar_shadow(&self, span: Span) -> Result<(), Diagnostic> {
        let Some(scalar) = &self.scalar else {
            return Ok(());
        };
        let next = self.next_revision(span)?;
        let value = self
            .payload
            .try_borrow()
            .map_err(|_| mir_error_at("MIR Shared payload is already mutably borrowed", span))?;
        let bits = mir_shared_scalar_bits(&value, scalar.kind(), span)?;
        scalar.store(bits);
        self.revision.store(next, Ordering::Release);
        Ok(())
    }

    fn acquire_guard(
        &self,
        editable: bool,
        span: Span,
    ) -> Result<Arc<shared_protocol::JetSharedGuardState>, Diagnostic> {
        shared_protocol::jet_shared_guard_acquire(&self.protocol, editable, || false)
            .ok_or_else(|| mir_error_at("MIR Shared guard protocol acquisition failed", span))
    }
}
#[derive(Debug)]
struct MirGameCallback {
    callback: Rc<MirClosure>,
    schedule: Option<jet_foundation::ResourceSchedule::JetFrameSchedule>,
    derivation: Option<jet_foundation::Facts::DerivationRef>,
    completion: Option<
        std::sync::Arc<std::sync::Mutex<jet_foundation::ResourceSchedule::JetFrameCompletionState>>,
    >,
}

impl Clone for MirGameCallback {
    fn clone(&self) -> Self {
        Self {
            callback: self.callback.clone(),
            schedule: self.schedule.clone(),
            derivation: self.derivation.clone(),
            completion: self.completion.as_ref().map(|state| {
                std::sync::Arc::new(std::sync::Mutex::new(
                    state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone(),
                ))
            }),
        }
    }
}

#[derive(Debug, Clone)]
struct MirCellLease {
    editable: bool,
}

#[derive(Debug, Clone)]
struct MirCellGuard {
    payload: Rc<RefCell<MirEvalValue>>,
    path: Vec<MirFieldId>,
    lease: Rc<MirCellLease>,
}
#[derive(Debug, Clone)]
struct MirGcRoot {
    value: MirEvalValue,
    edges: Vec<MirEvalValue>,
    edge_slots: BTreeMap<u64, Vec<MirEvalValue>>,
}

const MAX_MIR_GAME_SCENE_CALLBACKS: usize = 256;

#[derive(Debug, Clone)]
struct MirGameScene {
    name: String,
    assets: Vec<(String, String)>,
    bindings: Vec<(String, String)>,
    components: Vec<String>,
    callbacks: Vec<MirGameCallback>,
    dev_session: Option<game_dev_protocol::GameDevSession>,
}

#[derive(Debug, Clone)]
struct MirGameBackend {
    renderer: String,
    audio: String,
    editor: String,
    frame_budget: jet_foundation::Game::JetGameFrameBudget,
}

#[derive(Debug, Clone)]
struct MirGameFrame {
    index: i64,
    pressed: Vec<String>,
}

#[derive(Debug, Clone)]
enum MirGameValue {
    Scene(Rc<RefCell<MirGameScene>>),
    Assets(Rc<RefCell<MirGameScene>>),
    Input(Rc<RefCell<MirGameScene>>),
    Replay(String),
    Backend(Rc<RefCell<MirGameBackend>>),
    Frame(MirGameFrame),
    InputSnapshot(Vec<String>),
    Optional {
        present: bool,
        value: Option<Box<RuntimeValue>>,
    },
}

#[derive(Clone)]
enum MirAtomicCell {
    Bool(Arc<mir_atomic_prelude::JetAtomic<bool>>),
    I32(Arc<mir_atomic_prelude::JetAtomic<i32>>),
    U32(Arc<mir_atomic_prelude::JetAtomic<u32>>),
    I64(Arc<mir_atomic_prelude::JetAtomic<i64>>),
    U64(Arc<mir_atomic_prelude::JetAtomic<u64>>),
    Int(Arc<mir_atomic_prelude::JetAtomic<mir_atomic_prelude::JetAtomicInt>>),
}

impl std::fmt::Debug for MirAtomicCell {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MirAtomicCell")
    }
}

#[derive(Clone, Copy)]
enum MirAtomicKind {
    Bool,
    I32,
    U32,
    I64,
    U64,
    Int,
}

fn mir_atomic_kind(ty: &MirType, span: Span) -> Result<MirAtomicKind, Diagnostic> {
    match ty.kind() {
        MirTypeKind::Bool => Ok(MirAtomicKind::Bool),
        MirTypeKind::Int => Ok(MirAtomicKind::Int),
        MirTypeKind::IntN {
            signed: true,
            bits: 32,
        } => Ok(MirAtomicKind::I32),
        MirTypeKind::IntN {
            signed: false,
            bits: 32,
        } => Ok(MirAtomicKind::U32),
        MirTypeKind::IntN {
            signed: true,
            bits: 64,
        } => Ok(MirAtomicKind::I64),
        MirTypeKind::IntN {
            signed: false,
            bits: 64,
        } => Ok(MirAtomicKind::U64),
        _ => Err(mir_error_at(
            "MIR Atomic receiver inner type is not a supported scalar",
            span,
        )),
    }
}
fn mir_atomic_integer(value: &MirEvalValue, span: Span) -> Result<i128, Diagnostic> {
    match value {
        MirEvalValue::Int(value) => Ok(i128::from(*value)),
        MirEvalValue::BigInt(value) => value.parse::<i128>().map_err(|_| {
            mir_error_at(
                "MIR Atomic integer value is outside the interpreter carrier",
                span,
            )
        }),
        _ => Err(mir_error_at(
            "MIR Atomic scalar value is not an integer",
            span,
        )),
    }
}

enum MirAtomicWord {
    Fixed(u64),
    Exact(jet_foundation::Numeric::JetInt),
}

fn mir_atomic_try_bits(
    value: RuntimeValue,
    kind: MirAtomicKind,
    span: Span,
) -> Result<Result<MirAtomicWord, jet_foundation::Outcome::AllocError>, Diagnostic> {
    if !matches!(kind, MirAtomicKind::Int) {
        return mir_atomic_bits(value, kind, span).map(Ok);
    }
    let value = runtime_to_data(value, span)?;
    let value = match value {
        MirEvalValue::Int(value) => jet_foundation::Numeric::JetInt::try_from_i64(value),
        MirEvalValue::BigInt(value) => {
            let value = jet_foundation::Numeric::CtBigInt::from_str(&value)
                .map_err(|message| mir_error_at(&message, span))?;
            jet_foundation::Numeric::JetInt::try_from_big(value)
        }
        _ => {
            return Err(mir_error_at(
                "MIR Atomic Int value is not an exact integer",
                span,
            ))
        }
    };
    Ok(value.map(MirAtomicWord::Exact))
}

fn mir_atomic_bits(
    value: RuntimeValue,
    kind: MirAtomicKind,
    span: Span,
) -> Result<MirAtomicWord, Diagnostic> {
    let value = runtime_to_data(value, span)?;
    match kind {
        MirAtomicKind::Bool => match value {
            MirEvalValue::Bool(value) => Ok(MirAtomicWord::Fixed(u64::from(value))),
            _ => Err(mir_error_at("MIR Atomic Bool value is not a Bool", span)),
        },
        MirAtomicKind::I32 => {
            let value = i32::try_from(mir_atomic_integer(&value, span)?)
                .map_err(|_| mir_error_at("MIR Atomic I32 value is out of range", span))?;
            Ok(MirAtomicWord::Fixed(u64::from(value as u32)))
        }
        MirAtomicKind::U32 => {
            let value = u32::try_from(mir_atomic_integer(&value, span)?)
                .map_err(|_| mir_error_at("MIR Atomic U32 value is out of range", span))?;
            Ok(MirAtomicWord::Fixed(u64::from(value)))
        }
        MirAtomicKind::I64 => {
            let value = i64::try_from(mir_atomic_integer(&value, span)?)
                .map_err(|_| mir_error_at("MIR Atomic I64 value is out of range", span))?;
            Ok(MirAtomicWord::Fixed(value as u64))
        }
        MirAtomicKind::U64 => Ok(MirAtomicWord::Fixed(
            u64::try_from(mir_atomic_integer(&value, span)?)
                .map_err(|_| mir_error_at("MIR Atomic U64 value is out of range", span))?,
        )),
        MirAtomicKind::Int => {
            let value = match value {
                MirEvalValue::Int(value) => mir_atomic_prelude::jet_std::jet_int_from_i64(value),
                MirEvalValue::BigInt(value) => {
                    mir_atomic_prelude::jet_std::jet_int_from_str(&value)
                        .map_err(|message| mir_error_at(&message, span))?
                }
                _ => {
                    return Err(mir_error_at(
                        "MIR Atomic Int value is not an exact integer",
                        span,
                    ))
                }
            };
            Ok(MirAtomicWord::Exact(value))
        }
    }
}

fn mir_atomic_runtime_value(
    kind: MirAtomicKind,
    word: MirAtomicWord,
    _span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    let value = match word {
        MirAtomicWord::Fixed(bits) => bits,
        MirAtomicWord::Exact(value) => {
            let value = value.to_string_rep();
            return Ok(value
                .parse::<i64>()
                .map(MirEvalValue::Int)
                .unwrap_or(MirEvalValue::BigInt(value)));
        }
    };
    Ok(match kind {
        MirAtomicKind::Bool => MirEvalValue::Bool(value != 0),
        MirAtomicKind::I32 => MirEvalValue::Int(i64::from(value as u32 as i32)),
        MirAtomicKind::U32 => MirEvalValue::Int(i64::from(value as u32)),
        MirAtomicKind::I64 => MirEvalValue::Int(value as i64),
        MirAtomicKind::U64 => i64::try_from(value)
            .map(MirEvalValue::Int)
            .unwrap_or_else(|_| MirEvalValue::BigInt(value.to_string())),
        MirAtomicKind::Int => unreachable!("exact atomic word was not preserved"),
    })
}

fn mir_atomic_cell(kind: MirAtomicKind, word: MirAtomicWord) -> MirAtomicCell {
    match (kind, word) {
        (MirAtomicKind::Bool, MirAtomicWord::Fixed(bits)) => {
            MirAtomicCell::Bool(Arc::new(mir_atomic_prelude::JetAtomic::new(bits != 0)))
        }
        (MirAtomicKind::I32, MirAtomicWord::Fixed(bits)) => MirAtomicCell::I32(Arc::new(
            mir_atomic_prelude::JetAtomic::new(bits as u32 as i32),
        )),
        (MirAtomicKind::U32, MirAtomicWord::Fixed(bits)) => {
            MirAtomicCell::U32(Arc::new(mir_atomic_prelude::JetAtomic::new(bits as u32)))
        }
        (MirAtomicKind::I64, MirAtomicWord::Fixed(bits)) => {
            MirAtomicCell::I64(Arc::new(mir_atomic_prelude::JetAtomic::new(bits as i64)))
        }
        (MirAtomicKind::U64, MirAtomicWord::Fixed(bits)) => {
            MirAtomicCell::U64(Arc::new(mir_atomic_prelude::JetAtomic::new(bits)))
        }
        (MirAtomicKind::Int, MirAtomicWord::Exact(value)) => {
            let raw = value.to_raw();
            let cell = MirAtomicCell::Int(Arc::new(mir_atomic_prelude::JetAtomic::new(raw)));
            drop(value);
            cell
        }
        (MirAtomicKind::Int, MirAtomicWord::Fixed(bits)) => {
            let value = jet_foundation::Numeric::JetInt::from_i64(bits as i64);
            let raw = value.to_raw();
            let cell = MirAtomicCell::Int(Arc::new(mir_atomic_prelude::JetAtomic::new(raw)));
            drop(value);
            cell
        }
        (_, MirAtomicWord::Exact(_)) => unreachable!("fixed atomic lane received exact word"),
    }
}

fn mir_atomic_load(
    cell: &MirAtomicCell,
    kind: MirAtomicKind,
    span: Span,
) -> Result<MirAtomicWord, Diagnostic> {
    match (kind, cell) {
        (MirAtomicKind::Bool, MirAtomicCell::Bool(cell)) => Ok(MirAtomicWord::Fixed(u64::from(
            mir_atomic_prelude::jet_atomic_load(cell.as_ref()),
        ))),
        (MirAtomicKind::I32, MirAtomicCell::I32(cell)) => Ok(MirAtomicWord::Fixed(u64::from(
            mir_atomic_prelude::jet_atomic_load(cell.as_ref()) as u32,
        ))),
        (MirAtomicKind::U32, MirAtomicCell::U32(cell)) => Ok(MirAtomicWord::Fixed(u64::from(
            mir_atomic_prelude::jet_atomic_load(cell.as_ref()),
        ))),
        (MirAtomicKind::I64, MirAtomicCell::I64(cell)) => Ok(MirAtomicWord::Fixed(
            mir_atomic_prelude::jet_atomic_load(cell.as_ref()) as u64,
        )),
        (MirAtomicKind::U64, MirAtomicCell::U64(cell)) => Ok(MirAtomicWord::Fixed(
            mir_atomic_prelude::jet_atomic_load(cell.as_ref()),
        )),
        (MirAtomicKind::Int, MirAtomicCell::Int(cell)) => {
            let value = mir_atomic_prelude::jet_atomic_load(cell.as_ref());
            Ok(MirAtomicWord::Exact(unsafe {
                jet_foundation::Numeric::JetInt::from_raw_owned(value)
            }))
        }
        _ => Err(mir_error_at(
            "MIR Atomic cell type does not match its checked type",
            span,
        )),
    }
}

fn mir_atomic_observe(
    cell: &MirAtomicCell,
    kind: MirAtomicKind,
    span: Span,
) -> Result<MirAtomicWord, Diagnostic> {
    match (kind, cell) {
        (MirAtomicKind::Bool, MirAtomicCell::Bool(cell)) => Ok(MirAtomicWord::Fixed(u64::from(
            mir_atomic_prelude::jet_atomic_observe(cell.as_ref()),
        ))),
        (MirAtomicKind::I32, MirAtomicCell::I32(cell)) => Ok(MirAtomicWord::Fixed(u64::from(
            mir_atomic_prelude::jet_atomic_observe(cell.as_ref()) as u32,
        ))),
        (MirAtomicKind::U32, MirAtomicCell::U32(cell)) => Ok(MirAtomicWord::Fixed(u64::from(
            mir_atomic_prelude::jet_atomic_observe(cell.as_ref()),
        ))),
        (MirAtomicKind::I64, MirAtomicCell::I64(cell)) => Ok(MirAtomicWord::Fixed(
            mir_atomic_prelude::jet_atomic_observe(cell.as_ref()) as u64,
        )),
        (MirAtomicKind::U64, MirAtomicCell::U64(cell)) => Ok(MirAtomicWord::Fixed(
            mir_atomic_prelude::jet_atomic_observe(cell.as_ref()),
        )),
        (MirAtomicKind::Int, MirAtomicCell::Int(cell)) => {
            let value = mir_atomic_prelude::jet_atomic_observe(cell.as_ref());
            Ok(MirAtomicWord::Exact(unsafe {
                jet_foundation::Numeric::JetInt::from_raw_owned(value)
            }))
        }
        _ => Err(mir_error_at(
            "MIR Atomic cell type does not match its checked type",
            span,
        )),
    }
}

fn mir_atomic_store(
    cell: &MirAtomicCell,
    kind: MirAtomicKind,
    word: MirAtomicWord,
    span: Span,
) -> Result<(), Diagnostic> {
    match (kind, cell, word) {
        (MirAtomicKind::Bool, MirAtomicCell::Bool(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_store(cell.as_ref(), bits != 0);
            Ok(())
        }
        (MirAtomicKind::I32, MirAtomicCell::I32(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_store(cell.as_ref(), bits as u32 as i32);
            Ok(())
        }
        (MirAtomicKind::U32, MirAtomicCell::U32(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_store(cell.as_ref(), bits as u32);
            Ok(())
        }
        (MirAtomicKind::I64, MirAtomicCell::I64(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_store(cell.as_ref(), bits as i64);
            Ok(())
        }
        (MirAtomicKind::U64, MirAtomicCell::U64(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_store(cell.as_ref(), bits);
            Ok(())
        }
        (MirAtomicKind::Int, MirAtomicCell::Int(cell), MirAtomicWord::Exact(value)) => {
            let raw = value.to_raw();
            mir_atomic_prelude::jet_atomic_store(cell.as_ref(), raw);
            drop(value);
            Ok(())
        }
        _ => Err(mir_error_at(
            "MIR Atomic cell type does not match its checked type",
            span,
        )),
    }
}

fn mir_atomic_publish(
    cell: &MirAtomicCell,
    kind: MirAtomicKind,
    word: MirAtomicWord,
    span: Span,
) -> Result<(), Diagnostic> {
    match (kind, cell, word) {
        (MirAtomicKind::Bool, MirAtomicCell::Bool(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_publish(cell.as_ref(), bits != 0);
            Ok(())
        }
        (MirAtomicKind::I32, MirAtomicCell::I32(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_publish(cell.as_ref(), bits as u32 as i32);
            Ok(())
        }
        (MirAtomicKind::U32, MirAtomicCell::U32(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_publish(cell.as_ref(), bits as u32);
            Ok(())
        }
        (MirAtomicKind::I64, MirAtomicCell::I64(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_publish(cell.as_ref(), bits as i64);
            Ok(())
        }
        (MirAtomicKind::U64, MirAtomicCell::U64(cell), MirAtomicWord::Fixed(bits)) => {
            mir_atomic_prelude::jet_atomic_publish(cell.as_ref(), bits);
            Ok(())
        }
        (MirAtomicKind::Int, MirAtomicCell::Int(cell), MirAtomicWord::Exact(value)) => {
            let raw = value.to_raw();
            mir_atomic_prelude::jet_atomic_publish(cell.as_ref(), raw);
            drop(value);
            Ok(())
        }
        _ => Err(mir_error_at(
            "MIR Atomic cell type does not match its checked type",
            span,
        )),
    }
}

fn mir_atomic_add(
    cell: &MirAtomicCell,
    kind: MirAtomicKind,
    delta: MirAtomicWord,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    let previous = match (kind, cell, delta) {
        (MirAtomicKind::Bool, _, _) => {
            return Err(mir_error_at(
                "MIR Atomic.add is not supported for Bool",
                span,
            ))
        }
        (MirAtomicKind::I32, MirAtomicCell::I32(cell), MirAtomicWord::Fixed(delta)) => {
            MirAtomicWord::Fixed(mir_atomic_prelude::jet_atomic_add(
                cell.as_ref(),
                delta as u32 as i32,
            ) as u64)
        }
        (MirAtomicKind::U32, MirAtomicCell::U32(cell), MirAtomicWord::Fixed(delta)) => {
            MirAtomicWord::Fixed(u64::from(mir_atomic_prelude::jet_atomic_add(
                cell.as_ref(),
                delta as u32,
            )))
        }
        (MirAtomicKind::I64, MirAtomicCell::I64(cell), MirAtomicWord::Fixed(delta)) => {
            MirAtomicWord::Fixed(
                mir_atomic_prelude::jet_atomic_add(cell.as_ref(), delta as i64) as u64,
            )
        }
        (MirAtomicKind::U64, MirAtomicCell::U64(cell), MirAtomicWord::Fixed(delta)) => {
            MirAtomicWord::Fixed(mir_atomic_prelude::jet_atomic_add(cell.as_ref(), delta))
        }
        (MirAtomicKind::Int, MirAtomicCell::Int(cell), MirAtomicWord::Exact(delta)) => {
            let raw = delta.to_raw();
            let previous = mir_atomic_prelude::jet_atomic_add(cell.as_ref(), raw);
            drop(delta);
            MirAtomicWord::Exact(unsafe {
                jet_foundation::Numeric::JetInt::from_raw_owned(previous)
            })
        }
        _ => {
            return Err(mir_error_at(
                "MIR Atomic cell type does not match its checked type",
                span,
            ))
        }
    };
    mir_atomic_runtime_value(kind, previous, span)
}

fn mir_atomic_try_add(
    cell: &MirAtomicCell,
    kind: MirAtomicKind,
    delta: MirAtomicWord,
    span: Span,
) -> Result<Result<MirEvalValue, jet_foundation::Outcome::AllocError>, Diagnostic> {
    let (MirAtomicKind::Int, MirAtomicCell::Int(cell), MirAtomicWord::Exact(delta)) =
        (kind, cell, delta)
    else {
        return Err(mir_error_at(
            "MIR Atomic.try_add requires an exact Int cell",
            span,
        ));
    };
    let raw = delta.to_raw();
    let result = mir_atomic_prelude::jet_atomic_try_add(cell.as_ref(), raw);
    drop(delta);
    match result {
        Ok(previous) => {
            let previous = unsafe { jet_foundation::Numeric::JetInt::from_raw_owned(previous) };
            mir_atomic_runtime_value(MirAtomicKind::Int, MirAtomicWord::Exact(previous), span)
                .map(Ok)
        }
        Err(error) => Ok(Err(error)),
    }
}

fn mir_atomic_compare_exchange(
    cell: &MirAtomicCell,
    kind: MirAtomicKind,
    expected: MirAtomicWord,
    replacement: MirAtomicWord,
    span: Span,
) -> Result<bool, Diagnostic> {
    match (kind, cell, expected, replacement) {
        (
            MirAtomicKind::Bool,
            MirAtomicCell::Bool(cell),
            MirAtomicWord::Fixed(expected),
            MirAtomicWord::Fixed(replacement),
        ) => Ok(mir_atomic_prelude::jet_atomic_compare_exchange(
            cell.as_ref(),
            expected != 0,
            replacement != 0,
        )),
        (
            MirAtomicKind::I32,
            MirAtomicCell::I32(cell),
            MirAtomicWord::Fixed(expected),
            MirAtomicWord::Fixed(replacement),
        ) => Ok(mir_atomic_prelude::jet_atomic_compare_exchange(
            cell.as_ref(),
            expected as u32 as i32,
            replacement as u32 as i32,
        )),
        (
            MirAtomicKind::U32,
            MirAtomicCell::U32(cell),
            MirAtomicWord::Fixed(expected),
            MirAtomicWord::Fixed(replacement),
        ) => Ok(mir_atomic_prelude::jet_atomic_compare_exchange(
            cell.as_ref(),
            expected as u32,
            replacement as u32,
        )),
        (
            MirAtomicKind::I64,
            MirAtomicCell::I64(cell),
            MirAtomicWord::Fixed(expected),
            MirAtomicWord::Fixed(replacement),
        ) => Ok(mir_atomic_prelude::jet_atomic_compare_exchange(
            cell.as_ref(),
            expected as i64,
            replacement as i64,
        )),
        (
            MirAtomicKind::U64,
            MirAtomicCell::U64(cell),
            MirAtomicWord::Fixed(expected),
            MirAtomicWord::Fixed(replacement),
        ) => Ok(mir_atomic_prelude::jet_atomic_compare_exchange(
            cell.as_ref(),
            expected,
            replacement,
        )),
        (
            MirAtomicKind::Int,
            MirAtomicCell::Int(cell),
            MirAtomicWord::Exact(expected),
            MirAtomicWord::Exact(replacement),
        ) => {
            let expected_raw = expected.to_raw();
            let replacement_raw = replacement.to_raw();
            let exchanged = mir_atomic_prelude::jet_atomic_compare_exchange(
                cell.as_ref(),
                expected_raw,
                replacement_raw,
            );
            drop(expected);
            drop(replacement);
            Ok(exchanged)
        }
        _ => Err(mir_error_at(
            "MIR Atomic cell type does not match its checked type",
            span,
        )),
    }
}
#[derive(Debug)]
struct MirAllocatorState {
    kind: jet_foundation::MIR::MirAllocatorKind,
    closed: bool,
    generation: u64,
    used: usize,
    capacity: Option<usize>,
}

#[derive(Debug, Clone)]
struct MirAllocatorOwner {
    state: Arc<std::sync::Mutex<MirAllocatorState>>,
}

impl MirAllocatorOwner {
    fn new(kind: jet_foundation::MIR::MirAllocatorKind, inline_size: Option<usize>) -> Self {
        Self {
            state: Arc::new(std::sync::Mutex::new(MirAllocatorState {
                kind,
                closed: false,
                generation: 0,
                used: 0,
                capacity: match kind {
                    jet_foundation::MIR::MirAllocatorKind::Fixed => inline_size,
                    _ => None,
                },
            })),
        }
    }
}

/// Layout twin of `Prelude/Core/FixedAllocator.rs` `FixedHeader`, used only for
/// the interpreter's metadata overhead fact. Fit testing itself is
/// `jet_try_alloc_value`.
struct MirFixedHeaderLayout {
    _previous: usize,
    _value_offset: usize,
    _drop_fn: Option<unsafe fn(*mut u8)>,
    _bytes: usize,
}

fn mir_allocator_payload_bytes(value: &MirEvalValue) -> usize {
    match value {
        MirEvalValue::Int(_) | MirEvalValue::Float { .. } | MirEvalValue::BigInt(_) => {
            std::mem::size_of::<jet_foundation::Numeric::JetInt>()
        }
        MirEvalValue::Bool(_) | MirEvalValue::Char(_) | MirEvalValue::Unit => 1,
        _ => std::mem::size_of::<usize>(),
    }
}

fn mir_allocator_try_charge(
    state: &mut MirAllocatorState,
    allocator: &str,
    value: &MirEvalValue,
) -> Option<jet_foundation::Outcome::AllocError> {
    let Some(capacity) = state.capacity else {
        return None;
    };
    let requested = mir_allocator_payload_bytes(value);
    let overhead = match state.kind {
        jet_foundation::MIR::MirAllocatorKind::Fixed => {
            std::mem::size_of::<MirFixedHeaderLayout>()
        }
        _ => 0,
    };
    match jet_foundation::Outcome::jet_try_alloc_value(
        (),
        state.used,
        capacity,
        requested,
        allocator,
        overhead,
    ) {
        Ok((_, used)) => {
            state.used = used;
            None
        }
        Err(error) => Some(error),
    }
}

fn mir_alloc_error_result(error: jet_foundation::Outcome::AllocError) -> RuntimeValue {
    RuntimeValue::Result {
        ok: false,
        value: Box::new(RuntimeValue::Data(MirEvalValue::Struct {
            type_name: crate::Syntax::TYPE_ALLOC_ERROR.to_string(),
            fields: vec![
                (
                    "requested_bytes".to_string(),
                    MirEvalValue::Int(error.requested_bytes),
                ),
                (
                    "allocator".to_string(),
                    MirEvalValue::String(error.allocator),
                ),
            ],
        })),
    }
}

#[derive(Debug, Clone)]
struct MirAllocatorView {
    state: Arc<std::sync::Mutex<MirAllocatorState>>,
    generation: u64,
    value: MirEvalValue,
}

#[derive(Debug, Clone)]
enum RuntimeValue {
    Moved,
    Data(MirEvalValue),
    Stream(MirStreamHandle),
    StreamCursor(Rc<RefCell<MirInterpreterStreamCursor>>),
    Absent {
        element: MirType,
    },
    Closure(Rc<MirClosure>),
    App(crate::Comptime::AppLite::AppHandle),
    /// A private result carrier that preserves opaque runtime values until
    /// result inspection or the entry boundary consumes them.
    Result {
        ok: bool,
        value: Box<RuntimeValue>,
    },
    /// Native Prelude owners and their containing records never become serialized MIR data.
    Ambient(CtValue),
    Atomic(Arc<MirAtomicCell>),
    /// A private interpreter carrier for an opaque native pointer. Clones
    /// share one close-once token rather than copying ownership metadata.
    ForeignHandle {
        token: MirHandleToken,
    },
    /// A private Prelude carrier for a ProcessSpec builder. This raw value is
    ProcessSpec {
        raw: i64,
    },
    DmaTransfer {
        token: i64,
        profile: String,
        channel: String,
        transfer_id: u64,
    },
    Game(MirGameValue),
    Address(Address),
    Aggregate(Vec<(MirFieldId, RuntimeValue)>),
    SharedGuard(Rc<MirSharedGuard>),
    SharedCell(Rc<MirSharedCell>),
    SharedSnapshot(Rc<MirSharedSnapshot>),
    SharedTransaction(Rc<MirSharedTransaction>),
    CellGuard(Rc<MirCellGuard>),
    #[allow(dead_code)]
    GcRoot(Rc<RefCell<MirGcRoot>>),
}
type MirClock = std::sync::Mutex<crate::Comptime::ClockRuntime::jet_std::Clock>;
type MirPool = std::sync::Mutex<crate::Comptime::PoolRuntime::jet_std::JetPool<CtValue>>;
type MirPoolId = crate::Comptime::PoolRuntime::jet_std::JetId<CtValue>;

fn fork_runtime_value_owners(value: &mut RuntimeValue) {
    match value {
        RuntimeValue::Ambient(value) => fork_ct_value_owners(value),
        RuntimeValue::Result { value, .. } => fork_runtime_value_owners(value),
        RuntimeValue::Aggregate(fields) => {
            for (_, value) in fields {
                fork_runtime_value_owners(value);
            }
        }
        _ => {}
    }
}

fn fork_ct_value_owners(value: &mut CtValue) {
    if let Some(clock) = mir_runtime_owner::<MirClock>(value) {
        let clock = clock
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        *value = mir_runtime_owner_value(MirClock::new(clock));
        return;
    }
    match value {
        CtValue::Present(value) | CtValue::Failed(CtReport::Told(value)) => {
            fork_ct_value_owners(value)
        }
        CtValue::List(values) => values.iter_mut().for_each(fork_ct_value_owners),
        CtValue::Map(values) => values.values_mut().for_each(fork_ct_value_owners),
        CtValue::Struct { fields, .. } => {
            for (_, value) in fields {
                fork_ct_value_owners(value);
            }
        }
        CtValue::Enum { args, .. } => {
            for (_, value) in args {
                fork_ct_value_owners(value);
            }
        }
        _ => {}
    }
}

enum MirChannelEndpoint {
    Sender(crate::scheduler::JetSchedulerSender<CtValue>),
    Receiver(crate::scheduler::JetSchedulerChannel<CtValue>),
}

fn mir_runtime_owner_value<T: Send + Sync + 'static>(value: T) -> CtValue {
    CtValue::Closure(Arc::new(ClosureData {
        lambda: Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            result_type: None,
            error_type: None,
            effects: None,
            body: LambdaBody::Block(Vec::new()),
            span: Span::new(0, 0),
            meta: LambdaMeta::default(),
        },
        captured: HashMap::new(),
        return_type: None,
        opaque: Some(CtOpaque::new(value)),
    }))
}

fn mir_runtime_owner<T: 'static>(value: &CtValue) -> Option<&T> {
    let CtValue::Closure(data) = value else {
        return None;
    };
    data.opaque
        .as_ref()
        .and_then(|opaque| opaque.downcast_ref::<T>())
}

fn mir_channel_receiver(
    value: &CtValue,
) -> Option<&crate::scheduler::JetSchedulerChannel<CtValue>> {
    match mir_runtime_owner(value)? {
        MirChannelEndpoint::Receiver(channel) => Some(channel),
        MirChannelEndpoint::Sender(_) => None,
    }
}

fn mir_channel_sender(value: &CtValue) -> Option<&crate::scheduler::JetSchedulerSender<CtValue>> {
    match mir_runtime_owner(value)? {
        MirChannelEndpoint::Sender(sender) => Some(sender),
        MirChannelEndpoint::Receiver(_) => None,
    }
}

fn mir_channel_timer_ms(value: CtValue, span: Span) -> Result<u64, Diagnostic> {
    let value = match value {
        CtValue::Present(value) => return mir_channel_timer_ms(*value, span),
        CtValue::Int(value) => value,
        CtValue::BigInt(value) => value
            .try_i64()
            .ok_or_else(|| mir_error_at("MIR channel timer does not fit Int", span))?,
        CtValue::Struct { fields, .. } => {
            let value = fields
                .into_iter()
                .find_map(|(name, value)| (name == "ns").then_some(value))
                .ok_or_else(|| mir_error_at("MIR channel timer Duration has no ns field", span))?;
            return mir_channel_timer_ms(value, span);
        }
        _ => return Err(mir_error_at("MIR channel timer is not a Duration", span)),
    };
    Ok(crate::scheduler::jet_task_delay_ms_defaulted(
        crate::scheduler::jet_std_time_duration_to_millis(value),
    ))
}
#[derive(Clone, Copy)]
enum MirDmaElement {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
}

fn dma_element_kind(ty: &MirType, span: Span) -> Result<MirDmaElement, Diagnostic> {
    match ty.kind() {
        MirTypeKind::Int => Ok(MirDmaElement::I64),
        MirTypeKind::Float => Ok(MirDmaElement::F64),
        MirTypeKind::Float32 => Ok(MirDmaElement::F32),
        MirTypeKind::IntN { signed, bits } => match (signed, bits) {
            (true, 8) => Ok(MirDmaElement::I8),
            (false, 8) => Ok(MirDmaElement::U8),
            (true, 16) => Ok(MirDmaElement::I16),
            (false, 16) => Ok(MirDmaElement::U16),
            (true, 32) => Ok(MirDmaElement::I32),
            (false, 32) => Ok(MirDmaElement::U32),
            (true, 64) => Ok(MirDmaElement::I64),
            (false, 64) => Ok(MirDmaElement::U64),
            _ => Err(mir_error_at(
                "MIR DMA buffer element is not a fixed-width POD scalar",
                span,
            )),
        },
        MirTypeKind::InlineRange { base, .. } | MirTypeKind::Tagged { inner: base, .. } => {
            dma_element_kind(base, span)
        }
        _ => Err(mir_error_at(
            "MIR DMA buffer element is not a fixed-width POD scalar",
            span,
        )),
    }
}

fn dma_buffer_parts(
    buffer_ty: &MirType,
    span: Span,
) -> Result<(&MirType, Option<usize>), Diagnostic> {
    let (element, fixed_len) = match buffer_ty.kind() {
        MirTypeKind::List(element) => (element.as_ref(), None),
        MirTypeKind::FixedList { elem, len } => {
            let fixed_len = len
                .literal_value()
                .ok_or_else(|| {
                    mir_error_at("MIR DMA fixed buffer length has no static value", span)
                })
                .and_then(|value| {
                    usize::try_from(value)
                        .map_err(|_| mir_error_at("MIR DMA fixed buffer length is too large", span))
                })?;
            (elem.as_ref(), Some(fixed_len))
        }
        MirTypeKind::Tagged { inner, .. } => return dma_buffer_parts(inner, span),
        _ => {
            return Err(mir_error_at(
                "MIR DMA buffer type is not a checked List or FixedList",
                span,
            ))
        }
    };
    Ok((element, fixed_len))
}

fn dma_element_width(element: MirDmaElement) -> usize {
    match element {
        MirDmaElement::I8 | MirDmaElement::U8 => 1,
        MirDmaElement::I16 | MirDmaElement::U16 => 2,
        MirDmaElement::I32 | MirDmaElement::U32 | MirDmaElement::F32 => 4,
        MirDmaElement::I64 | MirDmaElement::U64 | MirDmaElement::F64 => 8,
    }
}

fn dma_u64_value(value: &MirEvalValue, span: Span) -> Result<u64, Diagnostic> {
    match value {
        MirEvalValue::Int(value) => u64::try_from(*value)
            .map_err(|_| mir_error_at("MIR DMA buffer value does not fit U64", span)),
        MirEvalValue::BigInt(value) => {
            let value = mir_bigint(value, span)?
                .try_i128()
                .ok_or_else(|| mir_error_at("MIR DMA buffer value does not fit U64", span))?;
            u64::try_from(value)
                .map_err(|_| mir_error_at("MIR DMA buffer value does not fit U64", span))
        }
        _ => Err(mir_error_at("MIR DMA buffer value is not an integer", span)),
    }
}

fn dma_encode_element(
    value: &MirEvalValue,
    element: MirDmaElement,
    output: &mut Vec<u8>,
    span: Span,
) -> Result<(), Diagnostic> {
    match element {
        MirDmaElement::I8 => output.push(
            i8::try_from(int_value(value.clone(), span)?)
                .map_err(|_| mir_error_at("MIR DMA buffer value does not fit I8", span))?
                as u8,
        ),
        MirDmaElement::U8 => output.push(
            u8::try_from(int_value(value.clone(), span)?)
                .map_err(|_| mir_error_at("MIR DMA buffer value does not fit U8", span))?,
        ),
        MirDmaElement::I16 => output.extend_from_slice(
            &i16::try_from(int_value(value.clone(), span)?)
                .map_err(|_| mir_error_at("MIR DMA buffer value does not fit I16", span))?
                .to_le_bytes(),
        ),
        MirDmaElement::U16 => output.extend_from_slice(
            &u16::try_from(int_value(value.clone(), span)?)
                .map_err(|_| mir_error_at("MIR DMA buffer value does not fit U16", span))?
                .to_le_bytes(),
        ),
        MirDmaElement::I32 => output.extend_from_slice(
            &i32::try_from(int_value(value.clone(), span)?)
                .map_err(|_| mir_error_at("MIR DMA buffer value does not fit I32", span))?
                .to_le_bytes(),
        ),
        MirDmaElement::U32 => output.extend_from_slice(
            &u32::try_from(int_value(value.clone(), span)?)
                .map_err(|_| mir_error_at("MIR DMA buffer value does not fit U32", span))?
                .to_le_bytes(),
        ),
        MirDmaElement::I64 => {
            output.extend_from_slice(&int_value(value.clone(), span)?.to_le_bytes())
        }
        MirDmaElement::U64 => output.extend_from_slice(&dma_u64_value(value, span)?.to_le_bytes()),
        MirDmaElement::F32 | MirDmaElement::F64 => {
            let MirEvalValue::Float { value, .. } = value else {
                return Err(mir_error_at("MIR DMA buffer value is not Float", span));
            };
            if matches!(element, MirDmaElement::F32) {
                output.extend_from_slice(&(*value as f32).to_le_bytes());
            } else {
                output.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    Ok(())
}

fn dma_encode_buffer(
    value: &RuntimeValue,
    buffer_ty: &MirType,
    span: Span,
) -> Result<Box<[u8]>, Diagnostic> {
    let (element_ty, fixed_len) = dma_buffer_parts(buffer_ty, span)?;
    let element = dma_element_kind(element_ty, span)?;
    let RuntimeValue::Data(MirEvalValue::List(values)) = value else {
        return Err(mir_error_at(
            "MIR DMA buffer is not a materialized list carrier",
            span,
        ));
    };
    if fixed_len.is_some_and(|expected| expected != values.len()) {
        return Err(mir_error_at(
            "MIR DMA buffer length disagrees with checked FixedList type",
            span,
        ));
    }
    let width = dma_element_width(element);
    let capacity = values
        .len()
        .checked_mul(width)
        .ok_or_else(|| mir_error_at("MIR DMA buffer byte length overflowed", span))?;
    let mut bytes = Vec::with_capacity(capacity);
    for value in values {
        dma_encode_element(value, element, &mut bytes, span)?;
    }
    Ok(bytes.into_boxed_slice())
}

fn dma_bytes_array<const N: usize>(bytes: &[u8], span: Span) -> Result<[u8; N], Diagnostic> {
    let source = bytes
        .get(..N)
        .ok_or_else(|| mir_error_at("MIR DMA completion buffer is truncated", span))?;
    let mut output = [0_u8; N];
    output.copy_from_slice(source);
    Ok(output)
}

fn dma_decode_element(
    bytes: &[u8],
    element: MirDmaElement,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    Ok(match element {
        MirDmaElement::I8 => MirEvalValue::Int(i64::from(i8::from_le_bytes(dma_bytes_array::<1>(
            bytes, span,
        )?))),
        MirDmaElement::U8 => MirEvalValue::Int(i64::from(u8::from_le_bytes(dma_bytes_array::<1>(
            bytes, span,
        )?))),
        MirDmaElement::I16 => MirEvalValue::Int(i64::from(i16::from_le_bytes(
            dma_bytes_array::<2>(bytes, span)?,
        ))),
        MirDmaElement::U16 => MirEvalValue::Int(i64::from(u16::from_le_bytes(
            dma_bytes_array::<2>(bytes, span)?,
        ))),
        MirDmaElement::I32 => MirEvalValue::Int(i64::from(i32::from_le_bytes(
            dma_bytes_array::<4>(bytes, span)?,
        ))),
        MirDmaElement::U32 => MirEvalValue::Int(i64::from(u32::from_le_bytes(
            dma_bytes_array::<4>(bytes, span)?,
        ))),
        MirDmaElement::I64 => {
            MirEvalValue::Int(i64::from_le_bytes(dma_bytes_array::<8>(bytes, span)?))
        }
        MirDmaElement::U64 => {
            let value = u64::from_le_bytes(dma_bytes_array::<8>(bytes, span)?);
            i64::try_from(value)
                .map(MirEvalValue::Int)
                .unwrap_or_else(|_| MirEvalValue::BigInt(value.to_string()))
        }
        MirDmaElement::F32 => MirEvalValue::Float {
            value: f64::from(f32::from_le_bytes(dma_bytes_array::<4>(bytes, span)?)),
            f32: true,
        },
        MirDmaElement::F64 => MirEvalValue::Float {
            value: f64::from_le_bytes(dma_bytes_array::<8>(bytes, span)?),
            f32: false,
        },
    })
}

fn dma_decode_buffer(
    value: RuntimeValue,
    buffer_ty: &MirType,
    bytes: &[u8],
    span: Span,
) -> Result<RuntimeValue, Diagnostic> {
    let (element_ty, fixed_len) = dma_buffer_parts(buffer_ty, span)?;
    let element = dma_element_kind(element_ty, span)?;
    let width = dma_element_width(element);
    if bytes.len() % width != 0 {
        return Err(mir_error_at(
            "MIR DMA completion byte length is not element-aligned",
            span,
        ));
    }
    let count = bytes.len() / width;
    if fixed_len.is_some_and(|expected| expected != count) {
        return Err(mir_error_at(
            "MIR DMA completion length disagrees with checked FixedList type",
            span,
        ));
    }
    let RuntimeValue::Data(MirEvalValue::List(original)) = value else {
        return Err(mir_error_at(
            "MIR DMA transfer retained a non-list buffer carrier",
            span,
        ));
    };
    if original.len() != count {
        return Err(mir_error_at(
            "MIR DMA completion length disagrees with the checked buffer value",
            span,
        ));
    }
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let start = index * width;
        values.push(dma_decode_element(
            &bytes[start..start + width],
            element,
            span,
        )?);
    }
    Ok(RuntimeValue::Data(MirEvalValue::List(values)))
}
#[derive(Debug)]
enum Action {
    Continue,
    Call {
        function: MirFunctionId,
        args: Vec<RuntimeValue>,
        captures: Vec<RuntimeValue>,
        capture_cells: Vec<Option<Rc<RefCell<RuntimeValue>>>>,
        result: Option<MirValueId>,
    },
    Return(RuntimeValue),
    Suspend(RuntimeValue),
}

#[derive(Clone, Copy)]
enum DropEdge {
    Normal,
    Return,
}

#[derive(Debug, Clone)]
enum PathStep {
    Field(String),
    Index {
        value: MirEvalValue,
        kind: MirIndexKind,
    },
}

#[derive(Debug, Clone)]
enum MoveStep {
    Field {
        field: MirFieldId,
        name: String,
    },
    Index {
        value: MirEvalValue,
        kind: MirIndexKind,
    },
    Deref,
}

fn mir_value_contains_moved(value: &MirEvalValue) -> bool {
    match value {
        MirEvalValue::Moved => true,
        MirEvalValue::List(values) => values.iter().any(mir_value_contains_moved),
        MirEvalValue::Map(values) => values
            .iter()
            .any(|(_, value)| mir_value_contains_moved(value)),
        MirEvalValue::Struct { fields, .. } => fields
            .iter()
            .any(|(_, value)| mir_value_contains_moved(value)),
        MirEvalValue::Enum { args, .. } => args
            .iter()
            .any(|(_, value)| mir_value_contains_moved(value)),
        MirEvalValue::Present(value) | MirEvalValue::FailedTold(value) => {
            mir_value_contains_moved(value)
        }
        MirEvalValue::Closure(closure) => closure.captures.iter().any(mir_value_contains_moved),
        MirEvalValue::Int(_)
        | MirEvalValue::BigInt(_)
        | MirEvalValue::Float { .. }
        | MirEvalValue::Bool(_)
        | MirEvalValue::Char(_)
        | MirEvalValue::String(_)
        | MirEvalValue::Bytes(_)
        | MirEvalValue::Absent { .. }
        | MirEvalValue::Unit => false,
    }
}

fn runtime_contains_moved(value: &RuntimeValue) -> bool {
    match value {
        RuntimeValue::Moved => true,
        RuntimeValue::Data(value) => mir_value_contains_moved(value),
        RuntimeValue::Result { value, .. } => runtime_contains_moved(value),
        RuntimeValue::Aggregate(fields) => fields
            .iter()
            .any(|(_, value)| runtime_contains_moved(value)),
        RuntimeValue::Closure(closure) => closure.captures.iter().any(runtime_contains_moved),
        RuntimeValue::Stream(_)
        | RuntimeValue::StreamCursor(_)
        | RuntimeValue::Absent { .. }
        | RuntimeValue::Atomic(_)
        | RuntimeValue::Address(_)
        | RuntimeValue::SharedGuard(_)
        | RuntimeValue::SharedCell(_)
        | RuntimeValue::SharedSnapshot(_)
        | RuntimeValue::SharedTransaction(_)
        | RuntimeValue::CellGuard(_)
        | RuntimeValue::GcRoot(_)
        | RuntimeValue::App(_) => false,
        RuntimeValue::Ambient(_) => false,
        RuntimeValue::ForeignHandle { token } => token.is_taken(),
        RuntimeValue::ProcessSpec { .. } | RuntimeValue::DmaTransfer { .. } => false,
        RuntimeValue::Game(MirGameValue::Optional { value, .. }) => {
            value.as_deref().is_some_and(runtime_contains_moved)
        }
        RuntimeValue::Game(_) => false,
    }
}

fn take_data_steps(
    data: MirEvalValue,
    steps: &[MoveStep],
    span: Span,
) -> Result<(MirEvalValue, MirEvalValue), Diagnostic> {
    let Some((step, rest)) = steps.split_first() else {
        if mir_value_contains_moved(&data) {
            return Err(mir_error_at("MIR place value was moved", span));
        }
        return Ok((MirEvalValue::Moved, data));
    };
    match step {
        MoveStep::Field { name, .. } => match data {
            MirEvalValue::Struct {
                type_name,
                mut fields,
            } => {
                let Some((_, child)) = fields.iter_mut().find(|(field, _)| field == name) else {
                    return Err(mir_error_at(
                        &format!("MIR field `{name}` is not present"),
                        span,
                    ));
                };
                if matches!(child, MirEvalValue::Moved) {
                    return Err(mir_error_at("MIR place value was moved", span));
                }
                let child = std::mem::replace(child, MirEvalValue::Moved);
                let (replacement, moved) = take_data_steps(child, rest, span)?;
                if let Some((_, child)) = fields.iter_mut().find(|(field, _)| field == name) {
                    *child = replacement;
                }
                Ok((MirEvalValue::Struct { type_name, fields }, moved))
            }
            MirEvalValue::Enum {
                type_name,
                variant,
                mut args,
            } => {
                let slot = if let Ok(index) = name.parse::<usize>() {
                    args.get_mut(index)
                } else {
                    args.iter_mut()
                        .find(|(field, _)| field.as_deref() == Some(name))
                };
                let Some((_, child)) = slot else {
                    return Err(mir_error_at(
                        &format!("MIR enum field `{name}` is not present"),
                        span,
                    ));
                };
                if matches!(child, MirEvalValue::Moved) {
                    return Err(mir_error_at("MIR place value was moved", span));
                }
                let child = std::mem::replace(child, MirEvalValue::Moved);
                let (replacement, moved) = take_data_steps(child, rest, span)?;
                let slot = if let Ok(index) = name.parse::<usize>() {
                    args.get_mut(index)
                } else {
                    args.iter_mut()
                        .find(|(field, _)| field.as_deref() == Some(name))
                };
                if let Some((_, child)) = slot {
                    *child = replacement;
                }
                Ok((
                    MirEvalValue::Enum {
                        type_name,
                        variant,
                        args,
                    },
                    moved,
                ))
            }
            MirEvalValue::Moved => Err(mir_error_at("MIR place value was moved", span)),
            MirEvalValue::List(_)
            | MirEvalValue::Map(_)
            | MirEvalValue::Present(_)
            | MirEvalValue::FailedTold(_)
            | MirEvalValue::Int(_)
            | MirEvalValue::BigInt(_)
            | MirEvalValue::Float { .. }
            | MirEvalValue::Bool(_)
            | MirEvalValue::Char(_)
            | MirEvalValue::String(_)
            | MirEvalValue::Bytes(_)
            | MirEvalValue::Absent { .. }
            | MirEvalValue::Unit
            | MirEvalValue::Closure(_) => Err(mir_error_at(
                &format!("MIR field `{name}` cannot be moved from this value"),
                span,
            )),
        },
        MoveStep::Index { value: index, kind } => match data {
            MirEvalValue::List(mut values) => {
                let index = int_value(index.clone(), span)?;
                let index = usize::try_from(index)
                    .map_err(|_| mir_error_at("MIR list index is negative", span))?;
                let child = values
                    .get_mut(index)
                    .ok_or_else(|| mir_error_at("MIR list index is out of bounds", span))?;
                if matches!(child, MirEvalValue::Moved) {
                    return Err(mir_error_at("MIR place value was moved", span));
                }
                let child = std::mem::replace(child, MirEvalValue::Moved);
                let (replacement, moved) = take_data_steps(child, rest, span)?;
                values[index] = replacement;
                Ok((MirEvalValue::List(values), moved))
            }
            MirEvalValue::Map(mut values) => {
                let key = data_key(index, span)?;
                let Some((_, child)) = values.iter_mut().find(|(candidate, _)| *candidate == key)
                else {
                    return Err(mir_error_at("MIR map key is absent", span));
                };
                if matches!(child, MirEvalValue::Moved) {
                    return Err(mir_error_at("MIR place value was moved", span));
                }
                let child = std::mem::replace(child, MirEvalValue::Moved);
                let (replacement, moved) = take_data_steps(child, rest, span)?;
                if let Some((_, child)) = values.iter_mut().find(|(candidate, _)| *candidate == key)
                {
                    *child = replacement;
                }
                Ok((MirEvalValue::Map(values), moved))
            }
            MirEvalValue::Moved => Err(mir_error_at("MIR place value was moved", span)),
            MirEvalValue::Struct { .. }
            | MirEvalValue::Enum { .. }
            | MirEvalValue::Present(_)
            | MirEvalValue::FailedTold(_)
            | MirEvalValue::Int(_)
            | MirEvalValue::BigInt(_)
            | MirEvalValue::Float { .. }
            | MirEvalValue::Bool(_)
            | MirEvalValue::Char(_)
            | MirEvalValue::String(_)
            | MirEvalValue::Bytes(_)
            | MirEvalValue::Absent { .. }
            | MirEvalValue::Unit
            | MirEvalValue::Closure(_) => Err(mir_error_at(
                &format!("MIR {kind:?} index cannot be moved from this value"),
                span,
            )),
        },
        MoveStep::Deref => Err(mir_error_at(
            "MIR data value cannot be dereferenced for a move",
            span,
        )),
    }
}

fn replace_ct_steps(
    root: &mut CtValue,
    steps: &[MoveStep],
    replacement: CtValue,
    span: Span,
) -> Result<(), Diagnostic> {
    let Some((step, rest)) = steps.split_first() else {
        *root = replacement;
        return Ok(());
    };
    match step {
        MoveStep::Field { name, .. } => match root {
            CtValue::Struct { fields, .. } => {
                let Some((_, value)) = fields.iter_mut().find(|(field, _)| field == name) else {
                    return Err(mir_error_at(
                        &format!("MIR field `{name}` is not present"),
                        span,
                    ));
                };
                if rest.is_empty() {
                    *value = replacement;
                    return Ok(());
                }
                replace_ct_steps(value, rest, replacement, span)
            }
            CtValue::Enum { args, .. } => {
                let slot = if let Ok(index) = name.parse::<usize>() {
                    args.get_mut(index)
                } else {
                    args.iter_mut()
                        .find(|(field, _)| field.as_deref() == Some(name))
                };
                let Some((_, value)) = slot else {
                    return Err(mir_error_at(
                        &format!("MIR enum field `{name}` is not present"),
                        span,
                    ));
                };
                if rest.is_empty() {
                    *value = replacement;
                    return Ok(());
                }
                replace_ct_steps(value, rest, replacement, span)
            }
            CtValue::Failed(CtReport::Told(value)) => {
                replace_ct_steps(value, steps, replacement, span)
            }
            CtValue::Int(_)
            | CtValue::Float(_)
            | CtValue::Bool(_)
            | CtValue::Char(_)
            | CtValue::Str(_)
            | CtValue::BigInt(_)
            | CtValue::Bytes(_)
            | CtValue::List(_)
            | CtValue::Map(_)
            | CtValue::Present(_)
            | CtValue::Failed(CtReport::Clean(_))
            | CtValue::Closure(_)
            | CtValue::Unit => Err(mir_error_at(
                &format!("MIR field `{name}` cannot be reinitialized in this value"),
                span,
            )),
        },
        MoveStep::Index { value: index, kind } => match root {
            CtValue::List(values) => {
                let index = usize::try_from(int_value(index.clone(), span)?)
                    .map_err(|_| mir_error_at("MIR list write index is negative", span))?;
                let value = values
                    .get_mut(index)
                    .ok_or_else(|| mir_error_at("MIR list write index is out of bounds", span))?;
                if rest.is_empty() {
                    *value = replacement;
                    return Ok(());
                }
                replace_ct_steps(value, rest, replacement, span)
            }
            CtValue::Map(values) => {
                let key_value = crate::Comptime::MirBridge::mir_to_ct_value(index.clone(), span)?;
                let key = CtKey::from_value(key_value)
                    .ok_or_else(|| mir_error_at("MIR map write key is not orderable", span))?;
                let Some(value) = values.get_mut(&key) else {
                    if rest.is_empty() {
                        values.insert(key, replacement);
                        return Ok(());
                    }
                    return Err(mir_error_at("MIR map write key is absent", span));
                };
                if rest.is_empty() {
                    *value = replacement;
                    return Ok(());
                }
                replace_ct_steps(value, rest, replacement, span)
            }
            CtValue::Failed(CtReport::Told(value)) => {
                replace_ct_steps(value, steps, replacement, span)
            }
            CtValue::Int(_)
            | CtValue::Float(_)
            | CtValue::Bool(_)
            | CtValue::Char(_)
            | CtValue::Str(_)
            | CtValue::BigInt(_)
            | CtValue::Bytes(_)
            | CtValue::Struct { .. }
            | CtValue::Enum { .. }
            | CtValue::Present(_)
            | CtValue::Failed(CtReport::Clean(_))
            | CtValue::Closure(_)
            | CtValue::Unit => Err(mir_error_at(
                &format!("MIR {kind:?} index cannot be reinitialized in this value"),
                span,
            )),
        },
        MoveStep::Deref => Err(mir_error_at(
            "MIR comptime value cannot be dereferenced for a write",
            span,
        )),
    }
}

fn replace_data_steps(
    root: &mut MirEvalValue,
    steps: &[MoveStep],
    replacement: MirEvalValue,
    span: Span,
) -> Result<(), Diagnostic> {
    let Some((step, rest)) = steps.split_first() else {
        *root = replacement;
        return Ok(());
    };
    match step {
        MoveStep::Field { name, .. } => match root {
            MirEvalValue::Struct { fields, .. } => {
                let Some((_, value)) = fields.iter_mut().find(|(field, _)| field == name) else {
                    return Err(mir_error_at(
                        &format!("MIR field `{name}` is not present"),
                        span,
                    ));
                };
                if rest.is_empty() {
                    *value = replacement;
                    return Ok(());
                }
                if matches!(value, MirEvalValue::Moved) {
                    return Err(mir_error_at("MIR write parent is moved", span));
                }
                replace_data_steps(value, rest, replacement, span)
            }
            MirEvalValue::Enum { args, .. } => {
                let slot = if let Ok(index) = name.parse::<usize>() {
                    args.get_mut(index)
                } else {
                    args.iter_mut()
                        .find(|(field, _)| field.as_deref() == Some(name))
                };
                let Some((_, value)) = slot else {
                    return Err(mir_error_at(
                        &format!("MIR enum field `{name}` is not present"),
                        span,
                    ));
                };
                if rest.is_empty() {
                    *value = replacement;
                    return Ok(());
                }
                if matches!(value, MirEvalValue::Moved) {
                    return Err(mir_error_at("MIR write parent is moved", span));
                }
                replace_data_steps(value, rest, replacement, span)
            }
            MirEvalValue::Moved => Err(mir_error_at("MIR write parent is moved", span)),
            MirEvalValue::List(_)
            | MirEvalValue::Map(_)
            | MirEvalValue::Present(_)
            | MirEvalValue::FailedTold(_)
            | MirEvalValue::Int(_)
            | MirEvalValue::BigInt(_)
            | MirEvalValue::Float { .. }
            | MirEvalValue::Bool(_)
            | MirEvalValue::Char(_)
            | MirEvalValue::String(_)
            | MirEvalValue::Bytes(_)
            | MirEvalValue::Absent { .. }
            | MirEvalValue::Unit
            | MirEvalValue::Closure(_) => Err(mir_error_at(
                &format!("MIR field `{name}` cannot be reinitialized in this value"),
                span,
            )),
        },
        MoveStep::Index { value: index, kind } => match root {
            MirEvalValue::List(values) => {
                let index = usize::try_from(int_value(index.clone(), span)?)
                    .map_err(|_| mir_error_at("MIR list write index is negative", span))?;
                let value = values
                    .get_mut(index)
                    .ok_or_else(|| mir_error_at("MIR list write index is out of bounds", span))?;
                if rest.is_empty() {
                    *value = replacement;
                    return Ok(());
                }
                if matches!(value, MirEvalValue::Moved) {
                    return Err(mir_error_at("MIR write parent is moved", span));
                }
                replace_data_steps(value, rest, replacement, span)
            }
            MirEvalValue::Map(values) => {
                let key = data_key(index, span)?;
                let Some((_, value)) = values.iter_mut().find(|(candidate, _)| candidate == &key)
                else {
                    if rest.is_empty() {
                        values.push((key, replacement));
                        return Ok(());
                    }
                    return Err(mir_error_at("MIR map write key is absent", span));
                };
                if rest.is_empty() {
                    *value = replacement;
                    return Ok(());
                }
                if matches!(value, MirEvalValue::Moved) {
                    return Err(mir_error_at("MIR write parent is moved", span));
                }
                replace_data_steps(value, rest, replacement, span)
            }
            MirEvalValue::Moved => Err(mir_error_at("MIR write parent is moved", span)),
            MirEvalValue::Struct { .. }
            | MirEvalValue::Enum { .. }
            | MirEvalValue::Present(_)
            | MirEvalValue::FailedTold(_)
            | MirEvalValue::Int(_)
            | MirEvalValue::BigInt(_)
            | MirEvalValue::Float { .. }
            | MirEvalValue::Bool(_)
            | MirEvalValue::Char(_)
            | MirEvalValue::String(_)
            | MirEvalValue::Bytes(_)
            | MirEvalValue::Absent { .. }
            | MirEvalValue::Unit
            | MirEvalValue::Closure(_) => Err(mir_error_at(
                &format!("MIR {kind:?} index cannot be reinitialized in this value"),
                span,
            )),
        },
        MoveStep::Deref => Err(mir_error_at(
            "MIR data value cannot be dereferenced for a write",
            span,
        )),
    }
}

fn program_function(
    program: &jet_foundation::MIR::MirProgram,
    id: MirFunctionId,
) -> Result<&MirFunction, Diagnostic> {
    program
        .functions
        .iter()
        .find(|function| function.id == id)
        .ok_or_else(|| mir_error("MIR function ID has no function row", None))
}

fn runtime_address(value: &RuntimeValue, _span: Span) -> Result<Option<Address>, Diagnostic> {
    match value {
        RuntimeValue::Address(address) => Ok(Some(address.clone())),
        RuntimeValue::Moved => Err(mir_error_at("MIR value was moved", _span)),
        RuntimeValue::Data(_)
        | RuntimeValue::Stream(_)
        | RuntimeValue::StreamCursor(_)
        | RuntimeValue::Absent { .. }
        | RuntimeValue::Closure(_)
        | RuntimeValue::App(_)
        | RuntimeValue::Result { .. }
        | RuntimeValue::Ambient(_)
        | RuntimeValue::Atomic(_)
        | RuntimeValue::ForeignHandle { .. }
        | RuntimeValue::ProcessSpec { .. }
        | RuntimeValue::DmaTransfer { .. }
        | RuntimeValue::Game(_)
        | RuntimeValue::Aggregate(_)
        | RuntimeValue::SharedGuard(_)
        | RuntimeValue::SharedCell(_)
        | RuntimeValue::SharedSnapshot(_)
        | RuntimeValue::SharedTransaction(_)
        | RuntimeValue::CellGuard(_)
        | RuntimeValue::GcRoot(_) => Ok(None),
    }
}
fn require_address_access(
    address: &Address,
    required: MirAccess,
    span: Span,
) -> Result<(), Diagnostic> {
    let allowed = match required {
        MirAccess::Read => matches!(address.access, MirAccess::Read | MirAccess::Write),
        MirAccess::Write => matches!(address.access, MirAccess::Write),
        MirAccess::Move => matches!(address.access, MirAccess::Move),
    };
    if allowed {
        Ok(())
    } else {
        Err(mir_error_at(
            &format!(
                "MIR address with {:?} access cannot satisfy {:?} access",
                address.access, required
            ),
            span,
        ))
    }
}

fn shared_payload_read(
    payload: &Rc<RefCell<MirEvalValue>>,
    path: &[String],
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    let root = payload
        .try_borrow()
        .map_err(|_| mir_error_at("MIR SharedGuard payload is already mutably borrowed", span))?;
    let mut value = root.clone();
    for name in path {
        value = match value {
            MirEvalValue::Struct { fields, .. } => fields
                .into_iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value),
            MirEvalValue::Enum { args, .. } => name
                .parse::<usize>()
                .ok()
                .and_then(|index| args.into_iter().nth(index).map(|(_, value)| value)),
            _ => None,
        }
        .ok_or_else(|| {
            mir_error_at(
                &format!("MIR SharedGuard field `{name}` is not present"),
                span,
            )
        })?;
    }
    Ok(value)
}
fn cell_payload_read(
    program: &jet_foundation::MIR::MirProgram,
    payload: &Rc<RefCell<MirEvalValue>>,
    path: &[MirFieldId],
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    let names = path
        .iter()
        .map(|field| {
            field_name(program, *field)
                .ok_or_else(|| mir_error_at("MIR CellGuard field ID is missing", span))
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    shared_payload_read(payload, &names, span)
}

fn write_shared_cell(
    cell: &MirSharedCell,
    value: RuntimeValue,
    span: Span,
) -> Result<(), Diagnostic> {
    let replacement = runtime_to_data(value, span)?;
    if cell.path.is_empty() && cell.guard.state.is_none() {
        return cell
            .guard
            .storage
            .as_ref()
            .ok_or_else(|| mir_error_at("MIR Shared value has no canonical storage", span))?
            .set(replacement, span);
    }
    let state = cell
        .guard
        .state
        .as_ref()
        .ok_or_else(|| mir_error_at("MIR SharedCell has no active protocol state", span))?;
    shared_protocol::jet_shared_guard_require_edit(state)
        .map_err(|message| mir_error_at(message, span))?;
    let mut root = cell
        .guard
        .payload
        .try_borrow_mut()
        .map_err(|_| mir_error_at("MIR SharedGuard payload is already borrowed", span))?;
    let path = cell
        .path
        .iter()
        .cloned()
        .map(PathStep::Field)
        .collect::<Vec<_>>();
    replace_path(&mut root, &path, replacement, span)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum MirTimeKind {
    Duration,
    Instant,
}

fn mir_time_kind(value: &MirEvalValue) -> Option<MirTimeKind> {
    match value {
        MirEvalValue::Struct { type_name, .. } if type_name == crate::Syntax::DURATION_TYPE => {
            Some(MirTimeKind::Duration)
        }
        MirEvalValue::Struct { type_name, .. } if type_name == crate::Syntax::TYPE_INSTANT => {
            Some(MirTimeKind::Instant)
        }
        _ => None,
    }
}

fn mir_time_field(
    value: &MirEvalValue,
    expected_type: &str,
    field: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    let MirEvalValue::Struct { type_name, fields } = value else {
        return Err(mir_error_at("MIR time value is not a struct", span));
    };
    if type_name != expected_type {
        return Err(mir_error_at("MIR time value has the wrong type", span));
    }
    let value = fields
        .iter()
        .find_map(|(name, value)| (name == field).then_some(value))
        .ok_or_else(|| mir_error_at("MIR time value has no nanosecond field", span))?;
    match value {
        MirEvalValue::Int(value) => Ok(*value),
        _ => Err(mir_error_at(
            "MIR time value nanoseconds are not an Int",
            span,
        )),
    }
}

fn mir_time_duration(ns: i64) -> MirEvalValue {
    MirEvalValue::Struct {
        type_name: crate::Syntax::DURATION_TYPE.to_string(),
        fields: vec![("ns".to_string(), MirEvalValue::Int(ns))],
    }
}

fn mir_time_instant(start_ns: i64) -> MirEvalValue {
    MirEvalValue::Struct {
        type_name: crate::Syntax::TYPE_INSTANT.to_string(),
        fields: vec![("start_ns".to_string(), MirEvalValue::Int(start_ns))],
    }
}

fn mir_time_ordering(ordering: i64) -> MirEvalValue {
    MirEvalValue::Enum {
        type_name: crate::Syntax::TYPE_ORDERING.to_string(),
        variant: match ordering {
            value if value < 0 => "Less",
            0 => "Equal",
            _ => "Greater",
        }
        .to_string(),
        args: Vec::new(),
    }
}

fn mir_duration_unit_scale(value: &MirEvalValue) -> Option<i64> {
    match value {
        MirEvalValue::Enum {
            type_name, variant, ..
        } if type_name == crate::Syntax::DURATION_UNIT_TYPE => match variant.as_str() {
            "Nanoseconds" => Some(1),
            "Microseconds" => Some(1_000),
            "Milliseconds" => Some(1_000_000),
            "Seconds" => Some(1_000_000_000),
            "Minutes" => Some(60_000_000_000),
            "Hours" => Some(3_600_000_000_000),
            _ => None,
        },
        MirEvalValue::Int(unit) => match unit {
            0 => Some(1),
            1 => Some(1_000),
            2 => Some(1_000_000),
            3 => Some(1_000_000_000),
            4 => Some(60_000_000_000),
            5 => Some(3_600_000_000_000),
            _ => None,
        },
        _ => None,
    }
}

fn mir_duration_constructor_result(nanoseconds: Option<i64>, reason: &str) -> MirEvalValue {
    match nanoseconds {
        Some(nanoseconds) => MirEvalValue::Present(Box::new(MirEvalValue::Struct {
            type_name: crate::Syntax::DURATION_TYPE.to_string(),
            fields: vec![("ns".to_string(), MirEvalValue::Int(nanoseconds))],
        })),
        None => MirEvalValue::FailedTold(Box::new(MirEvalValue::Struct {
            type_name: crate::Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
            fields: vec![(
                "reason".to_string(),
                MirEvalValue::String(reason.to_string()),
            )],
        })),
    }
}

fn eval_mir_duration_constructor(
    value: &MirEvalValue,
    unit: &MirEvalValue,
    float: bool,
) -> MirEvalValue {
    let reason = if float {
        mir_time_prelude::jet_duration_kernel_float_error_reason()
    } else {
        mir_time_prelude::jet_duration_kernel_int_error_reason()
    };
    let Some(scale) = mir_duration_unit_scale(unit) else {
        return mir_duration_constructor_result(None, reason);
    };
    let nanoseconds = if float {
        match value {
            MirEvalValue::Float { value, .. } => {
                mir_time_prelude::jet_duration_kernel_from_float(*value, scale)
            }
            _ => None,
        }
    } else {
        match value {
            MirEvalValue::Int(value) => {
                mir_time_prelude::jet_duration_kernel_from_int(*value, scale)
            }
            MirEvalValue::BigInt(value) => CtBigInt::from_str(value)
                .ok()
                .and_then(|value| value.try_i64())
                .and_then(|value| mir_time_prelude::jet_duration_kernel_from_int(value, scale)),
            _ => None,
        }
    };
    mir_duration_constructor_result(nanoseconds, reason)
}

fn eval_mir_time_binary(
    op: &MirBinaryOp,
    left: &MirEvalValue,
    right: &MirEvalValue,
    span: Span,
) -> Result<Option<MirEvalValue>, Diagnostic> {
    let (Some(left_kind), Some(right_kind)) = (mir_time_kind(left), mir_time_kind(right)) else {
        return Ok(None);
    };
    let comparison = matches!(
        *op,
        MirBinaryOp::Eq
            | MirBinaryOp::Ne
            | MirBinaryOp::Lt
            | MirBinaryOp::Gt
            | MirBinaryOp::Le
            | MirBinaryOp::Ge
            | MirBinaryOp::Compare
    );
    if comparison && left_kind != right_kind {
        return Ok(None);
    }
    let scalar = |value: &MirEvalValue, kind: MirTimeKind| {
        let (type_name, field) = match kind {
            MirTimeKind::Duration => (crate::Syntax::DURATION_TYPE, "ns"),
            MirTimeKind::Instant => (crate::Syntax::TYPE_INSTANT, "start_ns"),
        };
        mir_time_field(value, type_name, field, span)
    };
    let result = match *op {
        MirBinaryOp::Add => match (left_kind, right_kind) {
            (MirTimeKind::Duration, MirTimeKind::Duration) => {
                mir_time_duration(mir_time_prelude::jet_duration_kernel_add(
                    scalar(left, left_kind)?,
                    scalar(right, right_kind)?,
                ))
            }
            (MirTimeKind::Instant, MirTimeKind::Duration) => {
                mir_time_instant(mir_time_prelude::jet_time_instant_add_duration_ns(
                    scalar(left, left_kind)?,
                    scalar(right, right_kind)?,
                ))
            }
            (MirTimeKind::Duration, MirTimeKind::Instant) => {
                mir_time_instant(mir_time_prelude::jet_time_instant_add_duration_ns(
                    scalar(right, right_kind)?,
                    scalar(left, left_kind)?,
                ))
            }
            _ => return Ok(None),
        },
        MirBinaryOp::Sub => match (left_kind, right_kind) {
            (MirTimeKind::Duration, MirTimeKind::Duration) => {
                mir_time_duration(mir_time_prelude::jet_duration_kernel_sub(
                    scalar(left, left_kind)?,
                    scalar(right, right_kind)?,
                ))
            }
            (MirTimeKind::Instant, MirTimeKind::Duration) => {
                mir_time_instant(mir_time_prelude::jet_time_instant_sub_duration_ns(
                    scalar(left, left_kind)?,
                    scalar(right, right_kind)?,
                ))
            }
            (MirTimeKind::Instant, MirTimeKind::Instant) => {
                mir_time_duration(mir_time_prelude::jet_time_instant_difference_ns(
                    scalar(left, left_kind)?,
                    scalar(right, right_kind)?,
                ))
            }
            _ => return Ok(None),
        },
        MirBinaryOp::Eq
        | MirBinaryOp::Ne
        | MirBinaryOp::Lt
        | MirBinaryOp::Gt
        | MirBinaryOp::Le
        | MirBinaryOp::Ge
        | MirBinaryOp::Compare => {
            let ordering = mir_time_prelude::jet_time_instant_compare(
                scalar(left, left_kind)?,
                scalar(right, right_kind)?,
            );
            match *op {
                MirBinaryOp::Eq => MirEvalValue::Bool(ordering == 0),
                MirBinaryOp::Ne => MirEvalValue::Bool(ordering != 0),
                MirBinaryOp::Lt => MirEvalValue::Bool(ordering < 0),
                MirBinaryOp::Gt => MirEvalValue::Bool(ordering > 0),
                MirBinaryOp::Le => MirEvalValue::Bool(ordering <= 0),
                MirBinaryOp::Ge => MirEvalValue::Bool(ordering >= 0),
                MirBinaryOp::Compare => mir_time_ordering(ordering),
                _ => unreachable!("time comparison operator guard"),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

fn eval_mir_binary(
    op: MirBinaryOp,
    left: MirEvalValue,
    right: MirEvalValue,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    if let Some(value) = eval_mir_time_binary(&op, &left, &right, span)? {
        return Ok(value);
    }
    match (op, left, right) {
        (MirBinaryOp::Add, MirEvalValue::String(left), MirEvalValue::String(right)) => {
            Ok(MirEvalValue::String(format!("{left}{right}")))
        }
        (op @ (MirBinaryOp::Add | MirBinaryOp::Sub | MirBinaryOp::Mul), left, right)
            if mir_exact_big(&left).is_some() && mir_exact_big(&right).is_some() =>
        {
            let left = mir_exact_big(&left).expect("whole-number arithmetic operand");
            let right = mir_exact_big(&right).expect("whole-number arithmetic operand");
            let value = match op {
                MirBinaryOp::Add => left.add(&right),
                MirBinaryOp::Sub => left.sub(&right),
                MirBinaryOp::Mul => left.mul(&right),
                _ => unreachable!("whole-number arithmetic guard"),
            };
            Ok(mir_exact_int_value(value))
        }
        (MirBinaryOp::Div, left, right)
            if mir_exact_big(&left).is_some() && mir_exact_big(&right).is_some() =>
        {
            let left = mir_exact_big(&left).expect("whole-number dividend");
            let right = mir_exact_big(&right).expect("whole-number divisor");
            let fraction = jet_foundation::Numeric::CtFraction::from_bigints(left, right)
                .ok_or_else(|| mir_error_at("division by zero", span))?;
            crate::Comptime::MirBridge::ct_to_mir_value(fraction.to_value(), span)
        }
        (MirBinaryOp::FloorDiv, left, right)
            if mir_exact_big(&left).is_some() && mir_exact_big(&right).is_some() =>
        {
            let left = mir_exact_big(&left).expect("whole-number dividend");
            let right = mir_exact_big(&right).expect("whole-number divisor");
            let value = left
                .div_rem_euclid(&right)
                .ok_or_else(|| mir_error_at("division by zero", span))?
                .0;
            Ok(mir_exact_int_value(value))
        }
        (MirBinaryOp::Mod, left, right)
            if mir_exact_big(&left).is_some() && mir_exact_big(&right).is_some() =>
        {
            let left = mir_exact_big(&left).expect("whole-number dividend");
            let right = mir_exact_big(&right).expect("whole-number divisor");
            let (_, mut remainder) = left
                .div_rem(&right)
                .ok_or_else(|| mir_error_at("division by zero", span))?;
            if !remainder.is_zero() && mir_big_negative(&left) != mir_big_negative(&right) {
                remainder = remainder.add(&right);
            }
            Ok(mir_exact_int_value(remainder))
        }
        (MirBinaryOp::Rem, left, right)
            if mir_exact_big(&left).is_some() && mir_exact_big(&right).is_some() =>
        {
            let left = mir_exact_big(&left).expect("whole-number dividend");
            let right = mir_exact_big(&right).expect("whole-number divisor");
            let remainder = left
                .div_rem(&right)
                .ok_or_else(|| mir_error_at("division by zero", span))?
                .1;
            Ok(mir_exact_int_value(remainder))
        }
        (MirBinaryOp::Pow, left, right)
            if mir_exact_big(&left).is_some() && mir_exact_big(&right).is_some() =>
        {
            let left = mir_exact_big(&left).expect("whole-number base");
            let right = mir_exact_big(&right).expect("whole-number exponent");
            let exponent = right
                .try_i64()
                .ok_or_else(|| mir_error_at("integer exponent does not fit Int", span))?;
            if exponent < 0 {
                return Err(mir_error_at("integer exponent cannot be negative", span));
            }
            let value = left
                .pow(&CtBigInt::from_int(exponent))
                .ok_or_else(|| mir_error_at("integer exponent cannot be negative", span))?;
            Ok(mir_exact_int_value(value))
        }
        (op @ (MirBinaryOp::BitAnd | MirBinaryOp::BitOr | MirBinaryOp::BitXor), left, right)
            if mir_exact_big(&left).is_some() && mir_exact_big(&right).is_some() =>
        {
            let left = mir_exact_big(&left).expect("whole-number bit operand");
            let right = mir_exact_big(&right).expect("whole-number bit operand");
            let value = match op {
                MirBinaryOp::BitAnd => left.bit_and(&right),
                MirBinaryOp::BitOr => left.bit_or(&right),
                MirBinaryOp::BitXor => left.bit_xor(&right),
                _ => unreachable!("whole-number bitwise guard"),
            };
            Ok(mir_exact_int_value(value))
        }
        (op @ (MirBinaryOp::Shl | MirBinaryOp::Shr), left, right)
            if mir_exact_big(&left).is_some() && mir_exact_big(&right).is_some() =>
        {
            let left = mir_exact_big(&left).expect("whole-number shift operand");
            let right = mir_exact_big(&right).expect("whole-number shift count");
            let value = match op {
                MirBinaryOp::Shl => left.shl(&right),
                MirBinaryOp::Shr => left.shr(&right),
                _ => unreachable!("whole-number shift guard"),
            }
            .ok_or_else(|| mir_error_at("invalid or overflowing shift count", span))?;
            Ok(mir_exact_int_value(value))
        }
        (
            op @ (MirBinaryOp::Add
            | MirBinaryOp::Sub
            | MirBinaryOp::Mul
            | MirBinaryOp::Div
            | MirBinaryOp::Pow),
            MirEvalValue::Float {
                value: left,
                f32: left_f32,
            },
            MirEvalValue::Float {
                value: right,
                f32: right_f32,
            },
        ) => {
            let left = MirFloat::literal(left, left_f32);
            let right = MirFloat::literal(right, right_f32);
            left.binop(op, right)
                .map(mir_float_data)
                .ok_or_else(|| mir_error_at("MIR primitive operation mixes float widths", span))
        }
        (MirBinaryOp::Eq, left, right) => Ok(MirEvalValue::Bool(mir_values_equal(&left, &right))),
        (MirBinaryOp::Ne, left, right) => Ok(MirEvalValue::Bool(!mir_values_equal(&left, &right))),
        (
            op @ (MirBinaryOp::Lt | MirBinaryOp::Gt | MirBinaryOp::Le | MirBinaryOp::Ge),
            left,
            right,
        ) => {
            if mir_numeric_unordered(&left, &right) {
                return Ok(MirEvalValue::Bool(false));
            }
            let ordering = mir_compare(&left, &right, span)?;
            Ok(MirEvalValue::Bool(match op {
                MirBinaryOp::Lt => ordering.is_lt(),
                MirBinaryOp::Gt => ordering.is_gt(),
                MirBinaryOp::Le => ordering.is_le(),
                MirBinaryOp::Ge => ordering.is_ge(),
                _ => unreachable!("relational operator guard"),
            }))
        }
        (MirBinaryOp::Compare, left, right) => {
            let ordering = mir_compare(&left, &right, span)?;
            Ok(MirEvalValue::Enum {
                type_name: crate::Syntax::TYPE_ORDERING.to_string(),
                variant: match ordering {
                    std::cmp::Ordering::Less => "Less",
                    std::cmp::Ordering::Equal => "Equal",
                    std::cmp::Ordering::Greater => "Greater",
                }
                .to_string(),
                args: Vec::new(),
            })
        }
        (MirBinaryOp::And, MirEvalValue::Bool(left), MirEvalValue::Bool(right)) => {
            Ok(MirEvalValue::Bool(left && right))
        }
        (MirBinaryOp::Or, MirEvalValue::Bool(left), MirEvalValue::Bool(right)) => {
            Ok(MirEvalValue::Bool(left || right))
        }
        (op, _, _) => Err(mir_error_at(
            &format!(
                "MIR primitive operator `{}` has incompatible operands",
                op.spell()
            ),
            span,
        )),
    }
}

fn mir_exact_big(value: &MirEvalValue) -> Option<CtBigInt> {
    match value {
        MirEvalValue::Int(value) => Some(CtBigInt::from_int(*value)),
        MirEvalValue::BigInt(value) => CtBigInt::from_str(value).ok(),
        _ => None,
    }
}

fn mir_exact_int_value(value: CtBigInt) -> MirEvalValue {
    let lower = CtBigInt::from_int(-(1_i64 << 62));
    let upper = CtBigInt::from_int((1_i64 << 62) - 1);
    if value.compare(&lower).is_ge() && value.compare(&upper).is_le() {
        MirEvalValue::Int(value.try_i64().expect("exact Int range fits i64"))
    } else {
        MirEvalValue::BigInt(value.to_string_rep())
    }
}
fn mir_numeric_cast_value(
    value: MirEvalValue,
    source: &MirType,
    target: &MirType,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    if target.is_integer() && target.fixed_int().is_none() {
        let value = match value {
            MirEvalValue::Int(value) => source
                .fixed_int()
                .map(|(signed, _)| {
                    if signed {
                        CtBigInt::from_int(value)
                    } else {
                        CtBigInt::from_u64(value as u64)
                    }
                })
                .unwrap_or_else(|| CtBigInt::from_int(value)),
            MirEvalValue::BigInt(value) => mir_bigint(&value, span)?,
            _ => {
                return Err(mir_error_at(
                    "MIR numeric cast to exact Int requires an integer",
                    span,
                ));
            }
        };
        return Ok(mir_exact_int_value(value));
    }

    if target.is_float() {
        let value = match value {
            MirEvalValue::Int(value) => source
                .fixed_int()
                .map(|(signed, _)| {
                    if signed {
                        value as f64
                    } else {
                        (value as u64) as f64
                    }
                })
                .unwrap_or(value as f64),
            MirEvalValue::BigInt(value) => {
                let value = mir_bigint(&value, span)?;
                let text = value.to_string_rep();
                text.parse::<f64>().unwrap_or_else(|_| {
                    if text.starts_with('-') {
                        f64::NEG_INFINITY
                    } else {
                        f64::INFINITY
                    }
                })
            }
            MirEvalValue::Float { value, .. } => value,
            _ => {
                return Err(mir_error_at(
                    "MIR numeric cast to Float requires a numeric operand",
                    span,
                ));
            }
        };
        return Ok(if matches!(target.kind(), MirTypeKind::Float32) {
            mir_float_data(MirFloat::f32(value as f32))
        } else {
            mir_float_data(MirFloat::f64(value))
        });
    }

    Ok(value)
}

fn mir_numeric_unordered(left: &MirEvalValue, right: &MirEvalValue) -> bool {
    match (left, right) {
        (MirEvalValue::Float { value: left, .. }, MirEvalValue::Float { value: right, .. }) => {
            left.is_nan() || right.is_nan()
        }
        (MirEvalValue::Int(_), MirEvalValue::Float { value, .. })
        | (MirEvalValue::Float { value, .. }, MirEvalValue::Int(_)) => value.is_nan(),
        _ => false,
    }
}

fn mir_big_negative(value: &CtBigInt) -> bool {
    value.to_string_rep().starts_with('-')
}

fn mir_compare(
    left: &MirEvalValue,
    right: &MirEvalValue,
    span: Span,
) -> Result<std::cmp::Ordering, Diagnostic> {
    if let (
        MirEvalValue::Float {
            value: left,
            f32: left_f32,
        },
        MirEvalValue::Float {
            value: right,
            f32: right_f32,
        },
    ) = (left, right)
    {
        let left = MirFloat::literal(*left, *left_f32);
        let right = MirFloat::literal(*right, *right_f32);
        return left
            .partial_cmp(right)
            .ok_or_else(|| mir_error_at("comparing NaN", span));
    }
    if let Some(ordering) = numeric_compare(left, right) {
        return Ok(ordering);
    }
    match (left, right) {
        (MirEvalValue::Bool(left), MirEvalValue::Bool(right)) => Ok(left.cmp(right)),
        (MirEvalValue::Char(left), MirEvalValue::Char(right)) => Ok(left.cmp(right)),
        (MirEvalValue::String(left), MirEvalValue::String(right)) => Ok(left.cmp(right)),
        (MirEvalValue::List(left), MirEvalValue::List(right)) => {
            for (left, right) in left.iter().zip(right) {
                let ordering = mir_compare(left, right, span)?;
                if !ordering.is_eq() {
                    return Ok(ordering);
                }
            }
            Ok(left.len().cmp(&right.len()))
        }
        _ => Err(mir_error_at(
            "MIR primitive comparison is not defined for these values",
            span,
        )),
    }
}

fn mir_constant_to_runtime(constant: &MirConstant, span: Span) -> Result<RuntimeValue, Diagnostic> {
    match constant {
        MirConstant::Failed(MirConstReport::Clean(element)) => Ok(RuntimeValue::Absent {
            element: element.clone(),
        }),
        MirConstant::Int { .. }
        | MirConstant::Float { .. }
        | MirConstant::Bool(_)
        | MirConstant::Char(_)
        | MirConstant::String(_)
        | MirConstant::Bytes(_)
        | MirConstant::Unit
        | MirConstant::BigInt(_)
        | MirConstant::List(_)
        | MirConstant::Map(_)
        | MirConstant::Struct { .. }
        | MirConstant::Enum { .. }
        | MirConstant::Present(_)
        | MirConstant::Failed(MirConstReport::Told(_)) => {
            Ok(RuntimeValue::Data(mir_constant_to_value(constant, span)?))
        }
    }
}

fn mir_constant_to_value(constant: &MirConstant, span: Span) -> Result<MirEvalValue, Diagnostic> {
    match constant {
        MirConstant::Int {
            value,
            width,
            spelling,
        } => {
            let _ = (width, spelling);
            Ok(MirEvalValue::Int(*value))
        }
        MirConstant::Float {
            value,
            f32,
            spelling,
        } => {
            let _ = spelling;
            Ok(MirEvalValue::Float {
                value: *value,
                f32: *f32,
            })
        }
        MirConstant::Bool(value) => Ok(MirEvalValue::Bool(*value)),
        MirConstant::Char(value) => Ok(MirEvalValue::Char(*value)),
        MirConstant::String(value) => Ok(MirEvalValue::String(value.clone())),
        MirConstant::Bytes(value) => Ok(MirEvalValue::Bytes(value.clone())),
        MirConstant::Unit => Ok(MirEvalValue::Unit),
        MirConstant::BigInt(value) => {
            let _ = mir_bigint(value, span)?;
            Ok(MirEvalValue::BigInt(value.clone()))
        }
        MirConstant::List(values) => values
            .iter()
            .map(|value| mir_constant_to_value(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(MirEvalValue::List),
        MirConstant::Map(values) => values
            .iter()
            .map(|(key, value)| Ok((key.clone(), mir_constant_to_value(value, span)?)))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(MirEvalValue::Map),
        MirConstant::Struct { type_name, fields } => fields
            .iter()
            .map(|(name, value)| Ok((name.clone(), mir_constant_to_value(value, span)?)))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|fields| MirEvalValue::Struct {
                type_name: type_name.clone(),
                fields,
            }),
        MirConstant::Enum {
            type_name,
            variant,
            args,
        } => args
            .iter()
            .map(|(name, value)| Ok((name.clone(), mir_constant_to_value(value, span)?)))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|args| MirEvalValue::Enum {
                type_name: type_name.clone(),
                variant: variant.clone(),
                args,
            }),
        MirConstant::Present(value) => Ok(MirEvalValue::Present(Box::new(mir_constant_to_value(
            value, span,
        )?))),
        MirConstant::Failed(MirConstReport::Told(value)) => Ok(MirEvalValue::FailedTold(Box::new(
            mir_constant_to_value(value, span)?,
        ))),
        MirConstant::Failed(MirConstReport::Clean(element)) => Ok(MirEvalValue::Absent {
            element: element.clone(),
        }),
    }
}
fn game_string(value: RuntimeValue, span: Span) -> Result<String, Diagnostic> {
    match value {
        RuntimeValue::Data(MirEvalValue::String(value)) => Ok(value),
        _ => Err(mir_error_at(
            "MIR game operation requires a String value",
            span,
        )),
    }
}

fn game_int(value: &RuntimeValue, span: Span) -> Result<i64, Diagnostic> {
    match value {
        RuntimeValue::Data(MirEvalValue::Int(value)) => Ok(*value),
        _ => Err(mir_error_at(
            "MIR game frame budget requires an Int value",
            span,
        )),
    }
}

fn game_scene(value: RuntimeValue, span: Span) -> Result<Rc<RefCell<MirGameScene>>, Diagnostic> {
    match value {
        RuntimeValue::Game(MirGameValue::Scene(scene)) => Ok(scene),
        _ => Err(mir_error_at(
            "MIR game operation requires a GameScene",
            span,
        )),
    }
}
fn task_failure_value(reason: String) -> MirEvalValue {
    MirEvalValue::Enum {
        type_name: crate::Syntax::TYPE_TASK_FAILURE.to_string(),
        variant: "Panicked".to_string(),
        args: vec![(None, MirEvalValue::String(reason))],
    }
}

fn task_failure_value_from_scheduler(failure: crate::scheduler::JetTaskFailure) -> MirEvalValue {
    match failure {
        crate::scheduler::JetTaskFailure::Cancelled => MirEvalValue::Enum {
            type_name: crate::Syntax::TYPE_TASK_FAILURE.to_string(),
            variant: "Cancelled".to_string(),
            args: Vec::new(),
        },
        crate::scheduler::JetTaskFailure::DeadlineBlown => MirEvalValue::Enum {
            type_name: crate::Syntax::TYPE_TASK_FAILURE.to_string(),
            variant: "DeadlineBlown".to_string(),
            args: Vec::new(),
        },
        crate::scheduler::JetTaskFailure::Panicked(reason) => task_failure_value(reason),
    }
}

fn task_failure_result(failure: crate::scheduler::JetTaskFailure) -> RuntimeValue {
    RuntimeValue::Result {
        ok: false,
        value: Box::new(RuntimeValue::Data(task_failure_value_from_scheduler(
            failure,
        ))),
    }
}

fn game_assets(value: RuntimeValue, span: Span) -> Result<Rc<RefCell<MirGameScene>>, Diagnostic> {
    match value {
        RuntimeValue::Game(MirGameValue::Assets(scene)) => Ok(scene),
        _ => Err(mir_error_at(
            "MIR game asset operation requires GameAssets",
            span,
        )),
    }
}

fn game_input_scene(
    value: RuntimeValue,
    span: Span,
) -> Result<Rc<RefCell<MirGameScene>>, Diagnostic> {
    match value {
        RuntimeValue::Game(MirGameValue::Input(scene)) => Ok(scene),
        _ => Err(mir_error_at(
            "MIR game input operation requires GameInput",
            span,
        )),
    }
}

fn game_input_snapshot(value: RuntimeValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    match value {
        RuntimeValue::Game(MirGameValue::InputSnapshot(input)) => Ok(input),
        _ => Err(mir_error_at(
            "MIR game input operation requires a GameInputSnapshot",
            span,
        )),
    }
}

fn game_option(
    value: Option<RuntimeValue>,
    span: Span,
) -> Result<Option<RuntimeValue>, Diagnostic> {
    match value {
        None | Some(RuntimeValue::Absent { .. }) => Ok(None),
        Some(RuntimeValue::Data(MirEvalValue::Present(value))) => {
            Ok(Some(RuntimeValue::Data(*value)))
        }
        Some(RuntimeValue::Game(MirGameValue::Optional { present, value })) => {
            if present {
                value
                    .map(|value| *value)
                    .ok_or_else(|| mir_error_at("MIR game option has no present value", span))
                    .map(Some)
            } else {
                Ok(None)
            }
        }
        Some(RuntimeValue::Data(MirEvalValue::FailedTold(_))) => Err(mir_error_at(
            "MIR game option carries a failure outcome",
            span,
        )),
        Some(value) => Ok(Some(value)),
    }
}

fn game_replay(value: &RuntimeValue, span: Span) -> Result<String, Diagnostic> {
    match value {
        RuntimeValue::Game(MirGameValue::Replay(path)) => Ok(path.clone()),
        _ => Err(mir_error_at(
            "MIR game.run replay is not a GameReplay",
            span,
        )),
    }
}

fn game_backend(
    value: RuntimeValue,
    span: Span,
) -> Result<Rc<RefCell<MirGameBackend>>, Diagnostic> {
    match value {
        RuntimeValue::Game(MirGameValue::Backend(backend)) => Ok(backend),
        _ => Err(mir_error_at(
            "MIR game backend value is not a GameBackend",
            span,
        )),
    }
}

fn ct_contains_runtime_owner(value: &CtValue) -> bool {
    match value {
        CtValue::Closure(_) => true,
        CtValue::Present(value) => ct_contains_runtime_owner(value),
        CtValue::Failed(_) => value.told_report().is_some_and(ct_contains_runtime_owner),
        CtValue::List(values) => values.iter().any(ct_contains_runtime_owner),
        CtValue::Map(values) => values.values().any(ct_contains_runtime_owner),
        CtValue::Struct { fields, .. } => fields
            .iter()
            .any(|(_, value)| ct_contains_runtime_owner(value)),
        CtValue::Enum { args, .. } => args
            .iter()
            .any(|(_, value)| ct_contains_runtime_owner(value)),
        _ => false,
    }
}

fn runtime_from_ct(value: CtValue, span: Span) -> Result<RuntimeValue, Diagnostic> {
    if ct_contains_runtime_owner(&value) {
        Ok(RuntimeValue::Ambient(value))
    } else {
        crate::Comptime::MirBridge::ct_to_mir_value(value, span).map(RuntimeValue::Data)
    }
}

fn runtime_to_data(value: RuntimeValue, span: Span) -> Result<MirEvalValue, Diagnostic> {
    match value {
        RuntimeValue::Moved => Err(mir_error_at("MIR value was moved", span)),
        RuntimeValue::Data(value) if mir_value_contains_moved(&value) => {
            Err(mir_error_at("MIR value was moved", span))
        }
        RuntimeValue::Data(value) => Ok(value),
        RuntimeValue::Absent { element } => Ok(MirEvalValue::Absent { element }),
        RuntimeValue::Ambient(value) => {
            let Some(view) = mir_runtime_owner::<MirAllocatorView>(&value) else {
                return Err(mir_error_at(
                    "MIR value contains a private adapter handle and is not materializable",
                    span,
                ));
            };
            let state = view
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.closed || state.generation != view.generation {
                return Err(mir_error_at("MIR allocator view is no longer live", span));
            }
            Ok(view.value.clone())
        }
        RuntimeValue::Closure(closure) => closure
            .captures
            .iter()
            .enumerate()
            .map(|(slot, value)| {
                closure
                    .capture_cells
                    .get(slot)
                    .and_then(|cell| cell.as_ref())
                    .map(|cell| cell.borrow().clone())
                    .unwrap_or_else(|| value.clone())
            })
            .map(|value| runtime_to_data(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|captures| {
                MirEvalValue::Closure(MirClosureValue {
                    function: closure.function,
                    captures,
                })
            }),
        RuntimeValue::Result { ok, value } => runtime_to_data(*value, span).map(|value| {
            if ok {
                MirEvalValue::Present(Box::new(value))
            } else {
                MirEvalValue::FailedTold(Box::new(value))
            }
        }),
        RuntimeValue::Stream(_)
        | RuntimeValue::App(_)
        | RuntimeValue::Atomic(_)
        | RuntimeValue::StreamCursor(_)
        | RuntimeValue::ForeignHandle { .. }
        | RuntimeValue::ProcessSpec { .. }
        | RuntimeValue::DmaTransfer { .. }
        | RuntimeValue::Game(_)
        | RuntimeValue::SharedCell(_)
        | RuntimeValue::SharedGuard(_)
        | RuntimeValue::SharedSnapshot(_)
        | RuntimeValue::SharedTransaction(_)
        | RuntimeValue::CellGuard(_)
        | RuntimeValue::Aggregate(_)
        | RuntimeValue::Address(_)
        | RuntimeValue::GcRoot(_) => Err(mir_error_at(
            "MIR value contains a private adapter handle and is not materializable",
            span,
        )),
    }
}

fn runtime_to_state_data(value: RuntimeValue, span: Span) -> Option<MirEvalValue> {
    runtime_to_data(value, span).ok()
}

fn text_hole_kind(kind: MirTextHoleKind) -> JetTextHoleKind {
    match kind {
        MirTextHoleKind::Text => JetTextHoleKind::Text,
        MirTextHoleKind::Int => JetTextHoleKind::Int,
        MirTextHoleKind::Float => JetTextHoleKind::Float,
        MirTextHoleKind::Bool => JetTextHoleKind::Bool,
        MirTextHoleKind::InlineRange { lo, hi } => JetTextHoleKind::InlineRange { lo, hi },
    }
}

fn marshal_pattern_capture(
    value: MirEvalValue,
    ty: &MirTypeKind,
    span: Span,
) -> Result<MirEvalValue, Diagnostic> {
    match ty {
        MirTypeKind::String => match value {
            MirEvalValue::String(_) => Ok(value),
            _ => Err(mir_error_at("MIR text pattern capture is not String", span)),
        },
        MirTypeKind::Char => match value {
            MirEvalValue::String(value) => {
                let mut chars = value.chars();
                let Some(character) = chars.next() else {
                    return Err(mir_error_at(
                        "MIR text pattern capture is not one Char",
                        span,
                    ));
                };
                if chars.next().is_some() {
                    return Err(mir_error_at(
                        "MIR text pattern capture is not one Char",
                        span,
                    ));
                }
                Ok(MirEvalValue::Char(character))
            }
            MirEvalValue::Char(_) => Ok(value),
            _ => Err(mir_error_at("MIR text pattern capture is not Char", span)),
        },
        MirTypeKind::Int | MirTypeKind::IntN { .. } | MirTypeKind::InlineRange { .. } => {
            match value {
                MirEvalValue::Int(_) => Ok(value),
                _ => Err(mir_error_at("MIR pattern capture is not an integer", span)),
            }
        }
        MirTypeKind::Float | MirTypeKind::Measure(_) => match value {
            MirEvalValue::Float { value, .. } => Ok(MirEvalValue::Float { value, f32: false }),
            _ => Err(mir_error_at("MIR pattern capture is not Float", span)),
        },
        MirTypeKind::Float32 => match value {
            MirEvalValue::Float { value, .. } => Ok(MirEvalValue::Float {
                value: value as f32 as f64,
                f32: true,
            }),
            _ => Err(mir_error_at("MIR pattern capture is not Float32", span)),
        },
        MirTypeKind::Bool => match value {
            MirEvalValue::Bool(_) => Ok(value),
            _ => Err(mir_error_at("MIR pattern capture is not Bool", span)),
        },
        MirTypeKind::List(inner)
            if matches!(
                inner.kind(),
                MirTypeKind::IntN {
                    signed: false,
                    bits: 8
                }
            ) =>
        {
            match value {
                MirEvalValue::Bytes(_) => Ok(value),
                _ => Err(mir_error_at("MIR binary rest capture is not [U8]", span)),
            }
        }
        MirTypeKind::List(_)
        | MirTypeKind::Map { .. }
        | MirTypeKind::Shared(_)
        | MirTypeKind::Option(_)
        | MirTypeKind::Result { .. }
        | MirTypeKind::Fn(_)
        | MirTypeKind::SendFn { .. }
        | MirTypeKind::Apply { .. }
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::Tuple(_)
        | MirTypeKind::FixedList { .. }
        | MirTypeKind::Tagged { .. }
        | MirTypeKind::Union(_)
        | MirTypeKind::Quantity { .. } => Err(mir_error_at(
            "MIR pattern capture result type is not sema-admitted",
            span,
        )),
    }
}
fn pattern_capture_value(capture: JetPatternCapture) -> MirEvalValue {
    match capture {
        JetPatternCapture::Text(value) => MirEvalValue::String(value),
        JetPatternCapture::Int(value) => MirEvalValue::Int(value),
        JetPatternCapture::Float(value) => MirEvalValue::Float { value, f32: false },
        JetPatternCapture::Bool(value) => MirEvalValue::Bool(value),
        JetPatternCapture::Bytes(value) => MirEvalValue::Bytes(value),
    }
}

fn pattern_match_carrier(
    captures: Option<Vec<JetPatternCapture>>,
    result_ty: Option<&MirType>,
    span: Span,
) -> Result<RuntimeValue, Diagnostic> {
    let result_ty =
        result_ty.ok_or_else(|| mir_error_at("MIR pattern match has no result type fact", span))?;
    let tuple_ty = result_ty
        .option_inner()
        .ok_or_else(|| mir_error_at("MIR pattern match result is not an Option", span))?;
    let tuple_fields = tuple_ty.tuple_fields().ok_or_else(|| {
        mir_error_at(
            "MIR pattern match result Option payload is not a typed tuple",
            span,
        )
    })?;
    match captures {
        Some(captures) => {
            if captures.len() != tuple_fields.len() {
                return Err(mir_error_at(
                    &format!(
                        "MIR pattern capture count {} disagrees with checked tuple width {}",
                        captures.len(),
                        tuple_fields.len()
                    ),
                    span,
                ));
            }
            let fields = tuple_fields
                .iter()
                .zip(captures)
                .map(|((name, ty), capture)| {
                    Ok((
                        name.clone(),
                        marshal_pattern_capture(pattern_capture_value(capture), ty.kind(), span)?,
                    ))
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            Ok(RuntimeValue::Data(MirEvalValue::Present(Box::new(
                MirEvalValue::Struct {
                    type_name: tuple_ty.display_name(),
                    fields,
                },
            ))))
        }
        None => Ok(RuntimeValue::Absent {
            element: tuple_ty.clone(),
        }),
    }
}

fn mir_optional_value(value: Option<MirEvalValue>) -> MirEvalValue {
    value.unwrap_or(MirEvalValue::Unit)
}

fn int_value(value: MirEvalValue, span: Span) -> Result<i64, Diagnostic> {
    match value {
        MirEvalValue::Int(value) => Ok(value),
        MirEvalValue::BigInt(value) => mir_bigint(&value, span)?
            .try_i64()
            .ok_or_else(|| mir_error_at("MIR index does not fit Int", span)),
        _ => Err(mir_error_at("MIR index requires Int", span)),
    }
}
fn data_key(value: &MirEvalValue, span: Span) -> Result<MirEvalKey, Diagnostic> {
    match value {
        MirEvalValue::Int(value) => Ok(MirEvalKey::Int(*value)),
        MirEvalValue::String(value) => Ok(MirEvalKey::String(value.clone())),
        MirEvalValue::Bool(value) => Ok(MirEvalKey::Bool(*value)),
        MirEvalValue::Char(value) => Ok(MirEvalKey::Char(*value)),
        MirEvalValue::Struct { type_name, fields } => Ok(MirEvalKey::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| Ok((name.clone(), data_key(value, span)?)))
                .collect::<Result<Vec<_>, Diagnostic>>()?,
        }),
        MirEvalValue::Enum {
            type_name,
            variant,
            args,
        } if args.is_empty() => Ok(MirEvalKey::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
        }),
        _ => Err(mir_error_at("MIR map key is not orderable", span)),
    }
}

fn project_game_field(
    value: MirGameValue,
    name: &str,
    span: Span,
) -> Result<RuntimeValue, Diagnostic> {
    match value {
        MirGameValue::Scene(scene) => match name {
            "name" => {
                let scene = scene
                    .try_borrow()
                    .map_err(|_| mir_error_at("MIR game scene is already borrowed", span))?;
                Ok(RuntimeValue::Data(MirEvalValue::String(scene.name.clone())))
            }
            "assets" | "user_assets" => Ok(RuntimeValue::Game(MirGameValue::Assets(scene))),
            "input" | "user_input" => Ok(RuntimeValue::Game(MirGameValue::Input(scene))),
            _ => Err(mir_error_at(
                &format!("MIR game scene field `{name}` is not present"),
                span,
            )),
        },
        MirGameValue::Frame(frame) => match name {
            "index" | "user_index" => Ok(RuntimeValue::Data(MirEvalValue::Int(frame.index))),
            "input" | "user_input" => Ok(RuntimeValue::Game(MirGameValue::InputSnapshot(
                frame.pressed,
            ))),
            _ => Err(mir_error_at(
                &format!("MIR game frame field `{name}` is not present"),
                span,
            )),
        },
        MirGameValue::Replay(path) if name == "path" => {
            Ok(RuntimeValue::Data(MirEvalValue::String(path)))
        }
        MirGameValue::Backend(backend) => {
            let backend = backend
                .try_borrow()
                .map_err(|_| mir_error_at("MIR game backend is already borrowed", span))?;
            let value = match name {
                "renderer" => backend.renderer.clone(),
                "audio" => backend.audio.clone(),
                "editor" => backend.editor.clone(),
                _ => {
                    return Err(mir_error_at(
                        &format!("MIR game backend field `{name}` is not present"),
                        span,
                    ))
                }
            };
            Ok(RuntimeValue::Data(MirEvalValue::String(value)))
        }
        _ => Err(mir_error_at(
            &format!("MIR game field `{name}` is not present"),
            span,
        )),
    }
}

fn project_field(value: RuntimeValue, name: &str, span: Span) -> Result<RuntimeValue, Diagnostic> {
    if let RuntimeValue::Game(value) = value {
        return project_game_field(value, name, span);
    }
    if let RuntimeValue::Ambient(value) = value {
        let projected = match value {
            CtValue::Struct { fields, .. } => fields
                .into_iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value),
            CtValue::Enum { args, .. } => name
                .parse::<usize>()
                .ok()
                .and_then(|index| args.into_iter().nth(index).map(|(_, value)| value)),
            _ => None,
        };
        return projected
            .ok_or_else(|| mir_error_at(&format!("MIR field `{name}` is not present"), span))
            .and_then(|value| runtime_from_ct(value, span));
    }
    if name == "stdin" {
        if let RuntimeValue::ForeignHandle { token } = &value {
            return Ok(RuntimeValue::ForeignHandle {
                token: token.clone(),
            });
        }
    }
    let value = runtime_to_data(value, span)?;
    let projected = match value {
        MirEvalValue::Struct { fields, .. } => fields
            .into_iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value),
        MirEvalValue::Enum { args, .. } => name
            .parse::<usize>()
            .ok()
            .and_then(|index| args.into_iter().nth(index).map(|(_, value)| value)),
        _ => None,
    };
    projected
        .map(RuntimeValue::Data)
        .ok_or_else(|| mir_error_at(&format!("MIR field `{name}` is not present"), span))
}

fn project_field_id(
    program: &jet_foundation::MIR::MirProgram,
    value: RuntimeValue,
    field: MirFieldId,
    span: Span,
) -> Result<RuntimeValue, Diagnostic> {
    if let RuntimeValue::Aggregate(fields) = value {
        return fields
            .into_iter()
            .find(|(candidate, _)| *candidate == field)
            .map(|(_, value)| value)
            .ok_or_else(|| mir_error_at("MIR aggregate field ID is not present", span));
    }
    let name =
        field_name(program, field).ok_or_else(|| mir_error_at("MIR field ID is missing", span))?;
    project_field(value, &name, span)
}

fn replace_runtime_field(
    program: &jet_foundation::MIR::MirProgram,
    parent: RuntimeValue,
    field: MirFieldId,
    replacement: RuntimeValue,
    span: Span,
) -> Result<RuntimeValue, Diagnostic> {
    match parent {
        RuntimeValue::Aggregate(mut fields) => {
            let Some((_, value)) = fields.iter_mut().find(|(candidate, _)| *candidate == field)
            else {
                return Err(mir_error_at("MIR aggregate field ID is not present", span));
            };
            *value = replacement;
            Ok(RuntimeValue::Aggregate(fields))
        }
        RuntimeValue::Data(mut data) => {
            let name = field_name(program, field)
                .ok_or_else(|| mir_error_at("MIR field ID is missing", span))?;
            let replacement = runtime_to_data(replacement, span)?;
            replace_path(&mut data, &[PathStep::Field(name)], replacement, span)?;
            Ok(RuntimeValue::Data(data))
        }
        RuntimeValue::SharedCell(cell) => {
            let name = field_name(program, field)
                .ok_or_else(|| mir_error_at("MIR field ID is missing", span))?;
            let mut nested = cell.as_ref().clone();
            nested.path.push(name);
            write_shared_cell(&nested, replacement, span)?;
            Ok(RuntimeValue::SharedCell(cell))
        }
        RuntimeValue::Moved => Err(mir_error_at("MIR field write targets a moved value", span)),
        RuntimeValue::Stream(_) | RuntimeValue::StreamCursor(_) => Err(mir_error_at(
            "MIR field write cannot project a stream handle",
            span,
        )),
        RuntimeValue::ForeignHandle { .. }
        | RuntimeValue::ProcessSpec { .. }
        | RuntimeValue::DmaTransfer { .. } => Err(mir_error_at(
            "MIR field write cannot project an opaque native handle",
            span,
        )),
        RuntimeValue::Atomic(_) => Err(mir_error_at(
            "MIR field write cannot project an Atomic value",
            span,
        )),
        RuntimeValue::Game(_) => Err(mir_error_at(
            "MIR field write cannot project a game handle",
            span,
        )),
        RuntimeValue::App(_) | RuntimeValue::Result { .. } => Err(mir_error_at(
            "MIR field write cannot project an App handle",
            span,
        )),
        RuntimeValue::Ambient(_) => Err(mir_error_at(
            "MIR field write cannot project a native Prelude owner",
            span,
        )),
        RuntimeValue::Address(_) => Err(mir_error_at(
            "MIR field write cannot project an unresolved address",
            span,
        )),
        RuntimeValue::Absent { .. } => Err(mir_error_at(
            "MIR field write cannot project an Absent option value",
            span,
        )),
        RuntimeValue::SharedGuard(_) => Err(mir_error_at(
            "MIR field write requires dereferencing the SharedGuard value",
            span,
        )),
        RuntimeValue::SharedSnapshot(_) => Err(mir_error_at(
            "MIR field write cannot project a SharedSnapshot value",
            span,
        )),
        RuntimeValue::SharedTransaction(_) => Err(mir_error_at(
            "MIR field write cannot project a SharedTransaction value",
            span,
        )),
        RuntimeValue::CellGuard(_) => Err(mir_error_at(
            "MIR field write requires dereferencing the CellGuard value",
            span,
        )),
        RuntimeValue::GcRoot(_) => Err(mir_error_at(
            "MIR field write cannot project a GC root handle",
            span,
        )),
        RuntimeValue::Closure(_) => Err(mir_error_at(
            "MIR field write cannot project a closure value",
            span,
        )),
    }
}

fn replace_path(
    root: &mut MirEvalValue,
    path: &[PathStep],
    replacement: MirEvalValue,
    span: Span,
) -> Result<(), Diagnostic> {
    let Some(step) = path.first() else {
        *root = replacement;
        return Ok(());
    };
    match step {
        PathStep::Field(name) => {
            let MirEvalValue::Struct { fields, .. } = root else {
                return Err(mir_error_at("MIR field write base is not a struct", span));
            };
            let (_, value) = fields
                .iter_mut()
                .find(|(field, _)| field == name)
                .ok_or_else(|| mir_error_at(&format!("MIR field `{name}` is not present"), span))?;
            replace_path(value, &path[1..], replacement, span)
        }
        PathStep::Index { value, kind } => match (kind, root) {
            (&MirIndexKind::List | &MirIndexKind::FixedListProof, MirEvalValue::List(items)) => {
                let index = usize::try_from(int_value(value.clone(), span)?)
                    .map_err(|_| mir_error_at("MIR list write index is negative", span))?;
                let item = items
                    .get_mut(index)
                    .ok_or_else(|| mir_error_at("MIR list write index is out of bounds", span))?;
                replace_path(item, &path[1..], replacement, span)
            }
            (&MirIndexKind::Map, MirEvalValue::Map(items)) => {
                let key = data_key(value, span)?;
                if let Some((_, item)) = items.iter_mut().find(|(existing, _)| existing == &key) {
                    if path.len() == 1 {
                        *item = replacement;
                        return Ok(());
                    }
                    return replace_path(item, &path[1..], replacement, span);
                }
                if path.len() == 1 {
                    items.push((key, replacement));
                    return Ok(());
                }
                Err(mir_error_at("MIR map write key is absent", span))
            }
            (kind, _) => Err(mir_error_at(
                &format!("MIR {kind:?} indexed write base does not match checked container"),
                span,
            )),
        },
    }
}

fn variant_name_matches(actual: &str, expected: &str) -> bool {
    actual == expected
        || actual.rsplit("::").next().unwrap_or(actual)
            == expected.rsplit("::").next().unwrap_or(expected)
}

fn layout_compare(
    op: jet_foundation::MIR::MirLayoutCompareOp,
    left: &MirEvalValue,
    right: &MirEvalValue,
) -> bool {
    match op {
        jet_foundation::MIR::MirLayoutCompareOp::Equal => mir_values_equal(left, right),
        jet_foundation::MIR::MirLayoutCompareOp::LessEqual => {
            numeric_compare(left, right).is_some_and(|ordering| ordering.is_le())
        }
        jet_foundation::MIR::MirLayoutCompareOp::GreaterEqual => {
            numeric_compare(left, right).is_some_and(|ordering| ordering.is_ge())
        }
    }
}

fn numeric_compare(left: &MirEvalValue, right: &MirEvalValue) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (MirEvalValue::Int(left), MirEvalValue::Int(right)) => Some(left.cmp(right)),
        (
            MirEvalValue::Float {
                value: left,
                f32: left_f32,
            },
            MirEvalValue::Float {
                value: right,
                f32: right_f32,
            },
        ) if left_f32 == right_f32 => {
            MirFloat::literal(*left, *left_f32).partial_cmp(MirFloat::literal(*right, *right_f32))
        }
        (MirEvalValue::Int(left), MirEvalValue::Float { value: right, .. }) => {
            (*left as f64).partial_cmp(right)
        }
        (MirEvalValue::Float { value: left, .. }, MirEvalValue::Int(right)) => {
            left.partial_cmp(&(*right as f64))
        }
        (MirEvalValue::BigInt(left), MirEvalValue::BigInt(right)) => Some(
            mir_bigint(left, Span::new(0, 0))
                .ok()?
                .compare(&mir_bigint(right, Span::new(0, 0)).ok()?),
        ),
        (MirEvalValue::Int(left), MirEvalValue::BigInt(right)) => {
            Some(CtBigInt::from_int(*left).compare(&mir_bigint(right, Span::new(0, 0)).ok()?))
        }
        (MirEvalValue::BigInt(left), MirEvalValue::Int(right)) => Some(
            mir_bigint(left, Span::new(0, 0))
                .ok()?
                .compare(&CtBigInt::from_int(*right)),
        ),
        _ => None,
    }
}
fn mir_values_equal(left: &MirEvalValue, right: &MirEvalValue) -> bool {
    fn bytes_equal_list(bytes: &[u8], values: &[MirEvalValue]) -> bool {
        bytes.len() == values.len()
            && bytes.iter().zip(values).all(|(&byte, value)| {
                matches!(value, MirEvalValue::Int(number) if *number == i64::from(byte))
            })
    }

    match (left, right) {
        (MirEvalValue::Bytes(left), MirEvalValue::Bytes(right)) => left == right,
        (MirEvalValue::Bytes(bytes), MirEvalValue::List(values))
        | (MirEvalValue::List(values), MirEvalValue::Bytes(bytes)) => {
            bytes_equal_list(bytes, values)
        }
        (MirEvalValue::BigInt(left), MirEvalValue::BigInt(right)) => {
            match (
                mir_bigint(left, Span::new(0, 0)),
                mir_bigint(right, Span::new(0, 0)),
            ) {
                (Ok(left), Ok(right)) => left.compare(&right).is_eq(),
                _ => false,
            }
        }
        (MirEvalValue::Int(left), MirEvalValue::BigInt(right))
        | (MirEvalValue::BigInt(right), MirEvalValue::Int(left)) => {
            mir_bigint(right, Span::new(0, 0))
                .map(|right| CtBigInt::from_int(*left).compare(&right).is_eq())
                .unwrap_or(false)
        }
        _ => left == right,
    }
}

fn field_name(program: &jet_foundation::MIR::MirProgram, id: MirFieldId) -> Option<String> {
    // Checked type definitions are the source-of-truth projection rows. The
    // flat field table is only a compatibility fallback for older MIR.
    for ty in &program.types {
        match &ty.kind {
            MirTypeDefKind::Struct { fields, .. } => {
                if let Some(field) = fields.iter().find(|field| field.id == id) {
                    return Some(field.name.clone());
                }
            }
            MirTypeDefKind::Enum { variants, .. } => {
                for variant in variants {
                    if let jet_foundation::MIR::MirVariantPayload::Named(fields) = &variant.payload
                    {
                        if let Some(field) = fields.iter().find(|field| field.id == id) {
                            return Some(field.name.clone());
                        }
                    }
                }
            }
            MirTypeDefKind::Distinct { .. }
            | MirTypeDefKind::Alias { .. }
            | MirTypeDefKind::UnitFamily { .. } => {}
        }
    }
    program
        .fields
        .iter()
        .find(|row| row.id == id)
        .map(|row| row.field.name.clone())
}

fn checked_nominal_name(
    program: &jet_foundation::MIR::MirProgram,
    id: jet_foundation::MIR::MirTypeId,
    name: &str,
) -> Option<String> {
    program
        .types
        .iter()
        .find(|def| def.id == id || def.key == name || def.name == name)
        .map(|def| def.name.clone())
}

fn mir_type_display_name(program: &jet_foundation::MIR::MirProgram, ty: &MirType) -> String {
    match ty.kind() {
        MirTypeKind::Apply { name, args } => {
            let head = checked_nominal_name(program, name.id, &name.name)
                .unwrap_or_else(|| name.name.clone());
            if args.is_empty() {
                head
            } else {
                format!(
                    "{head}<{}>",
                    args.iter()
                        .map(|arg| mir_type_display_name(program, arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        MirTypeKind::Tagged { inner, .. } => mir_type_display_name(program, inner),
        _ => ty.display_name(),
    }
}

fn validate_enum_args(
    program: &jet_foundation::MIR::MirProgram,
    function_id: MirFunctionId,
    type_id: jet_foundation::MIR::MirTypeId,
    variant: &str,
    args: &[MirEnumArg],
    span: Span,
) -> Result<(), Diagnostic> {
    let type_def = program
        .types
        .iter()
        .find(|type_def| type_def.id == type_id)
        .ok_or_else(|| mir_error_at("MIR enum type ID has no type row", span))?;
    let payload = match &type_def.kind {
        MirTypeDefKind::Enum { variants, .. } => variants
            .iter()
            .find(|candidate| candidate.name == variant)
            .map(|candidate| &candidate.payload)
            .ok_or_else(|| mir_error_at("MIR enum variant has no canonical row", span))?,
        MirTypeDefKind::Struct { .. }
        | MirTypeDefKind::Distinct { .. }
        | MirTypeDefKind::Alias { .. }
        | MirTypeDefKind::UnitFamily { .. } => {
            return Err(mir_error_at(
                "MIR enum construction owner is not an enum type",
                span,
            ));
        }
    };
    let function = program_function(program, function_id)?;
    let validate_value = |arg: &MirEnumArg, expected: &MirType| -> Result<(), Diagnostic> {
        let _boxed = arg.boxed;
        let actual = function
            .values
            .iter()
            .find(|(value, ..)| *value == arg.value)
            .map(|(_, ty, ..)| ty)
            .ok_or_else(|| mir_error_at("MIR enum argument value has no type row", span))?;
        if !actual.same_checked_type(expected) && !actual.kind.same_identity(&expected.kind) {
            return Err(mir_error_at(
                "MIR enum argument type does not match its variant payload",
                span,
            ));
        }
        Ok(())
    };
    match payload {
        MirVariantPayload::Unit => {
            if !args.is_empty() {
                return Err(mir_error_at(
                    "MIR unit enum variant has payload arguments",
                    span,
                ));
            }
        }
        MirVariantPayload::Single(expected) => {
            let [arg] = args else {
                return Err(mir_error_at(
                    "MIR single-payload enum variant arity mismatch",
                    span,
                ));
            };
            if arg.field.is_some() {
                return Err(mir_error_at(
                    "MIR single-payload enum argument must be positional",
                    span,
                ));
            }
            validate_value(arg, expected)?;
        }
        MirVariantPayload::Named(fields) => {
            if args.len() != fields.len() {
                return Err(mir_error_at(
                    "MIR named enum variant payload arity mismatch",
                    span,
                ));
            }
            for (arg, field) in args.iter().zip(fields) {
                if arg.field != Some(field.id) {
                    return Err(mir_error_at(
                        "MIR named enum arguments are not in canonical field order",
                        span,
                    ));
                }
                validate_value(arg, &field.ty)?;
            }
        }
    }
    Ok(())
}

fn type_instance_name(
    program: &jet_foundation::MIR::MirProgram,
    id: jet_foundation::MIR::MirTypeId,
    span: Span,
) -> Result<String, Diagnostic> {
    let ty = program
        .type_instances
        .iter()
        .find(|ty| ty.identity == Some(id))
        .ok_or_else(|| mir_error_at("MIR type instance ID has no canonical row", span))?;
    Ok(mir_type_display_name(program, ty))
}

fn frame_from_state(
    program: &jet_foundation::MIR::MirProgram,
    state: &MirFrameState,
) -> Result<Frame, Diagnostic> {
    let function = program_function(program, state.function)?;
    if let Some(identity) = &state.identity {
        if identity.function != state.function || identity.block != Some(state.block) {
            return Err(mir_error(
                "MIR frame identity does not match its function/block state",
                None,
            ));
        }
        let expected = program
            .frame_identity(
                Some(identity.execution.artifact.artifact),
                state.function,
                Some(state.block),
                identity.sequence,
            )
            .map_err(|error| {
                mir_error(&format!("MIR frame identity unavailable: {error}"), None)
            })?;
        if identity != &expected {
            return Err(mir_error(
                "MIR frame identity does not match the current MIR program",
                None,
            ));
        }
    }
    let captures = state
        .captures
        .iter()
        .cloned()
        .map(|value| {
            let runtime = RuntimeValue::Data(value);
            match runtime_address(&runtime, function.span)? {
                Some(address) => Ok(RuntimeValue::Address(address)),
                None => Ok(runtime),
            }
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let mut frame = Frame::new(function, Vec::new(), captures)?;
    frame.block = state.block;
    frame.ip = state.ip;
    frame.predecessor = state.predecessor;
    frame.values = state
        .values
        .iter()
        .map(|(id, value)| (MirValueId(*id), RuntimeValue::Data(value.clone())))
        .collect();
    frame.place_overrides = state
        .places
        .iter()
        .map(|(id, value)| {
            (
                jet_foundation::MIR::MirPlaceId(*id),
                RuntimeValue::Data(value.clone()),
            )
        })
        .collect();
    frame.locals = state
        .locals
        .iter()
        .map(|(id, value)| {
            (
                jet_foundation::MIR::MirLocalId(*id),
                RuntimeValue::Data(value.clone()),
            )
        })
        .collect();
    frame.scopes = state
        .scopes
        .iter()
        .copied()
        .map(jet_foundation::MIR::MirScopeId)
        .collect();
    Ok(frame)
}
fn frame_state(
    frame: &Frame,
    program: &jet_foundation::MIR::MirProgram,
    identity: Option<MirFrameIdentity>,
) -> Result<MirFrameState, Diagnostic> {
    let function = program_function(program, frame.function)?;
    let captures = frame
        .captures
        .iter()
        .filter_map(|value| runtime_to_state_data(value.clone(), function.span))
        .collect();
    let values = frame
        .values
        .iter()
        .filter_map(|(id, value)| {
            runtime_to_state_data(value.clone(), function.span).map(|value| (id.0, value))
        })
        .collect();
    let places = frame
        .place_overrides
        .iter()
        .filter_map(|(id, value)| {
            runtime_to_state_data(value.clone(), function.span).map(|value| (id.0, value))
        })
        .collect();
    let locals = frame
        .locals
        .iter()
        .filter_map(|(id, value)| {
            runtime_to_state_data(value.clone(), function.span).map(|value| (id.0, value))
        })
        .collect();
    Ok(MirFrameState {
        function: frame.function,
        block: frame.block,
        ip: frame.ip,
        predecessor: frame.predecessor,
        captures,
        values,
        places,
        locals,
        scopes: frame.scopes.iter().map(|scope| scope.0).collect(),
        call_stack: Vec::new(),
        identity,
    })
}
fn write_frame_state(out: &mut String, state: &MirFrameState) {
    if let Some(identity) = &state.identity {
        let _ = identity.write_canonical(out);
    } else {
        let _ = write!(
            out,
            "mir-frame-identity:unavailable;function:{};block:{};",
            state.function.0, state.block.0
        );
    }
    write_values(out, "captures", &state.captures);
    write_map(out, "values", &state.values);
    write_map(out, "places", &state.places);
    write_map(out, "locals", &state.locals);
    let _ = write!(out, "scopes={:?};", state.scopes);
    let _ = write!(out, "calls={};", state.call_stack.len());
}

fn write_map(out: &mut String, name: &str, values: &BTreeMap<u64, MirEvalValue>) {
    let _ = write!(out, "{name}=");
    for (index, (id, value)) in values.iter().enumerate() {
        if index != 0 {
            out.push('|');
        }
        let _ = write!(out, "{id}:{}", mir_debug_value(value));
    }
    out.push(';');
}
fn write_values(out: &mut String, name: &str, values: &[MirEvalValue]) {
    let _ = write!(out, "{name}=");
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.push('|');
        }
        let _ = write!(out, "{index}:{}", mir_debug_value(value));
    }
    out.push(';');
}

fn mir_debug_value(value: &MirEvalValue) -> String {
    if mir_value_contains_moved(value) {
        "<moved>".to_string()
    } else {
        format!("{value:?}")
    }
}

fn mir_show(value: &MirEvalValue) -> String {
    match value {
        MirEvalValue::Moved => "<moved>".to_string(),
        MirEvalValue::Int(value) => value.to_string(),
        MirEvalValue::Float { value, .. } => value.to_string(),
        MirEvalValue::Bool(value) => value.to_string(),
        MirEvalValue::Char(value) => format!("{value:?}"),
        MirEvalValue::String(value) => value.clone(),
        MirEvalValue::BigInt(value) => value.clone(),
        MirEvalValue::Bytes(value) => format!("{value:?}"),
        MirEvalValue::List(values) => format!(
            "[{}]",
            values.iter().map(mir_show).collect::<Vec<_>>().join(", ")
        ),
        MirEvalValue::Map(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{key:?}: {}", mir_show(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        MirEvalValue::Struct { type_name, fields } => {
            let fields = fields
                .iter()
                .filter(|(name, _)| !matches!(name.as_str(), "on_click" | "on_drop"))
                .map(|(name, value)| format!("{name}: {}", mir_show(value)))
                .collect::<Vec<_>>()
                .join(", ");
            if fields.is_empty() {
                type_name.clone()
            } else {
                format!("{type_name} {{ {fields} }}")
            }
        }
        MirEvalValue::Enum {
            type_name,
            variant,
            args,
        } => {
            let payload = args
                .iter()
                .map(|(name, value)| {
                    name.as_deref()
                        .map(|name| format!("{name} = {}", mir_show(value)))
                        .unwrap_or_else(|| mir_show(value))
                })
                .collect::<Vec<_>>()
                .join(", ");
            if payload.is_empty() {
                format!("{type_name}::{variant}")
            } else {
                format!("{type_name}::{variant}({payload})")
            }
        }
        MirEvalValue::Present(value) => format!("Present({})", mir_show(value)),
        MirEvalValue::FailedTold(value) => format!("FailedTold({})", mir_show(value)),
        MirEvalValue::Absent { .. } => "Absent".to_string(),
        MirEvalValue::Unit => "()".to_string(),
        MirEvalValue::Closure(closure) => format!("<closure {:?}>", closure.function),
    }
}
fn mir_source_name(name: &str) -> &str {
    name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(name)
}

fn print_names_match(left: &str, right: &str) -> bool {
    left == right || mir_source_name(left) == mir_source_name(right)
}

fn mir_print_key(key: &MirConstKey) -> String {
    match key {
        MirConstKey::Int(value) => value.to_string(),
        MirConstKey::String(value) => value.clone(),
        MirConstKey::Bool(value) => value.to_string(),
        MirConstKey::Char(value) => format!("{value:?}"),
        MirConstKey::Tuple(fields) => format!(
            "({})",
            fields
                .iter()
                .map(|(_, key)| mir_print_key(key))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        MirConstKey::Struct { type_name, fields } => {
            let fields = fields
                .iter()
                .map(|(name, key)| (mir_source_name(name).to_string(), mir_print_key(key)))
                .collect::<Vec<_>>();
            jet_foundation::StructuralDebug::jet_debug_record(mir_source_name(type_name), fields)
        }
        MirConstKey::Enum {
            type_name: _,
            variant,
        } => mir_source_name(variant).to_string(),
    }
}

fn substitute_print_type(ty: &MirType, substitutions: &BTreeMap<String, MirType>) -> MirType {
    let kind = match ty.kind() {
        MirTypeKind::List(inner) => {
            MirTypeKind::List(Box::new(substitute_print_type(inner, substitutions)))
        }
        MirTypeKind::Map { key, value } => MirTypeKind::Map {
            key: Box::new(substitute_print_type(key, substitutions)),
            value: Box::new(substitute_print_type(value, substitutions)),
        },
        MirTypeKind::Shared(inner) => {
            MirTypeKind::Shared(Box::new(substitute_print_type(inner, substitutions)))
        }
        MirTypeKind::Option(inner) => {
            MirTypeKind::Option(Box::new(substitute_print_type(inner, substitutions)))
        }
        MirTypeKind::Result { ok, err } => MirTypeKind::Result {
            ok: Box::new(substitute_print_type(ok, substitutions)),
            err: Box::new(substitute_print_type(err, substitutions)),
        },
        MirTypeKind::Apply { name, args } => {
            if args.is_empty() {
                if let Some(replacement) = substitutions.get(&name.name) {
                    return replacement.clone();
                }
            }
            MirTypeKind::Apply {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| substitute_print_type(arg, substitutions))
                    .collect(),
            }
        }
        MirTypeKind::Tuple(fields) => MirTypeKind::Tuple(
            fields
                .iter()
                .map(|(name, field)| (name.clone(), substitute_print_type(field, substitutions)))
                .collect(),
        ),
        MirTypeKind::FixedList { elem, len } => MirTypeKind::FixedList {
            elem: Box::new(substitute_print_type(elem, substitutions)),
            len: len.clone(),
        },
        MirTypeKind::InlineRange { base, lo, hi } => MirTypeKind::InlineRange {
            base: Box::new(substitute_print_type(base, substitutions)),
            lo: *lo,
            hi: *hi,
        },
        MirTypeKind::Tagged { marker, inner } => MirTypeKind::Tagged {
            marker: marker.clone(),
            inner: Box::new(substitute_print_type(inner, substitutions)),
        },
        MirTypeKind::Union(members) => MirTypeKind::Union(
            members
                .iter()
                .map(|member| substitute_print_type(member, substitutions))
                .collect(),
        ),
        MirTypeKind::Quantity { base, dimension } => MirTypeKind::Quantity {
            base: Box::new(substitute_print_type(base, substitutions)),
            dimension: dimension.clone(),
        },
        _ => return ty.clone(),
    };
    let mut substituted = MirType::from_kind(kind);
    substituted.identity = ty.identity;
    substituted
}

fn mir_error(message: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::from_row("E0956", &[("what", message)], span)
}

fn mir_error_at(message: &str, span: Span) -> Diagnostic {
    mir_error(message, Some(span))
}
