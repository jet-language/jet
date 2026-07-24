use crate::AST::{Expr, Type, VariantField, VariantPayload};
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

/// D-EMAIL-SMTP-SURFACE1=A: exact ungated Message envelope access/replacement.
pub fn email_method_return(ty: &Type, method: &str, argc: usize) -> Option<Option<Type>> {
    match (ty, method, argc) {
        (Type::Named(name), "envelope", 0) if name == "Message" => {
            Some(Some(Type::Named("Envelope".to_string())))
        }
        (Type::Named(name), "with_envelope", 1) if name == "Message" => Some(Some(result_ty(
            Type::Named("Message".to_string()), Type::Named("EmailError".to_string()),
        ))),
        (Type::Named(name), "send", 1) if name == "Mailer" => Some(Some(result_ty(
            Type::Named("SendReport".to_string()), Type::Named("EmailError".to_string()),
        ))),
        _ => None,
    }
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
        | "Claims" | "AuthError"
        // D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum
        // (`.Stream`/`.Inherit`/`.Capture`, D-ENUMDOT2). `ProcessStdin`/
        // `ProcessStdoutStream`/`ProcessStderrStream` are field-access-only
        // handles off a `ProcessChild`; `ProcessLines` is the loop-source-only
        // result of `.lines()` on the latter two (mirrors `FileLines`/`StdinLines`).
        | "ProcessStreamMode" | "ProcessStdin" | "ProcessStdoutStream" | "ProcessStderrStream" | "ProcessLines"
        | "IOContext" | "IOOperation"
        // D-TEXTWIDTH1=B: `TextWidth` (dot-ctor struct, `core_constructable_fields`)
        // + its two dot-literal enum fields + the `.Reject` policy error.
        | "TextWidth" | "TextWidthAmbiguous" | "TextWidthControls" | "TextError" | "EnvError"
        // D-DET1: deterministic injected capability handles.
        // D-DET-CAPAPI: `Duration` value type for the widened clock surface.
        | "Clock" | "Rng" | "Duration" | "DurationUnit" | "RangeError"
        | "GameScene" | "GameAssets" | "GameInputMap"
        | "GameBackend" | "GameReplay" | "GameImage" | "GameSound" | "GameFrame"
        | "GameInputSnapshot" | "GameSceneType" | "GameReplayType" | "GameBackendType"
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
        | "DataGroup" | "DataColumn" | "DataStatus" | "DataSummary"
        // D-LOGTRACE1=A: typed structured logging values.
        | "LogField" | "LogSpan"
        // D-ITERTOOLS1=A: expanded collection handles.
        | "BitSet" | "ByteBuffer"
        // E2-M10: networking opaque types.
        | "TcpListener" | "TcpStream" | "IpAddr" | "SocketAddr" | "UdpSocket" | "UdpPacket"
        | "DnsSrv" | "UnixListener" | "UnixStream" | "TlsStream" | "TlsClientConfig" | "TlsClientConfigType"
        | "TlsRootCertificates" | "TlsRootCertificatesType" | "TlsClientIdentity" | "TlsClientIdentityType"
        | "TlsClientTrust" | "TlsVersion" | "TlsPeerIdentity" | "TlsCertificate"
        | "NetError" | "NetErrorDetail" | "NetDnsError" | "NetShutdown" | "NetReadyInterest" | "NetReady"
        | "HttpRequest" | "HttpResponse" | "HttpRouter" | "HttpClient" | "HttpClientType"
        // D-CRYPTO-API1=A: purpose-bound crypto values. Secret-bearing values
        // are opaque and receive no structural/collection capabilities.
        | "Secret" | "SigningKey" | "VerifyKey" | "X25519SecretKey" | "X25519PublicKey"
        | "SharedSecret" | "Signature" | "Sealed" | "WrappedKey" | "PasswordHash"
        | "Digest256" | "Digest512" | "CryptoError"
        | "KeyRef" | "MutationPlan" | "VaultWrite" | "Rotation" | "WrappedImportPlan"
        | "KeyStatus" | "VaultError" | "WrappedVaultKey" | "KeyUnlock" | "KeyWrapError"
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
        | "CBOROptions" | "CBORError" | "CBORErrorKind"
        | "XMLLimits" | "XMLParseOptions" | "XMLRenderOptions" | "XMLEncoding"
        | "XMLLexicalPolicy" | "XMLCanonical" | "XMLCanonicalMode" | "XMLError" | "XMLReason" | "XMLEntityPolicy"
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
        | "Signal" | "Derived" | "Computed" | "Effect"
        // D-EVENT1=D: first-party typed Event/Hook family.
        | "Event" | "Hook" | "DecisionHook" | "HookPolicy" | "HookDecision" | "HookOutcome"
        | "Subscription" | "EventScope" | "EventPolicy" | "EventTrace"
        | "AsyncEvent" | "AsyncPolicy" | "Overflow" | "FailurePolicy" | "DispatchReport" | "DispatchFailure" | "DispatchState" | "EventConfigError"
        // D-HONESTNUM1=A: Measurement<T> value ± uncertainty.
        | "Measurement"
        // D-PENDING1=B: async UI state machine.
        | "Loadable"
        // D-CORE-SECRETS1=A / D-TTLVAL1=A: generic TTL plus one secret wrapper.
        | "Expired" | "ExpiringValue" | "ExpiringSecret"
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
        // D-EMAIL1=A / D-EMAIL-SMTP-SURFACE1=A: exact ungated email values.
        | "Address" | "Message" | "Attachment" | "Envelope" | "EmailError"
        | "SmtpSecurity" | "RecipientPolicy" | "RecipientReport" | "SendReport"
        | "Limits" | "SmtpAuth" | "TlsTrust" | "DkimConfig" | "SmtpConfig" | "Mailer"
        // D-REGEXENGINE1=A: std-only linear regex values.
        | "Regex" | "RegexFlags" | "Match"
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP types.
        | "HttpMethod" | "HttpStatus" | "HttpVersion" | "HttpHeaderName" | "HttpHeaderValue"
        | "HttpHeaders" | "HttpBody" | "HttpBodyChunks" | "HttpError" | "HttpOperation" | "HttpProxy" | "HttpRedirectPolicy" | "HttpRetryPolicy" | "HttpCookieJar" | "HttpMux" | "HttpHandler" | "HttpServerTls" | "HttpServer" | "HttpShutdownReport" | "HttpCorsPolicy" | "HttpCompressEncoding"
        | "WsConn" | "WsError" | "WsMessage"
        // D-TYPEDTEXT1=D: typed text — a checked query/markup template built by
        // expected-type elaboration of a string literal (E0149 guards a plain
        // runtime `String` from filling this position).
        | "Sql" | "Html" | "Sh"
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
    if type_name == "TlsPeerIdentity" {
        return match field {
            "verified_server_name" => Some(Type::String),
            "leaf" => Some(Type::Named("TlsCertificate".to_string())),
            "certificate_chain" => Some(Type::List(Box::new(Type::Named("TlsCertificate".to_string())))),
            _ => None,
        };
    }
    if type_name == "TlsCertificate" {
        return match field {
            "der" | "sha256" | "spki_sha256" => Some(Type::List(Box::new(Type::IntN { signed: false, bits: 8 }))),
            "dns_names" => Some(Type::List(Box::new(Type::String))),
            "valid_from_unix_ms" | "valid_until_unix_ms" => Some(Type::Int),
            "subject" | "issuer" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "Claims" {
        return match field {
            "subject" | "issuer" => Some(Type::Option(Box::new(Type::String))),
            "audience" => Some(Type::String),
            "expires_at" => Some(Type::Int),
            "issued_at" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_IO_CONTEXT {
        return match field {
            "operation" => Some(Type::Named(Syntax::TYPE_IO_OPERATION.to_string())),
            "resource" | "cause" => Some(Type::Option(Box::new(Type::String))),
            "os_code" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if type_name == "HttpShutdownReport" && matches!(field, "accepted" | "overloaded" | "completed" | "cancelled") {
        return Some(Type::Int);
    }
    if matches!(type_name, "EncodingLimits" | "EncodingCause" | "EncodingError" | "CBOROptions" | "CBORError" | "XMLLimits" | "XMLParseOptions" | "XMLError" | "AsyncPolicy" | "RecipientReport" | "SendReport" | "Limits" | "DkimConfig" | "SmtpConfig") {
        return core_constructable_fields(type_name)?.into_iter().find(|(name, _)| name == field).map(|(_, ty)| ty);
    }
    if type_name == "Envelope" {
        return match field {
            "from" => Some(Type::Named("Address".to_string())),
            "recipients" => Some(Type::List(Box::new(Type::Named("Address".to_string())))),
            _ => None,
        };
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
    if type_name == "DataColumn" {
        return match field {
            "name" | "type_name" => Some(Type::String),
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
        // D-HTTP-CORE2=A: one byte-native message model.
        ("HttpRequest", "method" | "path") => Some(Type::String),
        ("HttpRequest", "body") => Some(Type::Named("HttpBody".to_string())),
        ("HttpRequest", "headers") => Some(Type::Named("HttpHeaders".to_string())),
        ("HttpResponse", "status") => Some(Type::Int),
        ("HttpResponse", "body") => Some(Type::Named("HttpBody".to_string())),
        ("HttpResponse", "headers") => Some(Type::Named("HttpHeaders".to_string())),
        // D-GAME-*: scene-owned headless game substrate fields.
        ("GameScene", "assets") => Some(Type::Named("GameAssets".to_string())),
        ("GameScene", "input") => Some(Type::Named("GameInputMap".to_string())),
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
    if type_name == "DataJoin" && args.len() == 2 {
        return match field {
            "left" => Some(args[0].clone()),
            "right" => Some(args[1].clone()),
            _ => None,
        };
    }
    if type_name == "Rotation" && args.len() == 1 {
        return match field {
            "previous" | "current" => Some(Type::Apply {
                name: "KeyRef".to_string(),
                args: vec![args[0].clone()],
            }),
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

/// D-HTTP-CORE2=A / D-HTTP-UNSUPPORTED1=A: the one closed HTTP error tree.
pub(crate) fn core_http_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::{VariantField, VariantPayload};
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    if enum_name == "HttpOperation" {
        for name in ["ClientConnect", "ServerBind", "ServeListener"] {
            variants.insert(name.to_string(), (zero, VariantPayload::Unit));
        }
        return Some(variants);
    }
    if enum_name == "HttpProxy" {
        variants.insert("FromEnvironment".to_string(), (zero, VariantPayload::Unit));
        variants.insert("None".to_string(), (zero, VariantPayload::Unit));
        variants.insert(
            "Url".to_string(),
            (zero, VariantPayload::Single(Type::String, zero)),
        );
        return Some(variants);
    }
    if enum_name == "HttpRedirectPolicy" {
        // D-HTTP-CLIENT2=A: `.Follow(max:, same_origin_credentials:)`.
        variants.insert(
            "Follow".to_string(),
            (
                zero,
                VariantPayload::Named(vec![
                    VariantField {
                        name: "max".to_string(),
                        name_span: zero,
                        ty: Type::Int,
                        ty_span: zero,
                    },
                    VariantField {
                        name: "same_origin_credentials".to_string(),
                        name_span: zero,
                        ty: Type::Bool,
                        ty_span: zero,
                    },
                ]),
            ),
        );
        return Some(variants);
    }
    if enum_name == "HttpRetryPolicy" {
        // D-HTTP-CLIENT2=A: `.None` / `.Safe` / `.Idempotent`.
        for name in ["None", "Safe", "Idempotent"] {
            variants.insert(name.to_string(), (zero, VariantPayload::Unit));
        }
        return Some(variants);
    }
    if enum_name == "HttpCookieJar" {
        variants.insert("Memory".to_string(), (zero, VariantPayload::Unit));
        return Some(variants);
    }
    if enum_name == "HttpCompressEncoding" {
        variants.insert("Gzip".to_string(), (zero, VariantPayload::Unit));
        return Some(variants);
    }
    if enum_name == "WsError" {
        for name in [
            "InvalidUrl",
            "InvalidHandshake",
            "Protocol",
            "Timeout",
            "Closed",
            "Cancelled",
            "UnsupportedTarget",
        ] {
            variants.insert(name.to_string(), (zero, VariantPayload::Unit));
        }
        variants.insert(
            "MessageTooLarge".to_string(),
            (
                zero,
                VariantPayload::Named(vec![VariantField {
                    name: "limit".to_string(),
                    name_span: zero,
                    ty: Type::Int,
                    ty_span: zero,
                }]),
            ),
        );
        variants.insert(
            "Io".to_string(),
            (
                zero,
                VariantPayload::Named(vec![VariantField {
                    name: "operation".to_string(),
                    name_span: zero,
                    ty: Type::String,
                    ty_span: zero,
                }]),
            ),
        );
        return Some(variants);
    }
    if enum_name != "HttpError" {
        return None;
    }
    for name in [
        "InvalidMethod", "InvalidUrl", "InvalidHeader", "InvalidStatus", "BodyConsumed",
        "InvalidFraming", "UnsupportedEncoding", "Cancelled",
    ] {
        variants.insert(name.to_string(), (zero, VariantPayload::Unit));
    }
    for (name, field, ty) in [
        ("BodyTooLarge", "limit", Type::Int),
        ("Resolve", "host", Type::String),
        ("Connect", "address", Type::String),
        ("Tls", "stage", Type::String),
        ("Timeout", "phase", Type::String),
        ("Proxy", "stage", Type::String),
        ("Redirect", "reason", Type::String),
        ("Protocol", "version", Type::String),
        ("Io", "operation", Type::String),
        ("ResourceUnavailable", "resource", Type::String),
        ("Internal", "incident_id", Type::String),
        ("UnsupportedTarget", "operation", Type::Named("HttpOperation".to_string())),
    ] {
        variants.insert(name.to_string(), (zero, VariantPayload::Named(vec![VariantField {
            name: field.to_string(),
            name_span: zero,
            ty,
            ty_span: zero,
        }])));
    }
    Some(variants)
}

pub(crate) fn core_io_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    if enum_name == Syntax::TYPE_IO_OPERATION {
        for name in Syntax::IO_OPERATION_VARIANTS {
            variants.insert((*name).to_string(), (zero, VariantPayload::Unit));
        }
        return Some(variants);
    }
    if !is_io_error_type_name(enum_name) { return None; }
    for name in Syntax::IO_ERROR_VARIANTS {
        variants.insert((*name).to_string(), (
            zero,
            VariantPayload::Single(Type::Named(Syntax::TYPE_IO_CONTEXT.to_string()), zero),
        ));
    }
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

/// D-SHAPE-DURATIONCONVERT1=A: the closed unit list accepted by
/// `duration.in(unit)`.
pub(crate) fn core_duration_unit_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    if enum_name != Syntax::DURATION_UNIT_TYPE {
        return None;
    }
    let zero = Span::new(0, 0);
    Some(
        Syntax::DURATION_UNITS
            .iter()
            .map(|name| ((*name).to_string(), (zero, VariantPayload::Unit)))
            .collect(),
    )
}

pub(crate) fn core_event_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    let unit = |names: &[&str]| names.iter().map(|name| ((*name).to_string(), (zero, VariantPayload::Unit))).collect();
    match enum_name {
        "Overflow" => Some(unit(&["Block", "DropNewest", "DropOldest"])),
        "FailurePolicy" => Some(unit(&["StopFirst", "Collect", "Log", "Ignore"])),
        "DispatchState" => Some(unit(&["Delivered", "HandlerFailed", "DroppedNewest", "DroppedOldest", "Closed", "Cancelled", "DeadlineExceeded"])),
        "HookPolicy" => Some(unit(&["FirstCancelElseTransform"])),
        "HookDecision" => Some([
            ("Continue".to_string(), (zero, VariantPayload::Unit)),
            ("Transform".to_string(), (zero, VariantPayload::Single(Type::Named("Unknown".to_string()), zero))),
            ("Cancel".to_string(), (zero, VariantPayload::Unit)),
            ("Fail".to_string(), (zero, VariantPayload::Single(Type::Named("Unknown".to_string()), zero))),
        ].into_iter().collect()),
        "HookOutcome" => Some([
            ("Continue".to_string(), (zero, VariantPayload::Single(Type::Named("Unknown".to_string()), zero))),
            ("Cancel".to_string(), (zero, VariantPayload::Unit)),
            ("Fail".to_string(), (zero, VariantPayload::Single(Type::Named("Unknown".to_string()), zero))),
        ].into_iter().collect()),
        _ => None,
    }
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
        ("JSONLReader", "next", 0) => Some(Some(result_ty(
            Type::Option(Box::new(Type::Named("DataTree".to_string()))),
            error,
        ))),
        ("JSONLWriter", "write", 1) | ("JSONLWriter", "flush" | "finish", 0) => {
            Some(Some(result_ty(unit, error)))
        }
        ("CSVReader", "next", 0) => Some(Some(result_ty(
            Type::Option(Box::new(Type::List(Box::new(Type::String)))),
            error,
        ))),
        ("CSVWriter", "write", 1) | ("CSVWriter", "flush" | "finish", 0) => {
            Some(Some(result_ty(unit, error)))
        }
        ("XMLReader", "next", 0) => Some(Some(result_ty(Type::Option(Box::new(Type::Named("DataTree".to_string()))),error))),
        ("XMLWriter", "write", 1) | ("XMLWriter", "flush" | "finish", 0) => {
            Some(Some(result_ty(unit, error)))
        }
        ("CBORReader", "next", 0) => Some(Some(result_ty(Type::Option(Box::new(Type::Named("DataEvent".to_string()))), error))),
        ("CBORWriter", "write", 1) | ("CBORWriter", "flush" | "finish", 0) => Some(Some(result_ty(unit, error))),
        _ => None,
    }
}

/// E2-M10: field definitions for compiler-known constructable struct types.
/// Returns `Some(fields)` when the named type is a prelude struct users can construct.
pub(crate) fn core_constructable_fields(type_name: &str) -> Option<Vec<(String, Type)>> {
    let str_ty = Type::String;
    match type_name {
        // D-TEXTWIDTH1=B: `TextWidth.{ ambiguous: .Wide, controls: .Reject }`
        // — the two dot-literal enum fields resolve via `resolve_enum_variants_cloned`
        // (below), the same "core enum, not in the user registry" mechanism as
        // `ProcessStreamMode`.
        "TextWidth" => Some(vec![
            ("ambiguous".to_string(), Type::Named("TextWidthAmbiguous".to_string())),
            ("controls".to_string(), Type::Named("TextWidthControls".to_string())),
        ]),
        "IOContext" => Some(vec![
            ("operation".to_string(), Type::Named(Syntax::TYPE_IO_OPERATION.to_string())),
            ("resource".to_string(), Type::Option(Box::new(Type::String))),
            ("os_code".to_string(), Type::Option(Box::new(Type::Int))),
            ("cause".to_string(), Type::Option(Box::new(Type::String))),
        ]),
        "AsyncPolicy" => Some(vec![
            ("capacity".to_string(), Type::Int),
            ("overflow".to_string(), Type::Named("Overflow".to_string())),
        ]),
        // D-SERDE2 / D-SERDE14=A: a hand `decode` builds its own rejection with
        // `DecodeError.{ path: …, reason: … }` and returns it via `Err(…)`. Both
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
        "Limits" => Some(vec![
            ("max_reply_line_bytes".to_string(), Type::Int),
            ("max_reply_lines".to_string(), Type::Int),
            ("max_capabilities".to_string(), Type::Int),
            ("max_recipients".to_string(), Type::Int),
            ("max_message_bytes".to_string(), Type::Int),
            ("max_auth_challenge_bytes".to_string(), Type::Int),
        ]),
        "SmtpConfig" => Some(vec![
            ("host".to_string(), Type::String),
            ("port".to_string(), Type::Int),
            ("security".to_string(), Type::Named("SmtpSecurity".to_string())),
            ("auth".to_string(), Type::Named("SmtpAuth".to_string())),
            ("recipient_policy".to_string(), Type::Named("RecipientPolicy".to_string())),
            ("trust".to_string(), Type::Named("TlsTrust".to_string())),
            ("limits".to_string(), Type::Named("Limits".to_string())),
            ("dkim".to_string(), Type::Option(Box::new(Type::Named("DkimConfig".to_string())))),
        ]),
        "DkimConfig" => Some(vec![
            ("domain".to_string(), Type::String),
            ("selector".to_string(), Type::String),
            (
                "private_key".to_string(),
                crate::Sema::Diagnostics::core_crypto_nominal(Type::Named("Secret".to_string())),
            ),
            ("signed_headers".to_string(), Type::List(Box::new(Type::String))),
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
        "CBOROptions" => Some(vec![
            ("max_depth".to_string(), Type::Int),
            ("max_items".to_string(), Type::Int),
            ("max_bytes".to_string(), Type::Int),
            ("require_canonical".to_string(), Type::Bool),
        ]),
        "CBORError" => Some(vec![
            ("kind".to_string(), Type::Named("CBORErrorKind".to_string())),
            ("byte_offset".to_string(), Type::Int),
            ("path".to_string(), Type::String),
            ("reason".to_string(), Type::String),
        ]),
        "XMLLimits" => Some(vec![
            ("max_depth".to_string(), Type::Int),
            ("max_nodes".to_string(), Type::Int),
            ("max_attributes_per_element".to_string(), Type::Int),
            ("max_name_bytes".to_string(), Type::Int),
            ("max_text_bytes".to_string(), Type::Int),
            ("max_entity_declarations".to_string(), Type::Int),
            ("max_entity_depth".to_string(), Type::Int),
            ("max_entity_replacement_bytes".to_string(), Type::Int),
        ]),
        "XMLParseOptions" => Some(vec![
            ("entities".to_string(), Type::Named("XMLEntityPolicy".to_string())),
            ("limits".to_string(), Type::Named("XMLLimits".to_string())),
        ]),
        "XMLRenderOptions" => Some(vec![
            ("encoding".to_string(), Type::Named("XMLEncoding".to_string())),
            ("lexical".to_string(), Type::Named("XMLLexicalPolicy".to_string())),
        ]),
        "XMLCanonical" => Some(vec![
            ("mode".to_string(), Type::Named("XMLCanonicalMode".to_string())),
            ("comments".to_string(), Type::Bool),
            ("inclusive_prefixes".to_string(), Type::List(Box::new(Type::String))),
        ]),
        "XMLError" => Some(vec![
            ("kind".to_string(), Type::Named("XMLReason".to_string())),
            ("byte_offset".to_string(), Type::Option(Box::new(Type::Int))),
            ("line".to_string(), Type::Option(Box::new(Type::Int))),
            ("column".to_string(), Type::Option(Box::new(Type::Int))),
            ("path".to_string(), Type::String),
            ("reason".to_string(), Type::String),
        ]),
        "RecipientReport" => Some(vec![
            ("address".to_string(), Type::Named("Address".to_string())),
            ("accepted".to_string(), Type::Bool),
            ("code".to_string(), Type::Int),
            ("message".to_string(), Type::String),
        ]),
        "SendReport" => Some(vec![
            ("server".to_string(), Type::String),
            ("accepted".to_string(), Type::List(Box::new(Type::Named("RecipientReport".to_string())))),
            ("rejected".to_string(), Type::List(Box::new(Type::Named("RecipientReport".to_string())))),
            ("response_code".to_string(), Type::Int),
            ("response".to_string(), Type::String),
            ("accepted_at".to_string(), Type::String),
        ]),
        _ => None,
    }
}

/// D-EMAIL-SMTP-SURFACE1=A: closed ungated email policy and error enums.
pub(crate) fn core_email_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (Span, VariantPayload)>> {
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    let units: &[&str] = match enum_name {
        "SmtpSecurity" => &["StartTls", "Tls"],
        "RecipientPolicy" => &["RequireAll", "DeliverAccepted"],
        "EmailError" | "SmtpAuth" | "TlsTrust" => &[],
        _ => return None,
    };
    for name in units {
        variants.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    if enum_name == "EmailError" {
        for name in [
            "Configuration", "Dns", "Connect", "Tls", "Auth", "Protocol", "Rejected",
            "Transient", "TimedOut", "Cancelled", "DeliveryUnknown",
        ] {
            let fields = [
                ("operation", Type::String),
                ("server", Type::Option(Box::new(Type::String))),
                ("code", Type::Option(Box::new(Type::Int))),
                ("reason", Type::String),
            ].into_iter().map(|(field, ty)| VariantField {
                name: field.to_string(), name_span: zero, ty, ty_span: zero,
            }).collect();
            variants.insert(name.to_string(), (zero, VariantPayload::Named(fields)));
        }
    } else if enum_name == "SmtpAuth" {
        variants.insert("None".to_string(), (zero, VariantPayload::Unit));
        variants.insert("Password".to_string(), (zero, VariantPayload::Named(vec![
            VariantField { name: "username".to_string(), name_span: zero, ty: Type::String, ty_span: zero },
            VariantField {
                name: "password".to_string(),
                name_span: zero,
                ty: crate::Sema::Diagnostics::core_crypto_nominal(Type::Named("Secret".to_string())),
                ty_span: zero,
            },
        ])));
    } else if enum_name == "TlsTrust" {
        variants.insert("System".to_string(), (zero, VariantPayload::Unit));
        variants.insert("SystemPlusCa".to_string(), (zero, VariantPayload::Named(vec![
            VariantField { name: "pem".to_string(), name_span: zero,
                ty: Type::List(Box::new(Type::IntN { signed: false, bits: 8 })), ty_span: zero },
        ])));
    }
    Some(variants)
}

pub(crate) fn core_tls_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (Span, VariantPayload)>> {
    let zero = Span::new(0, 0);
    let roots = Type::Named("TlsRootCertificates".to_string());
    let mut variants = std::collections::HashMap::new();
    match enum_name {
        "TlsVersion" => {
            variants.insert("Tls12".to_string(), (zero, VariantPayload::Unit));
            variants.insert("Tls13".to_string(), (zero, VariantPayload::Unit));
        }
        "TlsClientTrust" => {
            variants.insert("System".to_string(), (zero, VariantPayload::Unit));
            variants.insert("SystemPlus".to_string(), (zero, VariantPayload::Single(roots.clone(), zero)));
            variants.insert("CustomOnly".to_string(), (zero, VariantPayload::Single(roots, zero)));
        }
        _ => return None,
    }
    Some(variants)
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
        "CBORErrorKind" => &["Syntax", "Truncated", "Unsupported", "Limit", "TypeMismatch", "TrailingData", "NonCanonical"],
        "XMLReason" => &["InvalidEncoding", "Malformed", "MismatchedTag", "InvalidName", "Namespace", "DuplicateAttribute", "Entity", "EntityCycle", "Limit", "Canonicalization", "Shape", "Unsupported"],
        "XMLEntityPolicy" => &["Preserve", "Reject"],
        "XMLEncoding" => &["UTF8", "UTF8BOM", "UTF16LE", "UTF16BE"],
        "XMLLexicalPolicy" => &["PreserveValid", "Deterministic"],
        "XMLCanonicalMode" => &["Inclusive11", "Exclusive10"],
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
    if enum_name == "XMLEntityPolicy" {
        variants.insert("Resolve".to_string(), (zero, VariantPayload::Single(Type::Map {
            key: Box::new(Type::String), key_span: None, value: Box::new(Type::String),
        }, zero)));
    }
    Some(variants)
}

/// D-AUTH-TOKENPOLICY1=A: inspectable verifier failures.
pub(crate) fn core_auth_variants(
    enum_name: &str,
) -> Option<
    std::collections::HashMap<
        String,
        (crate::Diagnostics::Span, crate::AST::VariantPayload),
    >,
> {
    use crate::AST::{VariantField, VariantPayload};
    use crate::Diagnostics::Span;
    if enum_name != "AuthError" {
        return None;
    }
    let zero = Span::new(0, 0);
    let field = |name: &str, ty: Type| VariantField {
        name: name.to_string(),
        name_span: zero,
        ty,
        ty_span: zero,
    };
    let mut variants = std::collections::HashMap::new();
    for name in ["InvalidSignature", "WeakKey", "TokenExpired"] {
        variants.insert(name.to_string(), (zero, VariantPayload::Unit));
    }
    for name in ["MalformedToken", "UnsupportedToken", "MissingClaim", "DecodeError"] {
        variants.insert(
            name.to_string(),
            (zero, VariantPayload::Single(Type::String, zero)),
        );
    }
    variants.insert(
        "WrongAudience".to_string(),
        (
            zero,
            VariantPayload::Named(vec![
                field("expected", Type::String),
                field("actual", Type::String),
            ]),
        ),
    );
    variants.insert(
        "WrongIssuer".to_string(),
        (
            zero,
            VariantPayload::Named(vec![
                field("expected", Type::String),
                field("actual", Type::Option(Box::new(Type::String))),
            ]),
        ),
    );
    Some(variants)
}
