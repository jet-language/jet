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
    matches!(name, "ScopeGuard" | "Iter")
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

// D-ENC-DYN1=A+: the dynamic encoding value `Data` (+ aliases `JSON`/`TOML`/
// `YAML`/`CSV`).
pub(crate) fn is_json_type_name(name: &str) -> bool {
    Syntax::is_data_type_name(name)
}

// D-DBDRIVER1: the `DBValue` dynamic tagged SQL value.
pub(crate) fn is_db_value_type_name(name: &str) -> bool {
    Syntax::is_db_value_type_name(name)
}

/// D-VALIDATE-DECODE1=B: typed decode returns the accumulated validation list.
/// Structural and validation failures share the one `[FieldError]` contract.
pub(crate) fn decode_error_ty() -> Type {
    Type::List(Box::new(field_error_ty()))
}

/// D-VALIDATE1: one accumulated validation error (`{ path, reason }`).
/// `validate { }` blocks / `Type.validate(value)` /
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

/// D-LAYOUT-CTOR1: the constraint-layout container is named `Layout`. The old
/// `LayoutHandle` spelling is retired (no alias, I8).
pub(crate) fn layout_handle_renamed_to_layout(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2936",
        format!(
            "the constraint-layout type is named `{}`, not `{}`",
            Syntax::LAYOUT_TYPE,
            Syntax::LAYOUT_HANDLE_TYPE_RETIRED
        ),
        format!(
            "`{}` is the solver/container value constructed by `name {} {}.{{ … }}`",
            Syntax::LAYOUT_TYPE,
            Syntax::SIGIL_BIND_IMMUT,
            Syntax::LAYOUT_TYPE
        ),
        format!("write `{}` instead of `{}`", Syntax::LAYOUT_TYPE, Syntax::LAYOUT_HANDLE_TYPE_RETIRED),
        Some(span),
    )
}

/// D-ACRO-CASE1=A / D-ACRO-LEX1=A: a retired word-cased acronym spelling.
pub(crate) fn retired_acronym_spelling_diag(old: &str, canonical: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0358",
        format!("`{old}` is spelled `{canonical}`"),
        "Jet keeps acronyms fully capitalized inside PascalCase names (D-ACRO-CASE1=A, D-ACRO-LEX1=A)".to_string(),
        format!("write `{canonical}` instead of `{old}`"),
        Some(span),
    )
}

pub(crate) fn is_json_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_JSON_ERROR || name == "JSONError"
}

pub(crate) fn is_io_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_IO_ERROR || name == "IOError"
}

pub(crate) fn is_utf8_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_UTF8_ERROR || name == "UTF8Error"
}

/// D-TEXTWIDTH1=B: `text.display_width(s, policy: cjk)`'s reject-path error
/// (a `.Reject` control-character policy hit) — mirrors `UTF8Error`'s
/// minimal `{ message }` shape.
pub(crate) fn is_text_error_type_name(name: &str) -> bool {
    name == "TextError"
}

/// D-FACT-HOME1=A: the fixed marker-argument menu (`Capability`, `InlineMode`,
/// etc.) is a fact vocabulary published for reflection, never a general type —
/// no constructor exists outside `#Marker(param: Name.Variant)` position. Each
/// fix names the real path: the living counterpart when one exists (only
/// `Capability` has one — `Authority`/`[Right]`), otherwise the marker that
/// legitimately writes the name. `Layout` is excluded: it is also a real
/// dot-ctor value type (D-LAYOUT-CTOR1, see the `matches!` in
/// `core_type_known`), so that name resolves before this ever runs.
fn phantom_fact_menu_fix(name: &str) -> Option<&'static str> {
    Some(match name {
        "ABI" => "write it only inside `#ABI(name: system)`",
        "Capability" => "take `Authority` (the rights value), or a rights list `[Right]`; inside a marker, write it in `#Grant(...)` or `#Caps(...)`",
        "FfiLanguage" => "write it only inside `#FFI(language: c)`",
        "InlineMode" => "write it only inside `#Inline(mode: Always)`",
        "IntType" => "write it only inside `#Layout(tag: I32)`",
        "KernelMode" => "write it only inside `#Kernel(mode: parallel)`",
        "Maturity" => "write it only inside `#Meta(maturity: .Tested)`",
        "NamingCase" => "write it only inside `#RenameAll(case: snake)`",
        "ObligationMode" => "write it only inside `#Unsafe(\"reason\", obligations: .Track)`",
        "PolicySetting" => "write it only inside `#Policy(no_alloc)`",
        "Site" => "write it only as `$sites: [...]` on a `marker` declaration",
        "State" => "write it only inside `#State(state: .Draft)` or `#Transition(from:, to:)`",
        "TaintKind" => "it has no live marker: its only user was the retired `#Tainted`, now `#Input`",
        "Target" => "write it only inside `#Target(target: Web)`",
        "Track" => "write `#Track` instead — it takes no arguments",
        _ => return None,
    })
}

