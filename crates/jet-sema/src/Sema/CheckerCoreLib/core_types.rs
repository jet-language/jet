use crate::AST::{Expr, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::expr_root_ident;
use crate::Syntax;
use super::alloc_ptrs::{io_error_ty, result_ty};

/// D-MUSTUSE1 (c18iwxqx): built-in handle types whose bare statement result must
/// not be silently ignored (E0419). `scope.guard` returns `ScopeGuard` — bind it
/// or cleanup runs at end of the statement, not scope exit. `TransactionGuard` is
/// a phantom return from `on_commit`/`on_rollback` (registration is side-effect);
/// those calls are intentionally ignorable. `Task` stays on L1101.
pub(crate) fn core_must_use_type(name: &str) -> bool {
    matches!(name, "ScopeGuard")
}

pub(crate) fn unit_ty() -> Type {
    Type::Named("Unit".to_string())
}

pub(crate) fn u8_ty() -> Type {
    Type::IntN {
        signed: false,
        bits: 8,
    }
}

pub(crate) fn is_u8_ty(ty: &Type) -> bool {
    matches!(
        ty,
        Type::IntN {
            signed: false,
            bits: 8
        }
    )
}

pub(crate) fn json_ty() -> Type {
    Type::Named(Syntax::TYPE_DATA.to_string())
}

pub(crate) fn json_error_ty() -> Type {
    Type::Named(Syntax::TYPE_JSON_ERROR.to_string())
}

pub(crate) fn encoding_error_ty() -> Type {
    Type::Named("EncodingError".to_string())
}

// D-ENC-DYN1=A+: the dynamic encoding value `Data` (+ aliases `Json`/`Toml`/
// `Yaml`/`Csv`).
pub(crate) fn is_json_type_name(name: &str) -> bool {
    Syntax::is_data_type_name(name)
}

// D-DBDRIVER1: the `DbValue` dynamic tagged SQL value.
pub(crate) fn is_db_value_type_name(name: &str) -> bool {
    Syntax::is_db_value_type_name(name)
}

/// D-SERDE2: the typed-decode error (`{ path, reason }`). Flows as the error arm
/// of `decode<T>` results; the user composes it with `??` and rarely names it.
pub(crate) fn decode_error_ty() -> Type {
    Type::Named("DecodeError".to_string())
}

/// D-VALIDATE1: the accumulated validation error (`{ path, reason }`, same
/// shape as `DecodeError`). `validate { }` blocks / `Type.validate(value)` /
/// `Validate.over(s)` always report failures as `[FieldError]`.
pub(crate) fn field_error_ty() -> Type {
    Type::Named("FieldError".to_string())
}

/// D-SERDE13=B: the value tree's one user-facing name is `DataTree`. The old
/// `Data` spelling is retired (no alias, I8) — point at the new name wherever a
/// user still writes `Data` as a type or a construction receiver.
pub(crate) fn data_renamed_to_datatree(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0351",
        "the value tree is named `DataTree`, not `Data`".to_string(),
        "`DataTree` is the one name a hand codec constructs and returns and every format's `parse` yields — its variants are `.Null`/`.Bool`/`.Int`/`.Float`/`.Text`/`.Array`/`.Object`".to_string(),
        "write `DataTree` instead of `Data`".to_string(),
        Some(span),
    )
}

pub(crate) fn is_json_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_JSON_ERROR || name == "JsonError"
}

pub(crate) fn is_io_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_IO_ERROR || name == "IoError"
}

pub(crate) fn is_utf8_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_UTF8_ERROR || name == "Utf8Error"
}

/// D-TEXTWIDTH1=B: `text.display_width(s, policy: cjk)`'s reject-path error
/// (a `.Reject` control-character policy hit) — mirrors `Utf8Error`'s
/// minimal `{ message }` shape.
pub(crate) fn is_text_error_type_name(name: &str) -> bool {
    name == "TextError"
}

pub(crate) fn core_type_known(name: &str) -> bool {
    matches!(
        name,
        "Unit" | "Void" | "U8" | "Error" | "ProcessResult" | "ProcessSpec" | "ProcessChild" | "Stopwatch" | "Closed"
        // D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum
        // (`.Stream`/`.Inherit`/`.Capture`, D-ENUMDOT2). `ProcessStdin`/
        // `ProcessStdoutStream`/`ProcessStderrStream` are field-access-only
        // handles off a `ProcessChild`; `ProcessLines` is the loop-source-only
        // result of `.lines()` on the latter two (mirrors `FileLines`/`StdinLines`).
        | "ProcessStreamMode" | "ProcessStdin" | "ProcessStdoutStream" | "ProcessStderrStream" | "ProcessLines"
        // D-TEXTWIDTH1=B: `TextWidth` (dot-ctor struct, `core_constructable_fields`)
        // + its two dot-literal enum fields + the `.Reject` policy error.
        | "TextWidth" | "TextWidthAmbiguous" | "TextWidthControls" | "TextError" | "EnvError"
        // D-DET1: deterministic injected capability handles.
        // D-DET-CAPAPI: `Duration` value type for the widened clock surface.
        | "Clock" | "Rng" | "Duration"
        | "GameScene" | "GameAssets" | "GameInputMap" | "GameBudgetsSlot" | "GameBudgets"
        | "GameBackend" | "GameReplay" | "GameImage" | "GameSound" | "GameFrame"
        | "GameInputSnapshot" | "GameSceneType" | "GameReplayType" | "GameBackendType"
        | "GameBudgetsType"
        // D-BIGINT1 / D-DECIMAL1: arbitrary-precision numerics.
        | "BigInt" | "Decimal"
        // D-DBDRIVER1 / D-EFFDBREAD1=A: the `core.db` connection handle and its
        // error. Nameable so a query function can annotate its connection
        // parameter — the shape a `#(Db.Read)` live query (D-LIVEQUERY1) takes.
        | "DbConnection" | "DbError"
        | "FileReader" | "FileWriter" | "FileLines"
        | "StdinHandle" | "StdinLines" | "Stdout" | "Stderr"
        // D-LSDIR1/D-FSOPS1/D-WATCH-SCOPE1: filesystem and watcher values.
        | "DirEntry" | "Stat" | "WalkEntry" | "TempDir" | "TempFile" | "FileLock"
        | "WatchEvent" | "WatchHandle" | "WatchSet"
        // D-DATA-SURFACE1=A / D-DATA-STATUS1=A: data summary/status values.
        | "DataGroup" | "DataStatus" | "DataSummary"
        // D-LOGTRACE1=A: typed structured logging values.
        | "LogField" | "LogSpan"
        // D-ITERTOOLS1=A: expanded collection handles.
        | "BitSet" | "ByteBuffer"
        // E2-M10: networking opaque types.
        | "TcpListener" | "TcpStream" | "IpAddr" | "SocketAddr" | "UdpSocket" | "UdpPacket"
        | "DnsSrv" | "UnixListener" | "UnixStream" | "TlsStream"
        | "NetError" | "NetErrorDetail" | "NetDnsError" | "NetShutdown" | "NetReadyInterest" | "NetReady"
        | "HttpRequest" | "HttpResponse" | "HttpRouter"
        // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): allocator opaque types.
        | "Arena" | "Bump" | "Pool" | "Fixed"
        // D-ARGS1 (ratified 2026-06-22): declarative CLI arg parsing types.
        | "ArgsSpec" | "ParsedArgs"
        // D-ANY-JAI1 (c7jaiany §6): runtime reflection floor handle types.
        | "Value" | "Field"
        // D-TERM1 (ratified 2026-06-22): terminal key-event enum.
        | "Key"
        // D-SERDE2: the format-agnostic value tree + typed-decode error.
        | "DataTree" | "DecodeError"
        // D-VALIDATE1 (ratified 2026-07-12, card #506): the accumulated
        // validation error a `validate { }` block / `Type.validate(value)` /
        // `Validate.over(s)` build up.
        | "FieldError"
        // D-ENCSTREAM-SURFACE1=A: shared encoding values and codec-native
        // opaque stream handles.  Handles are intentionally non-Codable and
        // acquire values only from their format module constructors.
        | "EncodingLimits" | "EncodingError" | "EncodingCause"
        | "EncodingFormat" | "EncodingErrorKind" | "DataEvent"
        | "JSONReader" | "JSONWriter" | "JSONLReader" | "JSONLWriter"
        | "CSVReader" | "CSVWriter" | "XMLReader" | "XMLWriter"
        | "CBORReader" | "CBORWriter"
        // D-SIMD2 / D-LINALG1: built-in SIMD lane + linear-algebra value types.
        | "F32x4" | "F64x2"
        | "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4"
        // D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 2, ratified 2026-06-28/29): the
        // built-in constraint-layout value types.
        | "HVar" | "VVar" | "LengthVar" | "Constraint" | "LayoutHandle"
        // D-REACT1=B: opt-in reactive handle types (used bare as `Signal<T>`/`Derived<T>`).
        | "Signal" | "Derived" | "Computed"
        // D-EVENT1=D: first-party typed Event/Hook family.
        | "Event" | "Hook" | "Subscription" | "EventScope" | "EventPolicy" | "EventTrace"
        // D-HONESTNUM1=A: Measurement<T> value ± uncertainty.
        | "Measurement"
        // D-PENDING1=B: async UI state machine.
        | "Loadable"
        // D-TTLVAL1=A: TTL-wrapped values and rotting secrets.
        | "Expired" | "Expiring" | "Rotting"
        // D-RENDERTGT2=A (c133 M1): UI backend seam types.
        | "Point" | "Size" | "Rect" | "SizeConstraint" | "UiNode" | "InputEvent"
        | "EventResult" | "NullBackend" | "TuiBackend"
        // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend.
        | "GtkBackend"
        // D-A11YGATE1=B (c134 Phase 6): accessible-role opaque type.
        | "UiAriaRole"
        // c-devserver (owner-directed 2026-07-01): the configurable `jet dev`
        // server value returned by `core.web.devserver.for_app(...)`.
        | "DevServer"
        // D-APPROX1=A: approximate sketch data structures.
        | "HyperLogLog" | "TDigest" | "CountMinSketch" | "ReservoirSampler"
        // D-TIMEDEPTH1=A: civil-time types.
        | "Date" | "LocalDate" | "LocalTime" | "DateTime" | "Instant" | "Period" | "Zone"
        | "ZonedDateTime"
        // D-URL1=A: typed URL and MIME values.
        | "Url" | "Mime"
        // D-REGEXENGINE1=A: std-only linear regex values.
        | "Regex" | "RegexFlags" | "Match"
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP types.
        | "HttpClientReq" | "HttpClientResp" | "HttpMux" | "HttpHandler" | "HttpSrvReq" | "HttpSrvResp" | "HttpServerTls" | "HttpServer" | "HttpShutdownReport"
        // D-TYPEDTEXT1=D: typed text — a checked query/markup template built by
        // expected-type elaboration of a string literal (E0149 guards a plain
        // runtime `String` from filling this position).
        | "Sql" | "Html"
        // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — consuming,
        // fallible, `?`-composed cursors over `[U8]`/`String`.
        | "Reader" | "Cursor"
        // D-MIGRATE3=A: decode-time migration transparency. `DecodeResult<T>`
        // (generic, see `is_core_generic` in CheckerCore.rs) and its plain
        // `MigrationStatus` field both need the bare-name gate here too.
        | "DecodeResult" | "MigrationStatus"
        // D-BUILD*: selected-root build-program handles. No runtime values.
        | "BuildContext" | "BuildPlan" | "BuildAction" | "BuildTarget"
        | "BuildToolchain" | "BuildProbe" | "ProgramInfo" | "TypeInfo" | "SourceSpan"
        | "PackageInfo" | "FunctionInfo" | "EffectInfo" | "MethodInfo" | "FieldInfo"
    ) || is_json_type_name(name)
        || is_json_error_type_name(name)
        || is_io_error_type_name(name)
        || is_utf8_error_type_name(name)
}