/// D-FACT-HOME1=A: "a phantom fact-menu name is refused at the signature, and
/// the diagnostic names the real path rather than a bare unknown-type error."
pub(crate) fn phantom_fact_menu_diag(name: &str, span: Span) -> Option<Diagnostic> {
    let fix = phantom_fact_menu_fix(name)?;
    Some(Diagnostic::error(
        "E0119",
        format!("`{name}` is a fact menu, not a type"),
        format!("`{name}` names a fixed set of marker-argument values; it is never constructed as an ordinary value"),
        fix.to_string(),
        Some(span),
    ))
}

pub(crate) fn core_type_known(name: &str) -> bool {
    matches!(
        name,
        "Unit" | "U8" | "Error" | "ProcessResult" | "ProcessSpec" | "ProcessChild" | "Stopwatch" | "Closed"
        | "Claims" | "AuthError" | "Session" | "Auth"
        | "SyncText" | "SyncCounter" | "SyncMap" | "SyncList" | "RowPolicy"
        // D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum
        // (`.Stream`/`.Inherit`/`.Capture`, D-ENUMDOT2). `ProcessStdin`/
        // `ProcessStdoutStream`/`ProcessStderrStream` are field-access-only
        // handles off a `ProcessChild`; `ProcessLines` is the loop-source-only
        // result of `.lines()` on the latter two (mirrors `FileLines`/`StdinLines`).
        | "ProcessStreamMode" | "ProcessStdin" | "ProcessStdoutStream" | "ProcessStderrStream" | "ProcessLines"
        // D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D: public expert
        // controls. TerminalFact is a namespace of checked String keys, not a
        // fifth value type.
        | "TerminalPolicy" | "TerminalSize" | "TerminalMode" | "TerminalSession"
        | "Range"
        | "IOContext" | "IOOperation"
        // D-TEXTWIDTH1=B: `TextWidth` (dot-ctor struct, `core_constructable_fields`)
        // + its two dot-literal enum fields + the `.Reject` policy error.
        | "TextWidth" | "TextWidthAmbiguous" | "TextWidthControls" | "TextError" | "EnvError"
        // D-DET1: deterministic injected capability handles.
        // D-DET-CAPAPI: `Duration` value type for the widened clock surface.
        | "Clock" | "Rng" | "Duration" | "DurationUnit" | "RangeError" | "Condition"
        | "GameScene" | "GameAssets" | "GameInputMap"
        | "GameBackend" | "GameReplay" | "GameImage" | "GameSound" | "GameFrame"
        | "GameInputSnapshot" | "GameSceneType" | "GameReplayType" | "GameBackendType"
        | "RaylibWindow" | "RaylibColor" | "RaylibSound"
        // D-BIGINT1 / D-DECIMAL1: arbitrary-precision numerics.
        | "BigInt" | "Decimal"
        // D-DBDRIVER1 / D-EFFDBREAD1=A: the `core.db` connection handle and its
        // error. Nameable so a query function can annotate its connection
        // parameter — the shape a `#(DB.Read)` live query (D-LIVEQUERY1) takes.
        | "DBConnection" | "DBScope" | "DBError"
        | "FileReader" | "FileWriter" | "FileLines"
        | "StdinHandle" | "StdinLines" | "Stdout" | "Stderr"
        // D-LSDIR1/D-FSOPS1/D-WATCH-SCOPE1: filesystem and watcher values.
        | "DirEntry" | "Stat" | "WalkEntry" | "TempDir" | "TempFile" | "FileLock"
        | "WatchEvent" | "WatchHandle" | "WatchSet"
        // D-DATA-SURFACE1=A / D-DATA-STATUS1=A: data summary/status values.
        | "DataGroup" | "DataLineOptions" | "DataColumn" | "DataStatus" | "DataSummary"
        | "DataLimits" | "DataError" | "DataErrorKind" | "DataStream" | "DataPivotCell"
        // D-LOGTRACE1=A: typed structured logging values.
        | "LogField" | "LogSpan"
        // D-ITERTOOLS1=A: expanded collection handles.
        | "BitSet" | "ByteBuffer"
        // E2-M10: networking opaque types.
        | "TcpListener" | "TcpStream" | "IPAddr" | "SocketAddr" | "UdpSocket" | "UDPPacket"
        | "DNSSrv" | "UnixListener" | "UnixStream" | "TLSStream" | "TLSClientConfig" | "TLSClientConfigType"
        | "TLSRootCertificates" | "TLSRootCertificatesType" | "TLSClientIdentity" | "TLSClientIdentityType"
        | "TLSClientTrust" | "TLSVersion" | "TLSPeerIdentity" | "TLSCertificate"
        | "NetError" | "NetErrorDetail" | "NetDnsError" | "NetShutdown" | "NetReadyInterest" | "NetReady"
        // D-COMPUTE1=D / D-COMPUTE-TYPE1=D: ranked tensor owner + compute errors.
        | "Tensor" | "ComputeError" | "ComputeDevice" | "ComputeStream" | "VjpRun"
        | "SparseTensor"
        // D-SERVICE1=D: structured service tree handles.
        | "ServiceTree" | "ServiceEndpoint" | "ServiceError" | "ServiceRestart"
        | "ServiceDelivery" | "ServiceRuntime" | "ServiceStateStore" | "ServiceReceipt"
        | "ServiceUpgradeReceipt"
        | "HTTPRequest" | "HTTPResponse" | "HTTPRouter" | "HTTPClient" | "HTTPClientType"
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
        // D-SERDE2 / D-VALIDATE-DECODE1: the format-agnostic value tree.
        | "DataTree"
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
        | "F32x4" | "F64x2" | "ReduceOp"
        | "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4"
        // D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 2, ratified 2026-06-28/29): the
        // built-in constraint-layout value types.
        | "HVar" | "VVar" | "LengthVar" | "Constraint" | "Layout"
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
        // D-WEBAPP1=D: full-stack application builder types.
        | "WebApp"
        | "WebPage"
        | "WebContext"
        | "WebMount"
        | "LiveQuery"
        // D-APPROX1=A: approximate sketch data structures.
        | "HyperLogLog" | "TDigest" | "CountMinSketch" | "ReservoirSampler"
        // D-TIMEDEPTH1=A: civil-time types.
        | "Date" | "LocalDate" | "LocalTime" | "DateTime" | "Instant" | "Period" | "Zone"
        | "ZonedDateTime"
        // D-URL1=A: typed URL and MIME values.
        | "Url" | "Mime"
        // D-EMAIL1=A / D-EMAIL-SMTP-SURFACE1=A: exact ungated email values.
        | "Address" | "Message" | "Attachment" | "Envelope" | "EmailError"
        | "SMTPSecurity" | "RecipientPolicy" | "RecipientReport" | "SendReport"
        | "Limits" | "SMTPAuth" | "TLSTrust" | "DkimConfig" | "SMTPConfig" | "Mailer"
        // D-REGEXENGINE1=A: std-only linear regex values.
        | "Regex" | "RegexFlags" | "Match"
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP types.
        | "HTTPMethod" | "HTTPStatus" | "HTTPVersion" | "HTTPHeaderName" | "HTTPHeaderValue"
        | "HTTPHeaders" | "HTTPBody" | "HTTPBodyChunks" | "HTTPError" | "HTTPOperation" | "HTTPProxy" | "HTTPRedirectPolicy" | "HTTPRetryPolicy" | "HTTPCookieJar" | "HTTPMux" | "HTTPHandler" | "HTTPServerTls" | "HTTPServer" | "HTTPShutdownReport" | "HTTPCorsPolicy" | "HTTPCorsOrigins" | "HTTPCompressEncoding"
        | "WsConn" | "WsError" | "WsMessage"
        | "Browser" | "BrowserContext" | "BrowserPage" | "BrowserFrame" | "BrowserLocator"
        | "BrowserIntercept"
        | "BrowserEvent" | "BrowserTrace" | "BrowserReceipt" | "BrowserPrivacy" | "BrowserError"
        | "BrowserCapabilities"
        | "BrowserProfile" | "BrowserTimeout" | "BrowserProtocol" | "BrowserLocked"
        // D-TYPEDTEXT1=D: typed text — a checked query/markup template built by
        // expected-type elaboration of a string literal (E0149 guards a plain
        // runtime `String` from filling this position).
        | "SQL" | "HTML" | "Sh"
        // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — consuming,
        // fallible, `?`-composed cursors over `[U8]`/`String`.
        | "Reader" | "Cursor"
        // D-MIGRATE3=A: decode-time migration transparency. `DecodeResult<T>`
        // (generic, see `is_core_generic` in CheckerCore.rs) and its plain
        // `MigrationStatus` field both need the bare-name gate here too.
        | "DecodeResult" | "MigrationStatus"
        // D-BUILD*: selected-root build-program handles. No runtime values.
        | "BuildContext" | "BuildPlan" | "BuildAction" | "BuildTarget"
        | "BuildToolchain" | "BuildProbe" | "BuildSigningIdentity" | "ProgramInfo" | "TypeInfo" | "LayoutInfo" | "LayoutField" | "SourceSpan"
        | "CompilerLexed" | "CompilerSyntaxTree" | "CompilerChecked"
        | "CompilerSemanticIndex" | "CompilerDefinition" | "CompilerSymbolKind"
        | "CompilerParam" | "CompilerField" | "CompilerViewProvenance"
        | "CompilerViewSourcePath" | "CompilerViewSource" | "CompilerViewProjection"
        | "CompilerReference" | "CompilerDefinitionAnchor" | "CompilerCall"
        | "CompilerEffect" | "CompilerEffectProvenance" | "CompilerOutput"
        | "CompilerOutputEntry"
        | "CompilerSourceMap" | "CompilerToken" | "CompilerNode"
        | "CompilerDiagnostic" | "CompilerGeneratedLine" | "CompilerError"
        | "MarkerInfo" | "MarkerArgInfo" | "StateInfo" | "TransitionInfo" | "FactInfo"
        | "PackageInfo" | "FunctionInfo" | "EffectInfo" | "MethodInfo" | "FieldInfo" | "TypeParamInfo"
    ) || is_json_type_name(name)
        || is_json_error_type_name(name)
        || is_io_error_type_name(name)
        || is_utf8_error_type_name(name)
}