pub(crate) fn core_struct_field(type_name: &str, field: &str) -> Option<Type> {
    if type_name == "HttpShutdownReport" && matches!(field, "accepted" | "overloaded" | "completed" | "cancelled") {
        return Some(Type::Int);
    }
    if matches!(type_name, "EncodingLimits" | "EncodingCause" | "EncodingError") {
        return core_constructable_fields(type_name)?.into_iter().find(|(name, _)| name == field).map(|(_, ty)| ty);
    }
    if type_name == Syntax::TYPE_BUILD_CONTEXT && field == "program" {
        return Some(Type::Named(Syntax::TYPE_PROGRAM_INFO.to_string()));
    }
    if type_name == Syntax::TYPE_TYPE_INFO {
        return match field {
            "name" | "module" | "identity" | "kind" => Some(Type::String),
            "fields" => Some(Type::List(Box::new(Type::Named("FieldInfo".to_string())))),
            "methods" => Some(Type::List(Box::new(Type::Named("MethodInfo".to_string())))),
            "markers" | "implements" => Some(Type::List(Box::new(Type::String))),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == "FunctionInfo" {
        return match field {
            "name" | "module" | "identity" => Some(Type::String),
            "params" => Some(Type::List(Box::new(Type::String))),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            "effects" => Some(Type::Named("EffectInfo".to_string())),
            "reaches_panic" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "PackageInfo" {
        return match field {
            "name" | "identity" => Some(Type::String),
            "types" => Some(Type::List(Box::new(Type::Named(Syntax::TYPE_TYPE_INFO.to_string())))),
            "functions" => Some(Type::List(Box::new(Type::Named("FunctionInfo".to_string())))),
            _ => None,
        };
    }
    if type_name == "EffectInfo" && field == "values" {
        return Some(Type::List(Box::new(Type::String)));
    }
    if type_name == "MethodInfo" {
        return match field {
            "name" | "module" | "identity" | "return_type" | "signature" => Some(Type::String),
            "params" | "markers" => Some(Type::List(Box::new(Type::String))),
            "is_pub" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "FieldInfo" {
        return match field {
            "name" | "ty" => Some(Type::String),
            "markers" => Some(Type::List(Box::new(Type::String))),
            "is_pub" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_SOURCE_SPAN {
        return match field {
            "start" | "end" => Some(Type::Int),
            _ => None,
        };
    }
    if is_json_error_type_name(type_name) {
        return match field {
            "line" => Some(Type::Int),
            "message" => Some(Type::String),
            _ => None,
        };
    }
    if is_utf8_error_type_name(type_name) {
        return match field {
            "message" => Some(Type::String),
            _ => None,
        };
    }
    if is_text_error_type_name(type_name) {
        return match field {
            "message" => Some(Type::String),
            _ => None,
        };
    }
    // D-SERDE2: DecodeError exposes the field path and a plain reason.
    if type_name == "DecodeError" {
        return match field {
            "path" | "reason" => Some(Type::String),
            _ => None,
        };
    }
    // D-VALIDATE1: FieldError mirrors DecodeError's shape exactly.
    if type_name == "FieldError" {
        return match field {
            "path" | "reason" => Some(Type::String),
            _ => None,
        };
    }
    // D-MIGRATE3=A: `MigrationStatus` — `.migrated` false + `.from`/`.steps`
    // empty for fresh data and for non-`#PublishedSchema` types.
    if type_name == "MigrationStatus" {
        return match field {
            "migrated" => Some(Type::Bool),
            "from" => Some(Type::String),
            "steps" => Some(Type::List(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "DataGroup" {
        return match field {
            "key" => Some(Type::String),
            "count" => Some(Type::Int),
            "sum" | "mean" => Some(Type::Float),
            _ => None,
        };
    }
    if type_name == "DataStatus" {
        return match field {
            "step" | "path" | "replacement" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "DataSummary" {
        return match field {
            "count" => Some(Type::Int),
            "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev" => {
                Some(Type::Float)
            }
            _ => None,
        };
    }
    match (type_name, field) {
        // D-LSDIR1=A: DirEntry has name (bare filename), path (full path), is_dir.
        ("DirEntry", "name" | "path") => Some(Type::String),
        ("DirEntry", "is_dir") => Some(Type::Bool),
        // D-FSOPS1=A: typed filesystem metadata and recursive walk entries.
        ("Stat", "size" | "modified_ms" | "created_ms") => Some(Type::Int),
        ("Stat", "readonly" | "is_file" | "is_dir" | "is_symlink") => Some(Type::Bool),
        ("Stat", "kind") => Some(Type::String),
        ("WalkEntry", "path" | "relative") => Some(Type::String),
        ("WalkEntry", "is_dir") => Some(Type::Bool),
        ("WalkEntry", "depth") => Some(Type::Int),
        ("TempDir" | "TempFile" | "FileLock", "path") => Some(Type::String),
        ("WatchEvent", "domain" | "kind" | "path" | "detail") => Some(Type::String),
        ("WatchEvent", "pid" | "port") => Some(Type::Int),
        // D-RENDERTGT2=A (c133 M1): UI geometry fields.
        ("Point", "x" | "y") => Some(Type::Float),
        ("Size", "width" | "height") => Some(Type::Float),
        ("Rect", "x" | "y" | "width" | "height") => Some(Type::Float),
        ("SizeConstraint", "min_width" | "min_height" | "max_width" | "max_height") => {
            Some(Type::Float)
        }
        ("UiNode", "label") => Some(Type::String),
        ("UiNode", "width" | "height") => Some(Type::Float),
        ("ProcessResult", "code") => Some(Type::Int),
        ("ProcessResult", "success" | "timed_out") => Some(Type::Bool),
        ("ProcessResult", "signal") => Some(Type::Option(Box::new(Type::Int))),
        ("ProcessResult", "output" | "errors") => Some(Type::String),
        // D-PROCESS1=A: `child.stdin`/`.stdout`/`.stderr` are handle fields, not
        // plain values — a writer and two streaming readers (E2502-restricted,
        // see `core_type_known`).
        ("ProcessChild", "stdin") => Some(Type::Named("ProcessStdin".to_string())),
        ("ProcessChild", "stdout") => Some(Type::Named("ProcessStdoutStream".to_string())),
        ("ProcessChild", "stderr") => Some(Type::Named("ProcessStderrStream".to_string())),
        // E2-M10: HTTP request fields exposed to handlers.
        ("HttpRequest", "method" | "path" | "body") => Some(Type::String),
        ("HttpRequest", "headers") => Some(Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(Type::String),
        }),
        // E2-M10: HTTP response fields.
        ("HttpResponse", "status" | "body") => Some(Type::String),
        ("HttpResponse", "headers") => Some(Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(Type::String),
        }),
        // D-GAME-*: scene-owned headless game substrate fields.
        ("GameScene", "assets") => Some(Type::Named("GameAssets".to_string())),
        ("GameScene", "input") => Some(Type::Named("GameInputMap".to_string())),
        ("GameScene", "budgets") => Some(Type::Named("GameBudgetsSlot".to_string())),
        ("GameFrame", "index") => Some(Type::Int),
        ("GameFrame", "input") => Some(Type::Named("GameInputSnapshot".to_string())),
        _ => None,
    }
}

impl<'a> Checker<'a> {
    pub(super) fn check_game_run_scene_edit(&mut self, expr: &Expr) {
        let Some(root) = expr_root_ident(expr) else {
            self.diags.push(Diagnostic::error(
                "E0202",
                "`game.run` needs a mutable scene binding".to_string(),
                "running a scene advances its frame hooks and deterministic replay state"
                    .to_string(),
                "store the scene in `scene := game.Scene.new(...)`, then call `game.run(scene)`"
                    .to_string(),
                Some(expr.span()),
            ));
            return;
        };
        if let Some(info) = self.lookup(root) {
            if !info.mutable {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!("`game.run` needs edit access to `{root}`"),
                    "running a scene advances its frame hooks and deterministic replay state"
                        .to_string(),
                    format!("declare `{root} := game.Scene.new(...)` before running it"),
                    Some(expr.span()),
                ));
            }
        }
    }
}

pub(super) fn game_run_label_error(
    diags: &mut Vec<Diagnostic>,
    label: &str,
    arg: &crate::AST::CallArg,
    index: usize,
    span: Span,
) {
    let (expected, fix) = if index == 1 {
        ("replay or backend", "write `replay:` here, `backend:` here for a two-argument backend call, or drop the label")
    } else {
        ("backend", "write `backend:` here, or drop the label")
    };
    let label_span = arg.label.as_ref().map(|(_, s)| *s).unwrap_or(span);
    diags.push(Diagnostic::error(
        "E0125",
        format!("`game.run` has no `{label}:` option at argument {}", index + 1),
        format!("this position accepts {expected}; labels document the positional shape and never reorder arguments"),
        fix.to_string(),
        Some(label_span),
    ));
}

/// D-MIGRATE3=A: field access on the reserved generic `DecodeResult<T>` —
/// `.value: T` and `.migration: MigrationStatus`. Mirrors [`core_struct_field`]
/// for the one reserved core type that carries a generic type argument
/// (`Type::Apply`, not `Type::Named`); see the `Type::Apply` arm in
/// `CheckerInfer/expr.rs`'s member-access resolver.
pub(crate) fn core_generic_struct_field(
    type_name: &str,
    field: &str,
    args: &[Type],
) -> Option<Type> {
    if type_name == "DecodeResult" {
        return match field {
            "value" => args.first().cloned(),
            "migration" => Some(Type::Named("MigrationStatus".to_string())),
            _ => None,
        };
    }
    None
}

pub fn core_json_pattern_types(variant: &str) -> Option<Vec<Type>> {
    let json = json_ty();
    match variant {
        "Null" => Some(Vec::new()),
        "Bool" => Some(vec![Type::Bool]),
        "Int" => Some(vec![Type::Int]),
        "Float" => Some(vec![Type::Float]),
        "Text" => Some(vec![Type::String]),
        "Array" => Some(vec![Type::List(Box::new(json.clone()))]),
        "Object" => Some(vec![Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(json),
        }]),
        _ => None,
    }
}

/// D-TERM1 (ratified 2026-06-22): pattern types for `Key` enum variants.
/// Used by the pattern checker to validate `if k == Key.Char(c)` etc.
pub(crate) fn core_key_pattern_types(variant: &str) -> Option<Vec<Type>> {
    match variant {
        // Unit variants — no payload.
        "Enter" | "Escape" | "Backspace" | "Tab" | "Delete" | "Up" | "Down" | "Left" | "Right"
        | "Unknown" => Some(Vec::new()),
        // `Key.Char(c)` — one Char payload.
        "Char" => Some(vec![Type::Char]),
        // `Key.Ctrl(c)` — one Char payload (the control character).
        "Ctrl" => Some(vec![Type::Char]),
        // `Key.F(n)` — one Int payload (function key number 1–12).
        "F" => Some(vec![Type::Int]),
        _ => None,
    }
}

/// D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum (`.Stream`,
/// `.Inherit`, `.Capture` — exactly the three ratified stream modes), not in
/// the user registry. Mirrors `core_key_variants`.
pub(crate) fn core_process_stream_mode_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    let mut m = std::collections::HashMap::new();
    for name in &["Stream", "Inherit", "Capture"] {
        m.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    m
}

pub(crate) fn core_env_error_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    ["InvalidName", "InvalidValue", "NonUnicode"]
        .into_iter()
        .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
        .collect()
}

pub(crate) fn core_net_control_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let names: &[&str] = match enum_name {
        "NetShutdown" => &["Read", "Write", "Both"],
        "NetReadyInterest" => &["Read", "Write", "ReadWrite"],
        _ => return None,
    };
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    for name in names {
        variants.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    Some(variants)
}

pub(crate) fn core_net_error_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    if enum_name == "NetDnsError" {
        for name in ["NotFound", "Failure"] {
            variants.insert(name.to_string(), (zero, VariantPayload::Single(Type::String, zero)));
        }
        return Some(variants);
    }
    if enum_name != "NetError" { return None; }
    for name in [
        "InvalidInput", "PermissionDenied", "AddressInUse", "AddressUnavailable",
        "ConnectionRefused", "ConnectionReset", "NotConnected", "Closed", "Timeout",
        "Cancelled", "Unsupported", "Tls", "Protocol", "Other",
    ] {
        variants.insert(name.to_string(), (
            zero,
            VariantPayload::Single(Type::Named("NetErrorDetail".to_string()), zero),
        ));
    }
    variants.insert("Dns".to_string(), (
        zero,
        VariantPayload::Single(Type::Named("NetDnsError".to_string()), zero),
    ));
    Some(variants)
}

/// D-TEXTWIDTH1=B: the two `TextWidth` field enums (`.Narrow`/`.Wide`,
/// `.Zero`/`.Reject`) — synthesised the same way as `ProcessStreamMode`.
pub(crate) fn core_text_width_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    let names: &[&str] = match enum_name {
        "TextWidthAmbiguous" => &["Narrow", "Wide"],
        "TextWidthControls" => &["Zero", "Reject"],
        _ => return None,
    };
    let mut m = std::collections::HashMap::new();
    for name in names {
        m.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    Some(m)
}

/// D-TERM1 (ratified 2026-06-22): synthesised variant table for the `Key` enum.
/// Used by `resolve_enum_variants_cloned` so `Key.Char(c)` / `Key.Enter` literals
/// pass type-checking without `Key` being in the user type registry.
pub(crate) fn core_key_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let mut m = std::collections::HashMap::new();
    // Unit variants.
    for name in &[
        "Enter",
        "Escape",
        "Backspace",
        "Tab",
        "Delete",
        "Up",
        "Down",
        "Left",
        "Right",
        "Unknown",
    ] {
        m.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    // Single-payload variants.
    m.insert(
        "Char".to_string(),
        (zero, VariantPayload::Single(Type::Char, zero)),
    );
    m.insert(
        "Ctrl".to_string(),
        (zero, VariantPayload::Single(Type::Char, zero)),
    );
    m.insert(
        "F".to_string(),
        (zero, VariantPayload::Single(Type::Int, zero)),
    );
    m
}

/// E2-M7: type-check a method call on a FileReader or FileWriter handle (D-IO2).
/// Returns `Some(return_type)` when the method is valid, or emits E2501 and
/// returns `None` for an invalid method / wrong-direction call.
pub fn file_handle_method_return(
    handle_ty: &str,
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let io = io_error_ty();
    let unit = unit_ty();
    match handle_ty {
        "FileReader" => match method {
            // `.lines()` — returns the handle as a streaming source for `loop … in`.
            // We encode the return as `Named("FileLines")` so the loop body knows
            // the element type is `String`.
            "lines" if n_args == 0 => Some(Some(Type::Named("FileLines".to_string()))),
            // `.read_line()` — returns one line or `None` at EOF.
            "read_line" if n_args == 0 => {
                Some(Some(result_ty(Type::Option(Box::new(Type::String)), io)))
            }
            // Wrong direction: writing to a reader.
            "write_line" | "flush" => {
                diags.push(Diagnostic::error(
                    "E2501",
                    format!("`{}` is not available on a read-only file handle", method),
                    "`files.open` returns a read-only handle; it can only read lines or bytes"
                        .to_string(),
                    "use `files.create` or `files.append` to get a writable handle".to_string(),
                    Some(span),
                ));
                Some(None)
            }
            _ => None,
        },
        "FileWriter" => match method {
            // `.write_line(text)` — writes a line followed by a newline.
            "write_line" if n_args == 1 => Some(Some(result_ty(unit.clone(), io.clone()))),
            // `.flush()` — ensure buffered bytes reach disk.
            "flush" if n_args == 0 => Some(Some(result_ty(unit, io))),
            // Wrong direction: reading from a writer.
            "lines" | "read_line" => {
                diags.push(Diagnostic::error(
                    "E2501",
                    format!("`{}` is not available on a write-only file handle", method),
                    "`files.create` returns a write-only handle; it can only write lines"
                        .to_string(),
                    "use `files.open` to get a readable handle".to_string(),
                    Some(span),
                ));
                Some(None)
            }
            _ => None,
        },
        // D-STDIN1=A: StdinHandle methods.
        "StdinHandle" => match method {
            "lines" if n_args == 0 => Some(Some(Type::Named("StdinLines".to_string()))),
            "read_line" if n_args == 0 => {
                Some(Some(result_ty(Type::Option(Box::new(Type::String)), io)))
            }
            _ => None,
        },
        // D-COREIO1=A: stdout/stderr stream methods.
        "Stdout" | "Stderr" => match method {
            "write" | "write_line" if n_args == 1 => {
                Some(Some(result_ty(unit.clone(), io.clone())))
            }
            "write_bytes" if n_args == 1 => Some(Some(result_ty(unit.clone(), io.clone()))),
            "flush" if n_args == 0 => Some(Some(result_ty(unit.clone(), io))),
            "is_tty" if n_args == 0 => Some(Some(Type::Bool)),
            _ => None,
        },
        _ => None,
    }
}

/// D-ENCSTREAM-SURFACE1=A: mutable opaque codec-handle methods.
pub fn encoding_handle_method_return(
    handle_ty: &str,
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    let error = encoding_error_ty();
    let unit = unit_ty();
    match (handle_ty, method, n_args) {
        ("JSONReader", "next", 0) => Some(Some(result_ty(
            Type::Option(Box::new(Type::Named("DataEvent".to_string()))),
            error,
        ))),
        ("JSONWriter", "write", 1) | ("JSONWriter", "flush" | "finish", 0) => {
            Some(Some(result_ty(unit, error)))
        }
        _ => None,
    }
}

/// E2-M10: field definitions for compiler-known constructable struct types.
/// Returns `Some(fields)` when the named type is a prelude struct users can construct.
pub(crate) fn core_constructable_fields(type_name: &str) -> Option<Vec<(String, Type)>> {
    let str_ty = Type::String;
    let map_ty = Type::Map {
        key: Box::new(Type::String),
        key_span: None,
        value: Box::new(Type::String),
    };
    match type_name {
        "HttpResponse" => Some(vec![
            ("status".to_string(), str_ty.clone()),
            ("body".to_string(), str_ty),
            ("headers".to_string(), map_ty),
        ]),
        "HttpRequest" => Some(vec![
            ("method".to_string(), str_ty.clone()),
            ("path".to_string(), str_ty.clone()),
            ("body".to_string(), str_ty),
            ("headers".to_string(), map_ty),
        ]),
        // D-TEXTWIDTH1=B: `TextWidth.{ ambiguous: .Wide, controls: .Reject }`
        // — the two dot-literal enum fields resolve via `resolve_enum_variants_cloned`
        // (below), the same "core enum, not in the user registry" mechanism as
        // `ProcessStreamMode`.
        "TextWidth" => Some(vec![
            ("ambiguous".to_string(), Type::Named("TextWidthAmbiguous".to_string())),
            ("controls".to_string(), Type::Named("TextWidthControls".to_string())),
        ]),
        // D-SERDE2 / D-SERDE14=A: a hand `decode` builds its own rejection with
        // `DecodeError.{ path: …, reason: … }` and returns it via `err(…)`. Both
        // fields are `String`; `path` is the wire location (e.g. `""` for a
        // whole-value reject, `"email"` for a field). Registering it here is what
        // makes the dot-ctor legal (it was E0119 before this decision).
        "DecodeError" => Some(vec![
            ("path".to_string(), str_ty.clone()),
            ("reason".to_string(), str_ty.clone()),
        ]),
        // D-VALIDATE1: `FieldError.{ path: …, reason: … }` — registering it
        // here is what makes the dot-ctor legal, same as `DecodeError` above.
        "FieldError" => Some(vec![
            ("path".to_string(), str_ty.clone()),
            ("reason".to_string(), str_ty),
        ]),
        "EncodingLimits" => Some(vec![
            ("buffer_bytes".to_string(), Type::Int),
            ("max_depth".to_string(), Type::Int),
            ("max_item_bytes".to_string(), Type::Int),
            ("max_total_bytes".to_string(), Type::Option(Box::new(Type::Int))),
            ("max_expansion_depth".to_string(), Type::Int),
            ("max_expansion_bytes".to_string(), Type::Int),
        ]),
        "EncodingCause" => Some(vec![
            ("kind".to_string(), Type::String),
            ("os_code".to_string(), Type::Option(Box::new(Type::Int))),
            ("message".to_string(), Type::String),
        ]),
        "EncodingError" => Some(vec![
            ("format".to_string(), Type::Named("EncodingFormat".to_string())),
            ("kind".to_string(), Type::Named("EncodingErrorKind".to_string())),
            ("byte_offset".to_string(), Type::Int),
            ("line".to_string(), Type::Option(Box::new(Type::Int))),
            ("column".to_string(), Type::Option(Box::new(Type::Int))),
            ("path".to_string(), Type::String),
            ("reason".to_string(), Type::String),
            ("cause".to_string(), Type::Option(Box::new(Type::Named("EncodingCause".to_string())))),
        ]),
        _ => None,
    }
}

/// D-ENCSTREAM-SURFACE1=A: closed shared stream enums.
pub(crate) fn core_encoding_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    let units: &[&str] = match enum_name {
        "EncodingFormat" => &["JSON", "JSONL", "CSV", "XML", "CBOR"],
        "EncodingErrorKind" => &["Syntax", "Truncated", "Unsupported", "Limit", "IO", "State"],
        "DataEvent" => &["Null", "ArrayStart", "ArrayEnd", "ObjectStart", "ObjectEnd"],
        _ => return None,
    };
    for name in units {
        variants.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    if enum_name == "DataEvent" {
        for (name, ty) in [
            ("Bool", Type::Bool), ("Int", Type::Int), ("Float", Type::Float),
            ("Text", Type::String), ("Bytes", Type::List(Box::new(u8_ty()))),
            ("Key", Type::String),
        ] {
            variants.insert(name.to_string(), (zero, VariantPayload::Single(ty, zero)));
        }
    }
    Some(variants)
}