/// D-RULEARG-TYPES1=A: enum variants generated from the marker registry.
pub(crate) fn core_lang_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (Span, VariantPayload)>> {
    let declaration = crate::Policy::rule_arg_declaration(enum_name)?;
    let zero = Span::new(0, 0);
    Some(
        declaration
            .variants
            .iter()
            .map(|variant| {
                (
                    (*variant).to_string(),
                    (zero, VariantPayload::Unit),
                )
            })
            .collect(),
    )
}

pub(crate) fn core_struct_field(type_name: &str, field: &str) -> Option<Type> {
    if type_name == Syntax::TYPE_RANGE {
        return match field {
            "start" | "end" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "TLSPeerIdentity" {
        return match field {
            "verified_server_name" => Some(Type::String),
            "leaf" => Some(Type::Named("TLSCertificate".to_string())),
            "certificate_chain" => Some(Type::List(Box::new(Type::Named("TLSCertificate".to_string())))),
            _ => None,
        };
    }
    if type_name == "TLSCertificate" {
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
    if type_name == "Session" {
        return match field {
            "id" | "user_id" | "cookie" => Some(Type::String),
            "expires_at" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "Auth" {
        return match field {
            "users_table" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "ProcessChild" && field == "terminal" {
        return Some(Type::Option(Box::new(Type::Named(
            Syntax::TYPE_TERMINAL_SESSION.to_string(),
        ))));
    }
    if type_name == Syntax::TYPE_IO_CONTEXT {
        return match field {
            "operation" => Some(Type::Named(Syntax::TYPE_IO_OPERATION.to_string())),
            "resource" | "cause" => Some(Type::Option(Box::new(Type::String))),
            "os_code" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if type_name == "HTTPShutdownReport" && matches!(field, "accepted" | "overloaded" | "completed" | "cancelled") {
        return Some(Type::Int);
    }
    if matches!(type_name, "TerminalSize" | "TerminalPolicy" | "EncodingLimits" | "EncodingCause" | "EncodingError" | "CBOROptions" | "CBORError" | "XMLLimits" | "XMLParseOptions" | "XMLError" | "AsyncPolicy" | "RecipientReport" | "SendReport" | "Limits" | "DkimConfig" | "SMTPConfig") {
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
    if type_name == "CompilerToken" {
        return match field {
            "kind" | "text" => Some(Type::String),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if matches!(type_name, "CompilerLexed" | "CompilerSyntaxTree" | "CompilerChecked")
        && field == "source"
    {
        return Some(Type::String);
    }
    if matches!(type_name, "CompilerLexed" | "CompilerSyntaxTree" | "CompilerChecked" | "CompilerSourceMap")
        && field == "schema_version"
    {
        return Some(Type::Int);
    }
    if type_name == "CompilerNode" {
        return match field {
            "kind" => Some(Type::String),
            "name" => Some(Type::Option(Box::new(Type::String))),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == "CompilerDiagnostic" {
        return match field {
            "code" | "severity" | "message" | "why" | "fix" => Some(Type::String),
            "span" => Some(Type::Option(Box::new(Type::Named(
                Syntax::TYPE_SOURCE_SPAN.to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "CompilerGeneratedLine" {
        return match field {
            "generated_line" | "source_line" => Some(Type::Int),
            "source" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "CompilerSemanticIndex" {
        return match field {
            "schema_version" => Some(Type::Int),
            "source_digest" => Some(Type::String),
            "definitions" => Some(Type::List(Box::new(Type::Named(
                "CompilerDefinition".to_string(),
            )))),
            "references" => Some(Type::List(Box::new(Type::Named(
                "CompilerReference".to_string(),
            )))),
            "calls" => Some(Type::List(Box::new(Type::Named(
                "CompilerCall".to_string(),
            )))),
            "effects" => Some(Type::List(Box::new(Type::Named(
                "CompilerEffect".to_string(),
            )))),
            "outputs" => Some(Type::List(Box::new(Type::Named(
                "CompilerOutput".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "CompilerDefinition" {
        return match field {
            "identity" | "name" | "module" => Some(Type::String),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            "kind" => Some(Type::Named("CompilerSymbolKind".to_string())),
            "view_provenance" => Some(Type::List(Box::new(Type::Named(
                "CompilerViewProvenance".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "CompilerSymbolKind" {
        return match field {
            "kind" => Some(Type::String),
            "params" => Some(Type::List(Box::new(Type::Named(
                "CompilerParam".to_string(),
            )))),
            "ret" | "parent" | "ty" => Some(Type::Option(Box::new(Type::String))),
            "fields" => Some(Type::List(Box::new(Type::Named(
                "CompilerField".to_string(),
            )))),
            "variants" => Some(Type::List(Box::new(Type::String))),
            "mutable" => Some(Type::Option(Box::new(Type::Bool))),
            _ => None,
        };
    }
    if type_name == "CompilerParam" {
        return match field {
            "name" | "ty" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "CompilerField" {
        return match field {
            "name" | "ty" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "CompilerViewProvenance" {
        return match field {
            "output_path" => Some(Type::List(Box::new(Type::String))),
            "sources" => Some(Type::List(Box::new(Type::Named(
                "CompilerViewSourcePath".to_string(),
            )))),
            "mutable" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "CompilerViewSourcePath" {
        return match field {
            "source" => Some(Type::Named("CompilerViewSource".to_string())),
            "projections" => Some(Type::List(Box::new(Type::Named(
                "CompilerViewProjection".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "CompilerViewSource" {
        return match field {
            "kind" => Some(Type::String),
            "index" => Some(Type::Option(Box::new(Type::Int))),
            "module" | "name" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "CompilerViewProjection" {
        return match field {
            "kind" => Some(Type::String),
            "name" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "CompilerReference" {
        return match field {
            "name" | "module" => Some(Type::String),
            "scope_identity" => Some(Type::Option(Box::new(Type::String))),
            "target" => Some(Type::Option(Box::new(Type::Named(
                "CompilerDefinitionAnchor".to_string(),
            )))),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == "CompilerDefinitionAnchor" {
        return match field {
            "module" | "kind" => Some(Type::String),
            "semantic_identity" => Some(Type::Option(Box::new(Type::String))),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == "CompilerCall" {
        return match field {
            "caller" | "callee" | "module" => Some(Type::String),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == "CompilerEffect" {
        return match field {
            "function" => Some(Type::String),
            "direct" | "callees" | "inferred" => Some(Type::List(Box::new(Type::String))),
            "maximal" => Some(Type::Bool),
            "provenance" => Some(Type::List(Box::new(Type::Named(
                "CompilerEffectProvenance".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "CompilerEffectProvenance" {
        return match field {
            "effect" => Some(Type::String),
            "call_path" => Some(Type::List(Box::new(Type::String))),
            "spans" => Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_SOURCE_SPAN.to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "CompilerOutput" {
        return match field {
            "binding" | "kind" | "name" | "module" => Some(Type::String),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            "entry" => Some(Type::Named("CompilerOutputEntry".to_string())),
            _ => None,
        };
    }
    if type_name == "CompilerOutputEntry" {
        return match field {
            "identity" | "name" | "module" | "authority" => Some(Type::String),
            "definition_span" | "reference_span" => {
                Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string()))
            }
            "params" | "effects" => Some(Type::List(Box::new(Type::String))),
            "return_type" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    match type_name {
        "CompilerLexed" => return match field {
            "schema_version" => Some(Type::Int),
            "tokens" => Some(Type::List(Box::new(Type::Named("CompilerToken".to_string())))),
            "diagnostics" => Some(Type::List(Box::new(Type::Named("CompilerDiagnostic".to_string())))),
            _ => None,
        },
        "CompilerSyntaxTree" => return match field {
            "schema_version" => Some(Type::Int),
            "items" => Some(Type::List(Box::new(Type::Named("CompilerNode".to_string())))),
            "diagnostics" => Some(Type::List(Box::new(Type::Named("CompilerDiagnostic".to_string())))),
            _ => None,
        },
        "CompilerChecked" => return match field {
            "schema_version" => Some(Type::Int),
            "syntax" => Some(Type::Named("CompilerSyntaxTree".to_string())),
            "diagnostics" => Some(Type::List(Box::new(Type::Named("CompilerDiagnostic".to_string())))),
            "functions" => Some(Type::List(Box::new(Type::Named("FunctionInfo".to_string())))),
            "effects" => Some(Type::List(Box::new(Type::Named("EffectInfo".to_string())))),
            // A failed check has diagnostics but no trustworthy semantic
            // index. Keep that absence explicit at the typed API boundary;
            // callers must not mistake an empty index for a checked file.
            "semantic_index" => Some(Type::Option(Box::new(Type::Named(
                "CompilerSemanticIndex".to_string(),
            )))),
            _ => None,
        },
        _ => {}
    }
    if type_name == "CompilerSourceMap" {
        return match field {
            "schema_version" => Some(Type::Int),
            "sources" => Some(Type::List(Box::new(Type::String))),
            "generated_lines" => Some(Type::List(Box::new(Type::Named("CompilerGeneratedLine".to_string())))),
            _ => None,
        };
    }
    if type_name == "CompilerError" {
        return match field {
            "code" | "message" => Some(Type::String),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_TYPE_INFO {
        return match field {
            "name" | "module" | "identity" | "kind" => Some(Type::String),
            "layout" => Some(Type::Named(Syntax::TYPE_LAYOUT_INFO.to_string())),
            "fields" => Some(Type::List(Box::new(Type::Named("FieldInfo".to_string())))),
            "methods" => Some(Type::List(Box::new(Type::Named("MethodInfo".to_string())))),
            "type_params" => Some(Type::List(Box::new(Type::Named("TypeParamInfo".to_string())))),
            "markers" => Some(Type::List(Box::new(Type::Named("MarkerInfo".to_string())))),
            "expanded_markers" => Some(Type::List(Box::new(Type::Named("MarkerInfo".to_string())))),
            "implements" => Some(Type::List(Box::new(Type::String))),
            "states" => Some(Type::List(Box::new(Type::Named("StateInfo".to_string())))),
            "transitions" => Some(Type::List(Box::new(Type::Named("TransitionInfo".to_string())))),
            "facts" => Some(Type::List(Box::new(Type::Named("FactInfo".to_string())))),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_LAYOUT_INFO {
        return match field {
            "kind" | "target" | "guarantee" | "source" => Some(Type::String),
            "size" | "alignment" | "stride" => Some(Type::Option(Box::new(Type::Int))),
            "fields" => Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_LAYOUT_FIELD.to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_LAYOUT_FIELD {
        return match field {
            "name" | "ty" | "target" | "guarantee" | "source" => Some(Type::String),
            "offset" | "size" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if type_name == "MarkerInfo" {
        return match field {
            "name" => Some(Type::String),
            "args" => Some(Type::List(Box::new(Type::Named("MarkerArgInfo".to_string())))),
            _ => None,
        };
    }
    if type_name == "MarkerArgInfo" {
        return match field {
            "name" | "ty" => Some(Type::String),
            "value" => Some(Type::Union(
                std::iter::once(Type::String)
                    .chain([Type::Int, Type::Bool])
                    .chain(
                        crate::Policy::RULE_ARG_DECLARATIONS
                            .iter()
                            .map(|declaration| Type::Named(declaration.name.to_string())),
                    )
                    .collect(),
            )),
            _ => None,
        };
    }
    if type_name == "StateInfo" {
        return match field {
            "name" | "path" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "TransitionInfo" {
        return match field {
            "operation" | "from" | "to" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "FactInfo" {
        return match field {
            "kind" | "name" | "path" => Some(Type::String),
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
            "params" => Some(Type::List(Box::new(Type::String))),
            "markers" => Some(Type::List(Box::new(Type::Named("MarkerInfo".to_string())))),
            "is_pub" => Some(Type::Bool),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == "FieldInfo" {
        return match field {
            "name" | "ty" => Some(Type::String),
            "markers" => Some(Type::List(Box::new(Type::Named("MarkerInfo".to_string())))),
            "is_pub" => Some(Type::Bool),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == "TypeParamInfo" {
        return match field {
            "name" => Some(Type::String),
            "bounds" => Some(Type::List(Box::new(Type::String))),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
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
    // D-VALIDATE1 / D-VALIDATE-DECODE1: FieldError carries one path/reason.
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
    if type_name == "DataLineOptions" {
        return match field {
            "title" | "x_label" | "y_label" | "style" | "color" | "legend" => {
                Some(Type::String)
            }
            "markers" => Some(Type::Bool),
            "reference" => Some(Type::Option(Box::new(Type::Float))),
            _ => None,
        };
    }
    if type_name == "DataPivotCell" {
        return match field {
            "row_key" | "column_key" => Some(Type::String),
            "count" => Some(Type::Int),
            "sum" | "mean" => Some(Type::Float),
            _ => None,
        };
    }
    if type_name == "DataLimits" {
        return match field {
            "encoding" => Some(Type::Named("EncodingLimits".to_string())),
            "max_groups" | "max_sort_rows" | "max_join_rows" | "max_output_rows" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "DataError" {
        return match field {
            "kind" => Some(Type::Named("DataErrorKind".to_string())),
            "operation" | "reason" => Some(Type::String),
            "row" | "column" | "index" => Some(Type::Option(Box::new(Type::Int))),
            "cause" => Some(Type::Option(Box::new(Type::Named("EncodingError".to_string())))),
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
            "step" | "path" | "copy" | "ownership" | "trust" | "fallback" | "replacement" => {
                Some(Type::String)
            }
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
        ("HTTPRequest", "method" | "path") => Some(Type::String),
        ("HTTPRequest", "body") => Some(Type::Named("HTTPBody".to_string())),
        ("HTTPRequest", "headers") => Some(Type::Named("HTTPHeaders".to_string())),
        ("HTTPResponse", "status") => Some(Type::Int),
        ("HTTPResponse", "body") => Some(Type::Named("HTTPBody".to_string())),
        ("HTTPResponse", "headers") => Some(Type::Named("HTTPHeaders".to_string())),
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
        "E0764",
        format!("`game.run` has no `{label}:` option at argument {}", index + 1),
        format!("this position accepts {expected}"),
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
    if type_name == "VjpRun" && args.len() == 1 {
        return match field {
            "value" => Some(Type::Named("Tensor".to_string())),
            "pull" => Some(Type::Fn {
                params: vec![Type::Named("Tensor".to_string())],
                ret: Some(Box::new(args[0].clone())),
                effect_bound: None,
                param_contract: None,
                return_view_provenance: None,
            }),
            "grads" => Some(args[0].clone()),
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

/// D-PROCESS-SESSION2=D: expert terminal mode has the two owner-ratified
/// variants. The portable default is Cooked; Raw is explicit.
pub(crate) fn core_terminal_mode_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    ["Raw", "Cooked"]
        .into_iter()
        .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
        .collect()
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
        "Cancelled", "Unsupported", "TLS", "Protocol", "Other",
    ] {
        variants.insert(name.to_string(), (
            zero,
            VariantPayload::Single(Type::Named("NetErrorDetail".to_string()), zero),
        ));
    }
    variants.insert("DNS".to_string(), (
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
    if enum_name == "HTTPOperation" {
        for name in ["ClientConnect", "ServerBind", "ServeListener"] {
            variants.insert(name.to_string(), (zero, VariantPayload::Unit));
        }
        return Some(variants);
    }
    // D-HTTP-CORS1=A: a CORS policy names either every origin or a list.
    if enum_name == "HTTPCorsOrigins" {
        variants.insert("Any".to_string(), (zero, VariantPayload::Unit));
        variants.insert(
            "List".to_string(),
            (
                zero,
                VariantPayload::Single(Type::List(Box::new(Type::String)), zero),
            ),
        );
        return Some(variants);
    }
    if enum_name == "HTTPProxy" {
        variants.insert("FromEnvironment".to_string(), (zero, VariantPayload::Unit));
        variants.insert("None".to_string(), (zero, VariantPayload::Unit));
        variants.insert(
            "Url".to_string(),
            (zero, VariantPayload::Single(Type::String, zero)),
        );
        return Some(variants);
    }
    if enum_name == "HTTPRedirectPolicy" {
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
    if enum_name == "HTTPRetryPolicy" {
        // D-HTTP-CLIENT2=A: `.None` / `.Safe` / `.Idempotent`.
        for name in ["None", "Safe", "Idempotent"] {
            variants.insert(name.to_string(), (zero, VariantPayload::Unit));
        }
        return Some(variants);
    }
    if enum_name == "HTTPCookieJar" {
        variants.insert("Memory".to_string(), (zero, VariantPayload::Unit));
        return Some(variants);
    }
    if enum_name == "HTTPCompressEncoding" {
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
            "IO".to_string(),
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
    if enum_name != "HTTPError" {
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
        ("TLS", "stage", Type::String),
        ("Timeout", "phase", Type::String),
        ("Proxy", "stage", Type::String),
        ("Redirect", "reason", Type::String),
        ("Protocol", "version", Type::String),
        ("IO", "operation", Type::String),
        // D-HTTP-CORS1=A: a policy value was refused when it was built.
        ("Policy", "reason", Type::String),
        ("ResourceUnavailable", "resource", Type::String),
        ("Internal", "incident_id", Type::String),
        ("UnsupportedTarget", "operation", Type::Named("HTTPOperation".to_string())),
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

/// D-REDUCE-VALUE1=A: the closed Core enum passed to SIMD `reduce`.
pub(crate) fn core_reduce_op_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    if enum_name != Syntax::TYPE_REDUCE_OP {
        return None;
    }
    let zero = Span::new(0, 0);
    Some(
        ["Add", "Mul", "Min", "Max", "Avg"]
            .into_iter()
            .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
            .collect(),
    )
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

/// D-SERVICE-AUTHORITY1: durable delivery receipts are a closed sum. The
/// receipt, not a Boolean, tells callers whether the message was accepted,
/// replayed, retained, dead-lettered, rejected, or unavailable.
pub(crate) fn core_service_receipt_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::{VariantField, VariantPayload};
    use crate::Diagnostics::Span;
    if enum_name != "ServiceReceipt" {
        return None;
    }
    let zero = Span::new(0, 0);
    Some(
        [
            ("Accepted", VariantPayload::Single(Type::String, zero)),
            ("Duplicate", VariantPayload::Single(Type::String, zero)),
            (
                "Retained",
                VariantPayload::Named(vec![
                    VariantField {
                        name: "id".to_string(),
                        name_span: zero,
                        ty: Type::String,
                        ty_span: zero,
                    },
                    VariantField {
                        name: "until".to_string(),
                        name_span: zero,
                        ty: Type::Int,
                        ty_span: zero,
                    },
                ]),
            ),
            ("DeadLettered", VariantPayload::Single(Type::String, zero)),
            ("Rejected", VariantPayload::Single(Type::String, zero)),
            ("Unavailable", VariantPayload::Single(Type::String, zero)),
        ]
        .into_iter()
        .map(|(name, payload)| (name.to_string(), (zero, payload)))
        .collect(),
    )
}

/// D-SERVICE1=D: service failures remain a typed closed sum across AOT,
/// ambient, and persisted/comptime boundaries.
pub(crate) fn core_service_error_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    if enum_name != "ServiceError" {
        return None;
    }
    let zero = Span::new(0, 0);
    Some(
        [
            "Full",
            "Ambiguous",
            "Unknown",
            "NotStarted",
            "Policy",
            "Unavailable",
            "Partitioned",
            "Revoked",
            "Stale",
            "Expired",
        ]
        .into_iter()
        .map(|name| {
            (
                name.to_string(),
                (zero, VariantPayload::Single(Type::String, zero)),
            )
        })
        .collect(),
    )
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
        // DataStream<T>.next is handled specially in method_calls (needs T).
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
        // D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D: explicit terminal
        // controls use named fields so misspellings fail in sema.
        "TerminalSize" => Some(vec![
            ("cols".to_string(), Type::Int),
            ("rows".to_string(), Type::Int),
        ]),
        "TerminalPolicy" => Some(vec![
            (
                "size".to_string(),
                Type::Named(Syntax::TYPE_TERMINAL_SIZE.to_string()),
            ),
            (
                "mode".to_string(),
                Type::Named(Syntax::TYPE_TERMINAL_MODE.to_string()),
            ),
        ]),
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
        // D-VALIDATE1 / D-VALIDATE-DECODE1: `FieldError.{ path: …, reason: … }`.
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
        "DataLimits" => Some(vec![
            ("encoding".to_string(), Type::Named("EncodingLimits".to_string())),
            ("max_groups".to_string(), Type::Int),
            ("max_sort_rows".to_string(), Type::Int),
            ("max_join_rows".to_string(), Type::Int),
            ("max_output_rows".to_string(), Type::Int),
        ]),
        "DataLineOptions" => Some(vec![
            ("title".to_string(), Type::String),
            ("x_label".to_string(), Type::String),
            ("y_label".to_string(), Type::String),
            ("markers".to_string(), Type::Bool),
            ("reference".to_string(), Type::Option(Box::new(Type::Float))),
            ("style".to_string(), Type::String),
            ("color".to_string(), Type::String),
            ("legend".to_string(), Type::String),
        ]),
        "DataError" => Some(vec![
            ("kind".to_string(), Type::Named("DataErrorKind".to_string())),
            ("operation".to_string(), Type::String),
            ("row".to_string(), Type::Option(Box::new(Type::Int))),
            ("column".to_string(), Type::Option(Box::new(Type::Int))),
            ("index".to_string(), Type::Option(Box::new(Type::Int))),
            ("reason".to_string(), Type::String),
            ("cause".to_string(), Type::Option(Box::new(Type::Named("EncodingError".to_string())))),
        ]),
        "DataPivotCell" => Some(vec![
            ("row_key".to_string(), Type::String),
            ("column_key".to_string(), Type::String),
            ("count".to_string(), Type::Int),
            ("sum".to_string(), Type::Float),
            ("mean".to_string(), Type::Float),
        ]),
        "Limits" => Some(vec![
            ("max_reply_line_bytes".to_string(), Type::Int),
            ("max_reply_lines".to_string(), Type::Int),
            ("max_capabilities".to_string(), Type::Int),
            ("max_recipients".to_string(), Type::Int),
            ("max_message_bytes".to_string(), Type::Int),
            ("max_auth_challenge_bytes".to_string(), Type::Int),
        ]),
        "SMTPConfig" => Some(vec![
            ("host".to_string(), Type::String),
            ("port".to_string(), Type::Int),
            ("security".to_string(), Type::Named("SMTPSecurity".to_string())),
            ("auth".to_string(), Type::Named("SMTPAuth".to_string())),
            ("recipient_policy".to_string(), Type::Named("RecipientPolicy".to_string())),
            ("trust".to_string(), Type::Named("TLSTrust".to_string())),
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
        "SMTPSecurity" => &["StartTls", "TLS"],
        "RecipientPolicy" => &["RequireAll", "DeliverAccepted"],
        "EmailError" | "SMTPAuth" | "TLSTrust" => &[],
        _ => return None,
    };
    for name in units {
        variants.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    if enum_name == "EmailError" {
        for name in [
            "Configuration", "DNS", "Connect", "TLS", "Auth", "Protocol", "Rejected",
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
    } else if enum_name == "SMTPAuth" {
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
    } else if enum_name == "TLSTrust" {
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
    let roots = Type::Named("TLSRootCertificates".to_string());
    let mut variants = std::collections::HashMap::new();
    match enum_name {
        "TLSVersion" => {
            variants.insert("Tls12".to_string(), (zero, VariantPayload::Unit));
            variants.insert("Tls13".to_string(), (zero, VariantPayload::Unit));
        }
        "TLSClientTrust" => {
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
        "DataErrorKind" => &["Decode", "Limit", "IO", "Empty", "InvalidArgument", "NonFinite", "Overflow", "State", "Bridge"],
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
