use super::alloc_ptrs::{io_error_ty, result_ty};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::expr_root_ident;
use crate::Syntax;
use crate::AST::{Expr, Type, VariantField, VariantPayload};

/// D-MUSTUSE1 (c18iwxqx): built-in handle types whose bare statement result must
/// not be silently ignored (E0419). `scope.guard` returns `ScopeGuard` — bind it
/// or cleanup runs at end of the statement, not scope exit. `TransactionGuard` is
/// a phantom return from `on_commit`/`on_rollback` (registration is side-effect);
/// those calls are intentionally ignorable. `Task` stays on L1101.
pub(crate) fn core_must_use_type(name: &str) -> bool {
    matches!(name, "ScopeGuard" | "Iter" | "Delivery")
}

/// D-SERVICE-RECEIPT2=A: a bound `Delivery` owns one linear observation or
/// control obligation. The handle itself is not a cancellation token: moving
/// it to a return, storage, or another call transfers that obligation.
pub(crate) fn core_single_use_type(name: &str) -> bool {
    name == "Delivery"
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
            Type::Named("Message".to_string()),
            Type::Named("EmailError".to_string()),
        ))),
        (Type::Named(name), "send", 1) if name == "Mailer" => Some(Some(result_ty(
            Type::Named("SendReport".to_string()),
            Type::Named("EmailError".to_string()),
        ))),
        _ => None,
    }
}

pub(crate) fn json_ty() -> Type {
    Type::Named(Syntax::TYPE_DATA.to_string())
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
        format!(
            "write `{}` instead of `{}`",
            Syntax::LAYOUT_TYPE,
            Syntax::LAYOUT_HANDLE_TYPE_RETIRED
        ),
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

/// D-FACT-HOME1=A: the fixed marker-argument menu (`Effect`, `InlineMode`,
/// etc.) is a fact vocabulary published for reflection, never a general type —
/// no constructor exists outside `#Marker(param: Name.Variant)` position. Each
/// fix names the real path: the living counterpart when one exists (only
/// `Effect` has no general-position use here; it is written inside `#FX`,
/// otherwise the marker that
/// legitimately writes the name. `Layout` is excluded: it is also a real
/// dot-ctor value type (D-LAYOUT-CTOR1, see the `matches!` in
/// `core_type_known`), so that name resolves before this ever runs.
fn phantom_fact_menu_fix(name: &str) -> Option<&'static str> {
    Some(match name {
        "ABI" => "write it only inside `#ABI(name: system)`",
        "Effect" => "write it only inside `#FX(Net, FS)` or another `#FX(...)` effect scope",
        "FfiLanguage" => "write it only inside `#FFI(language: c)`",
        "InlineMode" => "write it only inside `#Inline(mode: Always)`",
        "IntType" => "write it only inside `#Layout(tag: I32)`",
        "KernelMode" => "write it only inside `#Kernel(mode: parallel)`",
        "MemoBound" => "write `#Memo`, or the explicit `#Memo(bound: none)` form",
        "Maturity" => "write it only inside `#Meta(maturity: .Tested)`",
        "NamingCase" => "write it only inside `#RenameAll(case: snake)`",
        "ObligationMode" => "write it only inside `#Unsafe(\"reason\", obligations: .Track)`",
        "PolicySetting" => "write it only inside `#Policy(gc)`, `#Policy(copies: .Explicit)`, or another registered non-memory policy",
        "Site" => "write it only as `@sites: [...]` on a `marker` declaration",
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

/// D-ABILITY-NAME2=A: the old authority vocabulary is refused at a type site.
/// These spellings are not aliases. Reusing E0119 keeps the retirement in the
/// existing registered type-diagnostic family while the message gives the
/// exact source replacement.
pub(crate) fn retired_authority_vocabulary_diag(name: &str, span: Span) -> Option<Diagnostic> {
    let fix = match name {
        "Ability" => "write `Effect` only inside `#FX(...)`",
        "Abilities" | "Capability" | "Caps" => "write `Authority` for the rights value",
        _ => return None,
    };
    Some(Diagnostic::error(
        "E0119",
        format!("`{name}` is retired"),
        "Jet has one effect menu, one Authority value, and one `#FX` scope; the old authority spellings are not types".to_string(),
        fix.to_string(),
        Some(span),
    ))
}

pub(crate) fn core_type_known(name: &str) -> bool {
    if Syntax::typed_head_kind(name).is_some_and(|kind| kind.is_typed_text()) {
        return true;
    }
    if Syntax::is_simd_lane_type(name) {
        return true;
    }
    matches!(
        name,
        "Unit" | "U8" | Syntax::TYPE_ERR | Syntax::TYPE_NEVER | Syntax::TYPE_TASK_FAILURE
        | "ProcessResult" | "ProcessReceipt" | "ProcessPlan" | "ProcessSpec" | "ProcessChild"
        | "Stopwatch" | "Closed" | "RealtimeStream" | "RealtimeReceipt"
        | "SyncText" | "SyncCounter" | "SyncMap" | "SyncList" | "RowPolicy"
        // D-FOUND-LIFECYCLE1=A: typed process signals are a closed Core enum.
        | "ProcessSignal"
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
        | Syntax::TYPE_VIEW_ITER
        | Syntax::TYPE_RANGE | Syntax::TYPE_ALLOC_ERROR
        // D-TEXTWIDTH1=B: `TextWidth` (dot-ctor struct, `core_constructable_fields`)
        // + its two dot-literal enum fields + the `.Reject` policy error.
        | "TextWidth" | "TextWidthAmbiguous" | "TextWidthControls" | "TextError" | "EnvError"
        // D-DET1: deterministic injected capability handles.
        // D-DET-CAPAPI: `Duration` value type for the widened clock surface.
        // D-AUTHORITY-NAME1=A: one ordinary, nameable rights carrier.
        | Syntax::TYPE_AUTHORITY
        | "Clock" | "Rng" | Syntax::DETERMINISTIC_WORLD_TYPE | "Fake" | "Duration" | "DurationUnit" | "RangeError" | "Condition"
        // D-SHARED-REVISION1=A: the opaque owner-bound snapshot carrier and
        // its typed wrong-owner/exhaustion failures.
        | Syntax::TYPE_SHARED_SNAPSHOT | Syntax::TYPE_SHARED_REVISION_ERROR
        | "Path"
        | "StreamEventTime" | "KeyedStream" | "Window" | "LateEventDisposition"
        | "TestSuite" | "TestComparison" | "Count" | "HandleId" | "TaskId" | "EventId"
        | "HistoryRng" | "HistoryValue" | "HistoryPrecondition" | "HistoryCase"
        | "HistoryOperation" | "HistoryScheduleChoice" | "HistoryBounds"
        | "HistoryDistribution" | "HistoryStrategy" | "TypedHistoryCase"
        | "GameBackend" | "GameReplay" | "GameImage" | "GameSound" | "GameFrame"
        | "GameInputSnapshot" | "GameSceneType" | "GameReplayType" | "GameBackendType"
        | "RaylibWindow" | "RaylibColor" | "RaylibSound"
        | "RaylibTextureAtlas"
        // D-DECIMAL1: exact decimal arithmetic.
        | "Decimal" | "Fraction"
        // D-TYPE2-IMAG1=A: imaginary literals construct the shared Complex value.
        | "Complex"
        // D-DBDRIVER1 / D-EFFDBREAD1=A: the `core.db` connection handle and its
        // error. Nameable so a query function can annotate its connection
        // parameter — the shape a `#(DB.Read)` live query (D-LIVEQUERY1) takes.
        | "DBConnection" | "DBScope" | "DbPool" | "DbLease" | "DbPoolReceipt" | "DBError"
        // D-LIB-CALLGRANT1=A: a loaded Mod is opaque; its read roots are the
        // only constructable part of the host grant value.
        | "Mod" | "ModGrant"
        | "FileReader" | "FileWriter" | "FileLines" | "FileScope" | "MappedFile"
        // D-LSDIR1/D-FSOPS1/D-WATCH-SCOPE1: filesystem and watcher values.
        | "DirEntry" | "Stat" | "WalkEntry" | "TempDir" | "TempFile" | "FileLock"
        // stdlib-api-laws D4: `WatchEvent.domain`/`.kind` are closed enums.
        | "WatchEvent" | "WatchDomain" | "WatchKind" | "WatchHandle" | "WatchSet"
        // D-DATA-SURFACE1=A / D-DATA-STATUS1=A: data summary/status values.
        | "DataLineOptions" | "DataColumn" | "DataFormat" | "DataSchema"
        | "DataStatus" | "DataSummary"
        | "Query" | "DataGroupedQuery" | "DataTracked" | "DataWatch"
        | "DataWatchStatus" | "Group"
        | "DataLimits" | "DataError" | "DataErrorKind" | "DataStream" | "DataPivotCell"
        | "DataSourceIdentity" | "DataProvenance" | "DataSnapshotIdentity" | "DataLoaderStatus"
        | "DataLoader" | "DataSnapshot"
        | "JetDataPlotMark" | "JetDataPlotChannel" | "JetDataPlotAggregate"
        | "JetDataPlotFilterOp" | "JetDataPlotValue" | "JetDataPlotScaleKind"
        | "JetDataPlotDomain" | "JetDataPlotLegendPosition" | "JetDataPlotFacetKind"
        | "JetDataPlotInteraction" | "JetDataPlotBackend" | "JetDataPlotSupport"
        | "JetDataPlotErrorKind" | "JetDataPlotRenderFormat" | "JetDataPlotField"
        | "JetDataPlotSchema" | "JetDataPlotSourceFacts" | "JetDataPlotEncoding"
        | "JetDataPlotTransform" | "JetDataPlotScale" | "JetDataPlotAxis" | "JetDataPlotLegend"
        | "JetDataPlotFacet" | "JetDataPlotLayer" | "JetDataPlotAccessibility" | "JetDataPlotLayout"
        | "JetDataPlotCapability" | "JetDataPlotError" | "JetDataPlotPlan"
        | "JetDataPlotSelectedRow" | "JetDataPlotInspection" | "JetDataPlotRender"
        | "JetDataPlotProjection" | "JetDataPlotColumn"
        // D-DX-QUEUE1=A: durable queue carriers. Provider policy/request
        // internals remain private; these are the caller-visible records.
        | "JobQueue" | "JobPayload" | "JobResult" | "JobError"
        | "JobQueueReceipt" | "JobQueueClaim" | "JobQueueEvent"
        | "JobQueueRecord" | "JobQueueStatus"
        | "JobQueueState" | "JobQueueDeliveryPolicy"
        // D-LOGTRACE1=A: typed structured logging values.
        | "LogField" | "LogSpan"
        // D-ITERTOOLS1=A: expanded collection handles.
        | Syntax::TYPE_BITS | Syntax::TYPE_BYTES
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
        | "ServiceTree" | "ServiceWorkflow" | "ServiceEndpoint" | "ServiceError" | "ServiceRestart"
        | "ServiceDelivery" | "ServiceRuntime" | "ServiceStateStore" | "Delivery"
        | "DeliveryReceipt" | "DeliveryEvent"
        | "ServiceUpgradeReceipt" | "TaskOutcome" | "TaskStatus"
        | "HTTPRequest" | "HTTPResponse" | "HTTPRouter" | "HTTPClient" | "HTTPClientType"
        // D-CRYPTO-API1=A: purpose-bound crypto values. Secret-bearing values
        // are opaque and receive no structural/collection capabilities.
        | "Secret" | "SigningKey" | "VerifyKey" | "X25519SecretKey" | "X25519PublicKey"
        | "SharedSecret" | "Signature" | "Sealed" | "WrappedKey" | "PasswordHash"
        | "Digest256" | "Digest512" | "Hasher" | "CryptoError"
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
        | "EncodingFormat"
        | "EncodingErrorKind" | "DataEvent"
        | "CBOROptions" | "CBORError" | "CBORErrorKind"
        | "XMLLimits" | "XMLParseOptions" | "XMLRenderOptions" | "XMLEncoding"
        | "XMLLexicalPolicy" | "XMLCanonical" | "XMLCanonicalMode" | "XMLError" | "XMLReason" | "XMLEntityPolicy"
        | "JSONReader" | "JSONWriter" | "JSONLReader" | "JSONLWriter"
        | "CSVReader" | "CSVWriter" | "CSVRow" | "XMLReader" | "XMLWriter"
        | "CBORReader" | "CBORWriter"
        // D-SIMD2 / D-LINALG1: built-in SIMD lane + linear-algebra value types.
        | "F32x4" | "F64x2" | "ReduceOp"
        | "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4"
        // D-SPACE-GEOMETRY1=A: typed coordinate spaces.  Space names are
        // nominal so `Point2<Float, Screen>` cannot silently cross a frame.
        | "Point2" | "Delta2" | "Transform" | "Transform2" | "Ray2"
        | "Screen" | "World" | "View" | "Camera" | "Device"
        | "ScreenPoint" | "WorldPoint" | "ViewPoint" | "CameraPoint" | "DevicePoint"
        | "ScreenDelta" | "WorldDelta" | "ViewDelta" | "CameraDelta" | "DeviceDelta"
        | "TransformError" | "FrameId"
        // D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 2, ratified 2026-06-28/29): the
        // built-in constraint-layout value types.
        | "HVar" | "VVar" | "LengthVar" | "Constraint" | "Layout"
        // D-REACT1=B: opt-in reactive handle types (used bare as `Signal<T>`/`Derived<T>`).
        | "Signal" | "Derived" | "Computed" | "Effect"
        // D-EVENT1=D: first-party typed Event/Hook family.
        | "Event" | "Hook" | "DecisionHook" | "HookPolicy" | "HookDecision" | "HookOutcome"
        | "Subscription" | "EventScope" | "EventPolicy" | "EventTrace"
        | "AsyncEvent" | "AsyncPolicy" | "Overflow" | "FailurePolicy" | "DispatchReport" | "DispatchFailure" | "DispatchState" | "EventConfigError"
        // D-FFI-CALLBACK2=A: managed event values and consuming owners.
        | "FfiCallbackEvent" | "FfiCallbackRegistration"
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
        // D-FOUND-PLATFORM1=A: shared font and host service value vocabulary.
        | "FontFace" | "FontStyle" | "Glyph" | "GlyphRun" | "GlyphShaper"
        | "UiCapability" | "UiCapabilityFact" | "UiCapabilityFacts" | "UiCancellation"
        | "UiHostError" | "UiServiceResult" | "UiFileDialogKind" | "UiFsAccess"
        | "UiFsRights" | "UiFsGrant" | "UiGrantedPath" | "UiFileFilter"
        | "UiClipboardWrite" | "UiTextRange" | "UiImeMode"
        | "UiPlayground" | "UiPreview" | "UiPreviewAccessibility" | "UiPreviewAuthority"
        | "UiPreviewContext" | "UiPreviewDevice" | "UiPreviewEffect"
        | "UiPreviewInputOverride" | "UiPreviewInputValue" | "UiPreviewKind"
        | "UiPreviewLifecycle" | "UiPreviewRegistry" | "UiPreviewSource"
        | "UiPreviewTheme" | "UiPreviewTraits" | "UiPreviewViewport"
        | "UiImePhase"
        | "UiDragPhase" | "UiDragEvent" | "UiShortcutModifier" | "UiShortcutModifiers"
        | "UiShortcut" | "UiShortcutBinding" | "UiShortcutDispatch"
        | "UiAccessibilityState" | "UiAccessibility" | "UiNodeId"
        | "UiAccessibilityProjection"
        // c-devserver (owner-directed 2026-07-01): the configurable `jet dev`
        // server value returned by `core.web.devserver.for_app(...)`.
        | "DevServer"
        // D-WEBAPP1=D: full-stack application builder types.
        | "App"
        | "WebPage"
        | "WebContext"
        | "WebMount"
        | "LiveQuery"
        // D-FLAGSHIP-WEBAPI1=A: headless first-party web suite values.
        | "WebRouterValueType" | "WebRouterField" | "WebRouterSearchCodec"
        | "WebRouterCacheStatus" | "WebRouterCacheState" | "WebNavigationStatus"
        | "WebNavigation" | "WebNavigationState"
        | "WebMutationStatus" | "WebMutationState"
        | "WebQueryStatus" | "WebQueryNetworkMode" | "WebQueryState" | "WebQuery"
        | "WebFormValueType" | "WebFormStatus" | "WebFormFieldState"
        | "WebFormFieldSpec" | "WebFormInput" | "WebFormDecodedInput"
        | "WebFormActionError" | "WebFormErrorState" | "WebFormLifecycleStatus"
        | "WebFormLifecycle" | "WebFormTyped" | "WebFormValidationChain"
        | "WebFormTypedValidation" | "WebFormTypedSubmission"
        | "WebFormState" | "WebForm" | "WebFormValidation"
        | "WebTableSortDirection" | "WebTablePageMode" | "WebTableSort" | "WebTableFilter"
        | "WebTableState" | "WebTableColumn" | "WebTablePage" | "WebTableRow" | "WebTable"
        | "WebVirtualWindow" | "WebVirtualPlan" | "WebVirtualPlanViewport"
        | "WebStoreTransaction" | "WebStoreEvent" | "WebStoreInspection"
        | "WebStorePatch" | "WebStore" | "WebStoreSubscription"
        // D-APPROX1=A: approximate sketch data structures.
        | "HyperLogLog" | "TDigest" | "CountMinSketch" | "ReservoirSampler"
        // D-TIMEDEPTH1=A: civil-time types.
        | "Date" | "LocalDate" | "LocalTime" | "DateTime" | "Instant" | "Period" | "Zone"
        | "ZonedDateTime"
        // D-URL1=A: typed URL and MIME values.
        | "Url" | Syntax::TYPE_URL | "Mime"
        // D-EMAIL1=A / D-EMAIL-SMTP-SURFACE1=A: exact ungated email values.
        | "Address" | "Message" | "Attachment" | "Envelope" | "EmailError"
        | "SMTPSecurity" | "RecipientPolicy" | "RecipientReport" | "SendReport"
        | "Limits" | "SMTPAuth" | "TLSTrust" | "DkimConfig" | "SMTPConfig" | "Mailer"
        | "Regex" | "RegexFlags" | "Match"
        | "HTTPMethod" | "HTTPStatus" | "HTTPVersion" | "HTTPHeaderName" | "HTTPHeaderValue"
        | "HTTPHeaders" | "HTTPBody" | "HTTPBodyChunks" | "HTTPError" | "HTTPOperation" | "HTTPProxy" | "HTTPRedirectPolicy" | "HTTPRetryPolicy" | "HTTPCookieJar" | "HTTPMux" | "HTTPHandler" | "HTTPServerTls" | "HTTPServer" | "HTTPShutdownReport" | "HTTPCorsPolicy" | "HTTPCorsOrigins" | "HTTPCompressEncoding"
        | "WsConn" | "WsError" | "WsMessage"
        | "Browser" | "BrowserContext" | "BrowserPage" | "BrowserFrame" | "BrowserLocator"
        | "BrowserIntercept"
        | "BrowserEvent" | "BrowserTrace" | "BrowserReceipt" | "BrowserPrivacy" | "BrowserError"
        | "BrowserAbilities"
        | "BrowserProfile" | "BrowserTimeout" | "BrowserProtocol" | "BrowserLocked"
        | "BrowserTestConfig" | "BrowserTestSource" | "BrowserTestAction"
        | "BrowserTestSnapshot" | "BrowserTestEventFact" | "BrowserTestArtifact"
        | "BrowserTestAttempt" | "BrowserTestCase" | "BrowserTestReport"
        | "BrowserTestFixture" | "BrowserTestServer"
        // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — consuming,
        // fallible, `?`-composed cursors over `[U8]`/`String`.
        | "Reader" | "Cursor"
        // D-BUILD*: selected-root build-program handles and the read-only
        // graph/diff records returned by `core.build`.
        | "BuildContext"
        | "BuildPlan"
        | "BuildAction"
        | "BuildTarget"
        | "BuildToolchain"
        | "BuildProbe"
        | "BuildSigningIdentity"
        | "BuildError"
        | "BuildGraph"
        | "BuildGraphTarget"
        | "BuildGraphAction"
        | "BuildGraphFile"
        | "BuildGraphNode"
        | "BuildGraphInputDigest"
        | "BuildGraphActionKey"
        | "BuildGraphFileDelta"
        | "BuildGraphKeyDelta"
        | "BuildGraphCacheDelta"
        | "BuildGraphDiff"
        | "ProgramInfo"
        | "MemoStats"
        | "TypeInfo"
        | "LayoutInfo"
        | "LayoutField"
        | "SourceSpan"
        | "CompilerViewSourcePath" | "CompilerViewSource" | "CompilerViewProjection"
        | "CompilerReference" | "CompilerDefinitionAnchor" | "CompilerCall"
        | "CompilerEffect" | "CompilerEffectProvenance" | "CompilerOutput"
        | "CompilerOutputEntry" | "CompilerStructuralNode" | "CompilerArithmeticOperation"
        | "CompilerSourceMap" | "CompilerToken" | "CompilerNode"
        | "CompilerDiagnostic" | "CompilerGeneratedLine" | "CompilerError"
        | "CompilerPackageError" | "CompilerDependency" | "CompilerPackageTarget"
        | "CompilerPackageOutput" | "CompilerBuildProfile" | "CompilerManifest"
        | "CompilerPackage" | "CompilerLockedPackage" | "CompilerLock"
        | "CompilerKeyValue" | "CompilerProfile" | "CompilerProfileSet"
        | "MarkerInfo" | "MarkerArgInfo" | "StateInfo" | "TransitionInfo" | "FactInfo"
        | "FactKind" | "FactValue" | "DimensionInfo" | "DimensionAxis"
        | "MeasureInfo" | "ExactnessInfo" | "ExactnessKind" | "LayoutFact"
        | "ClassificationInfo" | "NominalInfo" | "ObligationInfo"
        | "ObligationParamInfo" | "ParamZone"
        | "SendabilityInfo" | "MovednessInfo" | "AttributionInfo" | "OriginInfo"
        | "ViewProvenanceInfo" | "UnitScaleProvenanceInfo" | "UnitScaleProvenanceKind"
        | "MaturityInfo" | "Maturity"
        | "PackageInfo" | "FunctionInfo" | "EffectInfo" | "ArithmeticOperationInfo" | "MethodInfo" | "FieldInfo" | "TypeParamInfo"
    ) || is_json_type_name(name)
        || is_io_error_type_name(name)
        || is_utf8_error_type_name(name)
}

/// D-CONC-FAIL1=A: the task wait report is a normal closed enum. Its variants
/// are synthesized here because the runtime owns the type, but user code can
/// construct and match the same values on every tier.
pub(crate) fn core_task_failure_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (Span, VariantPayload)>> {
    if enum_name != Syntax::TYPE_TASK_FAILURE {
        return None;
    }
    let zero = Span::new(0, 0);
    Some(
        [
            ("Cancelled".to_string(), (zero, VariantPayload::Unit)),
            ("DeadlineBlown".to_string(), (zero, VariantPayload::Unit)),
            (
                "Panicked".to_string(),
                (zero, VariantPayload::Single(Type::String, zero)),
            ),
        ]
        .into_iter()
        .collect(),
    )
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
            .map(|variant| ((*variant).to_string(), (zero, VariantPayload::Unit)))
            .collect(),
    )
}
/// D-TEST-STRATEGY1=A: the history value and precondition enums are public
/// records, but their payloads remain closed so every tier shares one shape.
pub(crate) fn core_history_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (Span, VariantPayload)>> {
    let zero = Span::new(0, 0);
    let field = |name: &str, ty: Type| VariantField {
        name: name.to_string(),
        name_span: zero,
        ty,
        ty_span: zero,
    };
    let variants = match enum_name {
        "HistoryValue" => vec![
            ("Integer".to_string(), VariantPayload::Single(Type::Int, zero)),
            ("Boolean".to_string(), VariantPayload::Single(Type::Bool, zero)),
            ("Text".to_string(), VariantPayload::Single(Type::String, zero)),
            (
                "Handle".to_string(),
                VariantPayload::Single(Type::Named("HandleId".to_string()), zero),
            ),
            ("Redacted".to_string(), VariantPayload::Single(Type::String, zero)),
        ],
        "HistoryPrecondition" => vec![
            (
                "HandleLive".to_string(),
                VariantPayload::Single(Type::Named("HandleId".to_string()), zero),
            ),
            (
                "HandleState".to_string(),
                VariantPayload::Named(vec![
                    field("handle", Type::Named("HandleId".to_string())),
                    field("state", Type::String),
                ]),
            ),
            (
                "TaskCompleted".to_string(),
                VariantPayload::Single(Type::Named("TaskId".to_string()), zero),
            ),
            (
                "EventAvailable".to_string(),
                VariantPayload::Single(Type::Named("EventId".to_string()), zero),
            ),
        ],
        _ => return None,
    };
    Some(variants.into_iter().map(|(name, payload)| (name, (zero, payload))).collect())
}

/// Core-declared enum variants are synthesized from the canonical Core
/// declaration table, keeping sema's ordinary enum resolver aligned with the
/// source used for module/type exports.
pub(crate) fn core_declared_enum_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (Span, VariantPayload)>> {
    if let Some(variants) = core_history_variants(enum_name) {
        return Some(variants);
    }
    let names = jet_foundation::CoreModuleExports::core_enum_variants(enum_name)?;
    let zero = Span::new(0, 0);
    Some(
        names
            .iter()
            .map(|name| ((*name).to_string(), (zero, VariantPayload::Unit)))
            .collect(),
    )
}

/// D-FACT-READ1=A: reflection fact kinds are a closed typed menu, not strings
/// smuggled through `FactInfo.kind`.
pub fn core_fact_kind_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (Span, VariantPayload)>> {
    let variants: Vec<String> = match enum_name {
        "FactKind" => {
            let mut names = vec!["Effect", "State", "Tag"];
            for row in jet_foundation::Registry::fact_rows()
                .filter(|row| row.kind() == jet_foundation::Registry::RowKind::Plane)
            {
                if let Some(kind) = jet_foundation::Registry::reflection_kind(row.name) {
                    if !names.contains(&kind) {
                        names.push(kind);
                    }
                }
            }
            names.into_iter().map(str::to_string).collect()
        }
        "Maturity" => ["Experimental", "Tested", "Hardened"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        "ExactnessKind" => ["Exact", "Approximate", "Measured"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        "ParamZone" => ["PositionalOnly", "Either", "LabelOnly"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        "UnitScaleProvenanceKind" => ["Rational", "SymbolicPi", "Conventional", "Measured"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        _ => return None,
    };
    Some({
        let zero = Span::new(0, 0);
        variants
            .into_iter()
            .map(|variant| (variant, (zero, VariantPayload::Unit)))
            .collect()
    })
}

pub(crate) fn core_struct_field(type_name: &str, field: &str) -> Option<Type> {
    if type_name == "CSVRow" {
        return match field {
            "fields" => Some(Type::List(Box::new(Type::String))),
            "line" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_MEMO_STATS {
        return match field {
            "hits" | "misses" | "size" => Some(Type::Int),
            "bound" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_ALLOC_ERROR {
        return match field {
            "requested_bytes" => Some(Type::Int),
            "allocator" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_ERR {
        return match field {
            "message" => Some(Type::String),
            "code" => Some(Type::Option(Box::new(Type::String))),
            "cause" => Some(Type::Option(Box::new(Type::Named(
                Syntax::TYPE_ERR.to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_RANGE {
        return match field {
            "start" | "end" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "TestSuite" {
        return matches!(field, "iteration" | "result").then_some(Type::Int);
    }
    if type_name == "TestComparison" {
        return match field {
            "first_difference" => Some(Type::Int),
            "seed" => Some(Type::Option(Box::new(Type::Int))),
            "status" | "relation" | "source" | "tool" | "target" | "reason" => {
                Some(Type::String)
            }
            "universal_proof" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "TLSPeerIdentity" {
        return match field {
            "verified_server_name" => Some(Type::String),
            "leaf" => Some(Type::Named("TLSCertificate".to_string())),
            "certificate_chain" => Some(Type::List(Box::new(Type::Named(
                "TLSCertificate".to_string(),
            )))),
            "cipher_suite" => Some(Type::String),
            "tls_version" => Some(Type::Named("TLSVersion".to_string())),
            _ => None,
        };
    }
    if type_name == "TLSCertificate" {
        return match field {
            "der" | "sha256" | "spki_sha256" => Some(Type::List(Box::new(Type::IntN {
                signed: false,
                bits: 8,
            }))),
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
            "not_before" => Some(Type::Option(Box::new(Type::Int))),
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
    if type_name == "ModGrant" {
        return match field {
            "read" => Some(Type::List(Box::new(Type::String))),
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
    if type_name == Syntax::TYPE_BUILD_GRAPH {
        let graph_list = |name: &str| Type::List(Box::new(Type::Named(name.to_string())));
        return match field {
            "targets" => Some(graph_list(Syntax::TYPE_BUILD_GRAPH_TARGET)),
            "actions" => Some(graph_list(Syntax::TYPE_BUILD_GRAPH_ACTION)),
            "action_keys" => Some(graph_list(Syntax::TYPE_BUILD_GRAPH_ACTION_KEY)),
            "cache_hits" => Some(graph_list(Syntax::TYPE_BUILD_GRAPH_ACTION)),
            "files" | "affected_files" => Some(graph_list(Syntax::TYPE_BUILD_GRAPH_FILE)),
            "nodes" => Some(graph_list(Syntax::TYPE_BUILD_GRAPH_NODE)),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_TARGET {
        return match field {
            "id" => Some(Type::Int),
            "name" => Some(Type::String),
            "kind" => Some(Type::String),
            "deps" | "actions" => Some(Type::List(Box::new(Type::Int))),
            "files" => Some(Type::List(Box::new(Type::String))),
            "plugin" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_ACTION {
        return match field {
            "id" => Some(Type::Int),
            "name" | "kind" | "key" => Some(Type::String),
            "inputs" | "outputs" | "caps" | "pools" => {
                Some(Type::List(Box::new(Type::String)))
            }
            "target" | "plugin" => Some(Type::Option(Box::new(Type::Int))),
            "legacy_wrapper" => Some(Type::Option(Box::new(Type::String))),
            "compiler_owned" => Some(Type::Bool),
            "cache_hit" => Some(Type::Option(Box::new(Type::Bool))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_FILE {
        return match field {
            "path" => Some(Type::String),
            "owner" => Some(Type::Option(Box::new(Type::Int))),
            "consumers" | "targets" => Some(Type::List(Box::new(Type::Int))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_NODE {
        return match field {
            "kind" | "key" | "subject" => Some(Type::String),
            "inputs" => Some(Type::List(Box::new(Type::String))),
            "input_digests" => Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_BUILD_GRAPH_INPUT_DIGEST.to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_INPUT_DIGEST {
        return match field {
            "name" | "digest" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_ACTION_KEY {
        return match field {
            "action" | "key" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_FILE_DELTA {
        return match field {
            "path" => Some(Type::String),
            "before" | "after" => Some(Type::Option(Box::new(Type::Named(
                Syntax::TYPE_BUILD_GRAPH_FILE.to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_KEY_DELTA {
        return match field {
            "action" => Some(Type::String),
            "before" | "after" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_CACHE_DELTA {
        return match field {
            "action" => Some(Type::String),
            "before" | "after" => Some(Type::Option(Box::new(Type::Bool))),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_BUILD_GRAPH_DIFF {
        return match field {
            "file_deltas" => Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_BUILD_GRAPH_FILE_DELTA.to_string(),
            )))),
            "affected_files" => Some(Type::List(Box::new(Type::String))),
            "key_deltas" => Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_BUILD_GRAPH_KEY_DELTA.to_string(),
            )))),
            "cache_deltas" => Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_BUILD_GRAPH_CACHE_DELTA.to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "HTTPShutdownReport"
        && matches!(field, "accepted" | "overloaded" | "completed" | "cancelled")
    {
        return Some(Type::Int);
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
    if let Some(field_type) = compiler_package_struct_field(type_name, field) {
        return Some(field_type);
    }
    if type_name == "CompilerToken" {
        return match field {
            "kind" | "text" => Some(Type::String),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if matches!(
        type_name,
        "CompilerLexed" | "CompilerSyntaxTree" | "CompilerChecked"
    ) && field == "source"
    {
        return Some(Type::String);
    }
    if matches!(
        type_name,
        "CompilerLexed" | "CompilerSyntaxTree" | "CompilerChecked" | "CompilerSourceMap"
    ) && field == "schema_version"
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
            "structural_nodes" => Some(Type::List(Box::new(Type::Named(
                "CompilerStructuralNode".to_string(),
            )))),
            "effects" => Some(Type::List(Box::new(Type::Named(
                "CompilerEffect".to_string(),
            )))),
            "arithmetic" => Some(Type::List(Box::new(Type::Named(
                "CompilerArithmeticOperation".to_string(),
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
    if type_name == "CompilerArithmeticOperation" {
        return match field {
            "operation" | "policy" | "module" => Some(Type::String),
            "operation_span" | "scope_span" => {
                Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string()))
            }
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
    if type_name == "CompilerStructuralNode" {
        return match field {
            "id" | "ordinal" => Some(Type::Int),
            "parent" => Some(Type::Option(Box::new(Type::Int))),
            "slot" | "slot_kind" | "class" | "shape" | "module" => Some(Type::String),
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
            "identity" | "name" | "module" | "failure_contract" | "failure_source"
            | "authority" => Some(Type::String),
            "definition_span" | "reference_span" => {
                Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string()))
            }
            "params" | "effects" => Some(Type::List(Box::new(Type::String))),
            "return_type" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    match type_name {
        "CompilerLexed" => {
            return match field {
                "schema_version" => Some(Type::Int),
                "tokens" => Some(Type::List(Box::new(Type::Named(
                    "CompilerToken".to_string(),
                )))),
                "diagnostics" => Some(Type::List(Box::new(Type::Named(
                    "CompilerDiagnostic".to_string(),
                )))),
                _ => None,
            }
        }
        "CompilerSyntaxTree" => {
            return match field {
                "schema_version" => Some(Type::Int),
                "items" => Some(Type::List(Box::new(Type::Named(
                    "CompilerNode".to_string(),
                )))),
                "diagnostics" => Some(Type::List(Box::new(Type::Named(
                    "CompilerDiagnostic".to_string(),
                )))),
                _ => None,
            }
        }
        "CompilerChecked" => {
            return match field {
                "schema_version" => Some(Type::Int),
                "syntax" => Some(Type::Named("CompilerSyntaxTree".to_string())),
                "diagnostics" => Some(Type::List(Box::new(Type::Named(
                    "CompilerDiagnostic".to_string(),
                )))),
                "functions" => Some(Type::List(Box::new(Type::Named(
                    "FunctionInfo".to_string(),
                )))),
                "effects" => Some(Type::List(Box::new(Type::Named("EffectInfo".to_string())))),
                // A failed check has diagnostics but no trustworthy semantic
                // index. Keep that absence explicit at the typed API boundary;
                // callers must not mistake an empty index for a checked file.
                "semantic_index" => Some(Type::Option(Box::new(Type::Named(
                    "CompilerSemanticIndex".to_string(),
                )))),
                _ => None,
            };
        }
        _ => {}
    }
    if type_name == "CompilerSourceMap" {
        return match field {
            "schema_version" => Some(Type::Int),
            "sources" => Some(Type::List(Box::new(Type::String))),
            "generated_lines" => Some(Type::List(Box::new(Type::Named(
                "CompilerGeneratedLine".to_string(),
            )))),
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
            "name" | "path" | "module" | "identity" | "kind" => Some(Type::String),
            "layout" => Some(Type::Named(Syntax::TYPE_LAYOUT_INFO.to_string())),
            "fields" => Some(Type::List(Box::new(Type::Named("FieldInfo".to_string())))),
            "methods" => Some(Type::List(Box::new(Type::Named("MethodInfo".to_string())))),
            "type_params" => Some(Type::List(Box::new(Type::Named(
                "TypeParamInfo".to_string(),
            )))),
            "markers" => Some(Type::List(Box::new(Type::Named("MarkerInfo".to_string())))),
            "expanded_markers" => Some(Type::List(Box::new(Type::Named("MarkerInfo".to_string())))),
            "implements" => Some(Type::List(Box::new(Type::String))),
            "states" => Some(Type::List(Box::new(Type::Named("StateInfo".to_string())))),
            "transitions" => Some(Type::List(Box::new(Type::Named(
                "TransitionInfo".to_string(),
            )))),
            "facts" => Some(Type::List(Box::new(Type::Named("FactInfo".to_string())))),
            "dimensions" => Some(Type::List(Box::new(Type::Named(
                "DimensionInfo".to_string(),
            )))),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == Syntax::TYPE_LAYOUT_INFO {
        return match field {
            "kind" | "target" | "guarantee" | "source" => Some(Type::String),
            "size" | "alignment" | "stride" | "requested_alignment" | "effective_alignment" => {
                Some(Type::Option(Box::new(Type::Int)))
            }
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
            "args" => Some(Type::List(Box::new(Type::Named(
                "MarkerArgInfo".to_string(),
            )))),
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
            "name" => Some(Type::String),
            "path" => Some(Type::Named("StateRef".to_string())),
            "terminal" => Some(Type::Bool),
            "reachable" => Some(Type::Option(Box::new(Type::Bool))),
            _ => None,
        };
    }
    if type_name == "StateRef" {
        return match field {
            "owner" | "name" | "path" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "TransitionInfo" {
        return match field {
            "operation" => Some(Type::String),
            "from" | "to" => Some(Type::Named("StateRef".to_string())),
            _ => None,
        };
    }
    if type_name == "FactInfo" {
        return match field {
            "kind" => Some(Type::Named("FactKind".to_string())),
            "name" | "path" => Some(Type::String),
            "value" => Some(Type::Named("FactValue".to_string())),
            _ => None,
        };
    }
    if type_name == "FactValue" {
        return match field {
            "kind" => Some(Type::Named("FactKind".to_string())),
            "name" => Some(Type::String),
            "members" => Some(Type::List(Box::new(Type::String))),
            "range" => Some(Type::Option(Box::new(Type::Named(
                Syntax::TYPE_RANGE.to_string(),
            )))),
            "dimension" => Some(Type::Option(Box::new(Type::Named(
                "DimensionInfo".to_string(),
            )))),
            "measure" => Some(Type::Option(Box::new(Type::Named(
                "MeasureInfo".to_string(),
            )))),
            "exactness" => Some(Type::Option(Box::new(Type::Named(
                "ExactnessInfo".to_string(),
            )))),
            "layout" => Some(Type::Option(Box::new(Type::Named(
                "LayoutFact".to_string(),
            )))),
            "classification" => Some(Type::Option(Box::new(Type::Named(
                "ClassificationInfo".to_string(),
            )))),
            "nominal" => Some(Type::Option(Box::new(Type::Named(
                "NominalInfo".to_string(),
            )))),
            "obligation" => Some(Type::Option(Box::new(Type::Named(
                "ObligationInfo".to_string(),
            )))),
            "state" => Some(Type::Option(Box::new(Type::Named("StateRef".to_string())))),
            "sendability" => Some(Type::Option(Box::new(Type::Named(
                "SendabilityInfo".to_string(),
            )))),
            "view_provenance" => Some(Type::Option(Box::new(Type::Named(
                "ViewProvenanceInfo".to_string(),
            )))),
            "movedness" => Some(Type::Option(Box::new(Type::Named(
                "MovednessInfo".to_string(),
            )))),
            "attribution" => Some(Type::Option(Box::new(Type::Named(
                "AttributionInfo".to_string(),
            )))),
            "origin" => Some(Type::Option(Box::new(Type::Named(
                "OriginInfo".to_string(),
            )))),
            "unit_scale_provenance" => Some(Type::Option(Box::new(Type::Named(
                "UnitScaleProvenanceInfo".to_string(),
            )))),
            "maturity" => Some(Type::Option(Box::new(Type::Named(
                "MaturityInfo".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "MeasureInfo" {
        return match field {
            "kind" => Some(Type::String),
            "value" => Some(Type::Option(Box::new(Type::Int))),
            "symbol" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "ExactnessInfo" {
        return match field {
            "kind" => Some(Type::Named("ExactnessKind".to_string())),
            "precision" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if type_name == "LayoutFact" {
        return match field {
            "bytes" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "ClassificationInfo" || type_name == "NominalInfo" {
        return match field {
            "name" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "ObligationParamInfo" {
        return match field {
            "name" => Some(Type::String),
            "zone" => Some(Type::Named("ParamZone".to_string())),
            _ => None,
        };
    }
    if type_name == "ObligationInfo" {
        return match field {
            "effect_bound" => Some(Type::List(Box::new(Type::String))),
            "param_contract" => Some(Type::List(Box::new(Type::Named(
                "ObligationParamInfo".to_string(),
            )))),
            "variadic" => Some(Type::List(Box::new(Type::Bool))),
            _ => None,
        };
    }
    if type_name == "SendabilityInfo" || type_name == "MovednessInfo" {
        return match field {
            "known" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "ViewProvenanceInfo" {
        return match field {
            "sources" => Some(Type::List(Box::new(Type::String))),
            "mutable" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "AttributionInfo" {
        return match field {
            "source" => Some(Type::String),
            "code" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "OriginInfo" {
        return match field {
            "tracked" => Some(Type::Bool),
            "source" => Some(Type::Option(Box::new(Type::String))),
            "line" | "column" => Some(Type::Option(Box::new(Type::Int))),
            "ambiguity" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "UnitScaleProvenanceInfo" {
        return match field {
            "kind" => Some(Type::Named("UnitScaleProvenanceKind".to_string())),
            "value" | "source" | "uncertainty" => Some(Type::Option(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "MaturityInfo" {
        return match field {
            "level" => Some(Type::Named("Maturity".to_string())),
            _ => None,
        };
    }
    if type_name == "DimensionInfo" {
        return match field {
            "axes" => Some(Type::List(Box::new(Type::Named(
                "DimensionAxis".to_string(),
            )))),
            "identity" | "display" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "DimensionAxis" {
        return match field {
            "name" => Some(Type::String),
            "exponent" => Some(Type::Int),
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
            "arithmetic" => Some(Type::List(Box::new(Type::Named(
                "ArithmeticOperationInfo".to_string(),
            )))),
            "facts" => Some(Type::List(Box::new(Type::Named("FactInfo".to_string())))),
            _ => None,
        };
    }
    if type_name == "ArithmeticOperationInfo" {
        return match field {
            "operation" | "policy" => Some(Type::String),
            "operation_span" | "scope_span" => {
                Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string()))
            }
            _ => None,
        };
    }
    if type_name == "PackageInfo" {
        return match field {
            "name" | "identity" => Some(Type::String),
            "types" => Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_TYPE_INFO.to_string(),
            )))),
            "functions" => Some(Type::List(Box::new(Type::Named(
                "FunctionInfo".to_string(),
            )))),
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
            "effects" => Some(Type::Named("EffectInfo".to_string())),
            "markers" => Some(Type::List(Box::new(Type::Named("MarkerInfo".to_string())))),
            "dimensions" => Some(Type::List(Box::new(Type::Named(
                "DimensionInfo".to_string(),
            )))),
            "facts" => Some(Type::List(Box::new(Type::Named("FactInfo".to_string())))),
            "arithmetic" => Some(Type::List(Box::new(Type::Named(
                "ArithmeticOperationInfo".to_string(),
            )))),
            "is_pub" => Some(Type::Bool),
            "span" => Some(Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string())),
            _ => None,
        };
    }
    if type_name == "FieldInfo" {
        return match field {
            "name" | "ty" => Some(Type::String),
            "index" => Some(Type::Int),
            "fields" => Some(Type::List(Box::new(Type::Named("FieldInfo".to_string())))),
            "markers" => Some(Type::List(Box::new(Type::Named("MarkerInfo".to_string())))),
            "dimensions" => Some(Type::List(Box::new(Type::Named(
                "DimensionInfo".to_string(),
            )))),
            "facts" => Some(Type::List(Box::new(Type::Named("FactInfo".to_string())))),
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
    if type_name == "RangeError" {
        return match field {
            "reason" => Some(Type::String),
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
    if type_name == "DataLineOptions" {
        return match field {
            "title" | "x_label" | "y_label" | "style" | "color" | "legend" => Some(Type::String),
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
            "cause" => Some(Type::Option(Box::new(Type::Named(
                "EncodingError".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "DataColumn" {
        return match field {
            "id" | "name" | "type_name" => Some(Type::String),
            "nullable" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "DataSchema" {
        return match field {
            "identity" => Some(Type::String),
            "format" => Some(Type::Named("DataFormat".to_string())),
            "columns" => Some(Type::List(Box::new(Type::Named("DataColumn".to_string())))),
            "projection" => Some(Type::Option(Box::new(Type::Named(
                "ShapeProjection".to_string(),
            )))),
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
    if type_name == "DataAuthority" {
        return match field {
            "scope" | "revision" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "DataSourceIdentity" {
        return match field {
            "kind" => Some(Type::Named("DataLoaderKind".to_string())),
            "locator" | "member" => Some(Type::String),
            "parameters" => Some(Type::List(Box::new(Type::String))),
            _ => None,
        };
    }
    if type_name == "DataProvenance" {
        return match field {
            "source" => Some(Type::Named("DataSourceIdentity".to_string())),
            "format" => Some(Type::Named("DataFormat".to_string())),
            "authority" => Some(Type::Named("DataAuthority".to_string())),
            _ => None,
        };
    }
    if type_name == "DataSnapshotIdentity" {
        return match field {
            "id" | "source" | "content" | "schema" => Some(Type::String),
            "format" => Some(Type::Named("DataFormat".to_string())),
            _ => None,
        };
    }
    if type_name == "DataLoaderStatus" {
        return match field {
            "identity" | "error" | "cleanup" => Some(Type::String),
            "freshness" => Some(Type::Named("DataFreshness".to_string())),
            "invalidated_by" => Some(Type::Named("DataInvalidationCause".to_string())),
            "buffered_bytes" => Some(Type::Int),
            "backpressure" | "last_good" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "JetDataPlotField" {
        return match field {
            "id" | "name" | "type_name" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "JetDataPlotSchema" {
        return match field {
            "identity" | "row_type" => Some(Type::String),
            "columns" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotField".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "JetDataPlotSourceFacts" {
        return match field {
            "table_plan_identity" | "source_identity" | "schema_identity" | "row_type"
            | "data_identity" | "provenance" => Some(Type::String),
            "rows" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "JetDataPlotEncoding" {
        return match field {
            "channel" => Some(Type::Named("JetDataPlotChannel".to_string())),
            "field" => Some(Type::Named("JetDataPlotField".to_string())),
            "aggregate" => Some(Type::Named("JetDataPlotAggregate".to_string())),
            _ => None,
        };
    }
    if type_name == "JetDataPlotScale" {
        return match field {
            "channel" => Some(Type::Named("JetDataPlotChannel".to_string())),
            "kind" => Some(Type::Named("JetDataPlotScaleKind".to_string())),
            "domain" => Some(Type::Named("JetDataPlotDomain".to_string())),
            "clamp" | "reverse" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "JetDataPlotAxis" {
        return match field {
            "channel" => Some(Type::Named("JetDataPlotChannel".to_string())),
            "title" => Some(Type::String),
            "visible" | "grid" => Some(Type::Bool),
            "ticks" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "JetDataPlotLegend" {
        return match field {
            "channel" => Some(Type::Named("JetDataPlotChannel".to_string())),
            "title" => Some(Type::String),
            "position" => Some(Type::Named("JetDataPlotLegendPosition".to_string())),
            "visible" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "JetDataPlotFacet" {
        return match field {
            "field" => Some(Type::Named("JetDataPlotField".to_string())),
            "kind" => Some(Type::Named("JetDataPlotFacetKind".to_string())),
            "title" => Some(Type::String),
            "columns" | "rows" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "JetDataPlotLayer" {
        return match field {
            "name" => Some(Type::String),
            "mark" => Some(Type::Named("JetDataPlotMark".to_string())),
            "encodings" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotEncoding".to_string(),
            )))),
            "transforms" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotTransform".to_string(),
            )))),
            "opacity" => Some(Type::Float),
            _ => None,
        };
    }
    if type_name == "JetDataPlotAccessibility" {
        return match field {
            "title" | "description" | "summary" => Some(Type::String),
            "keyboard" | "announce_selection" => Some(Type::Bool),
            _ => None,
        };
    }
    if type_name == "JetDataPlotLayout" {
        return match field {
            "width" | "height" | "margin_top" | "margin_right" | "margin_bottom"
            | "margin_left" => Some(Type::Float),
            _ => None,
        };
    }
    if type_name == "JetDataPlotCapability" {
        return match field {
            "backend" => Some(Type::Named("JetDataPlotBackend".to_string())),
            "feature" | "reason" | "replacement" => Some(Type::String),
            "support" => Some(Type::Named("JetDataPlotSupport".to_string())),
            _ => None,
        };
    }
    if type_name == "JetDataPlotError" {
        return match field {
            "kind" => Some(Type::Named("JetDataPlotErrorKind".to_string())),
            "operation" | "reason" => Some(Type::String),
            "field" | "channel" | "mark" | "expected" | "actual" => {
                Some(Type::Option(Box::new(Type::String)))
            }
            "index" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if type_name == "JetDataPlotPlan" {
        return match field {
            "source" => Some(Type::Named("JetDataPlotSourceFacts".to_string())),
            "schema" => Some(Type::Named("JetDataPlotSchema".to_string())),
            "mark" => Some(Type::Named("JetDataPlotMark".to_string())),
            "encodings" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotEncoding".to_string(),
            )))),
            "transforms" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotTransform".to_string(),
            )))),
            "scales" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotScale".to_string(),
            )))),
            "axes" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotAxis".to_string(),
            )))),
            "legends" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotLegend".to_string(),
            )))),
            "facets" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotFacet".to_string(),
            )))),
            "layers" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotLayer".to_string(),
            )))),
            "interactions" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotInteraction".to_string(),
            )))),
            "accessibility" => Some(Type::Named("JetDataPlotAccessibility".to_string())),
            "layout" => Some(Type::Named("JetDataPlotLayout".to_string())),
            _ => None,
        };
    }
    if type_name == "JetDataPlotSelectedRow" {
        return match field {
            "index" => Some(Type::Int),
            "values" => Some(Type::List(Box::new(Type::Tuple(vec![
                (
                    "field".to_string(),
                    Box::new(Type::Named("JetDataPlotField".to_string())),
                ),
                (
                    "value".to_string(),
                    Box::new(Type::Named("JetDataPlotValue".to_string())),
                ),
            ])))),
            _ => None,
        };
    }
    if type_name == "JetDataPlotInspection" {
        return match field {
            "plan" => Some(Type::Named("JetDataPlotPlan".to_string())),
            "selected_indices" => Some(Type::List(Box::new(Type::Int))),
            "selected_columns" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotField".to_string(),
            )))),
            "selected_data" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotSelectedRow".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "JetDataPlotRender" {
        return match field {
            "backend" => Some(Type::Named("JetDataPlotBackend".to_string())),
            "format" => Some(Type::Named("JetDataPlotRenderFormat".to_string())),
            "body" => Some(Type::String),
            "source" => Some(Type::Named("JetDataPlotSourceFacts".to_string())),
            "capabilities" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotCapability".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "JetDataPlotProjection" {
        return match field {
            "backend" => Some(Type::Named("JetDataPlotBackend".to_string())),
            "plan" => Some(Type::Named("JetDataPlotPlan".to_string())),
            "source" => Some(Type::Named("JetDataPlotSourceFacts".to_string())),
            "capabilities" => Some(Type::List(Box::new(Type::Named(
                "JetDataPlotCapability".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "DataSummary" {
        return match field {
            "count" => Some(Type::Int),
            "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev" => Some(Type::Float),
            _ => None,
        };
    }
    match (type_name, field) {
        // D-DX-QUEUE1=A: queue carriers mirror the provider's typed public
        // records. State and delivery remain nameable enum values instead of
        // being flattened to strings.
        ("JobPayload" | "JobResult", "type_id") => Some(Type::String),
        ("JobPayload" | "JobResult", "bytes") => Some(Type::List(Box::new(u8_ty()))),
        ("JobPayload" | "JobResult", "publish") => Some(Type::Bool),
        ("JobError", "type_id" | "reason") => Some(Type::String),
        ("JobError", "detail") => Some(Type::Option(Box::new(Type::String))),
        ("JobQueueReceipt", "id" | "authority" | "queue" | "job_type" | "signature") => {
            Some(Type::String)
        }
        ("JobQueueReceipt", "state") => Some(Type::Named("JobQueueState".to_string())),
        ("JobQueueReceipt", "delivery") => {
            Some(Type::Named("JobQueueDeliveryPolicy".to_string()))
        }
        ("JobQueueReceipt", "sequence" | "attempts" | "due_at_ms" | "accepted_at_ms") => {
            Some(Type::Int)
        }
        ("JobQueueReceipt", "request_id" | "idempotency_key" | "error_reason") => {
            Some(Type::Option(Box::new(Type::String)))
        }
        ("JobQueueReceipt", "lease_until_ms" | "duration_ms") => {
            Some(Type::Option(Box::new(Type::Int)))
        }
        ("JobQueueReceipt", "duplicate") => Some(Type::Bool),
        ("JobQueueClaim", "receipt") => Some(Type::Named("JobQueueReceipt".to_string())),
        ("JobQueueClaim", "payload") => Some(Type::Named("JobPayload".to_string())),
        ("JobQueueClaim", "worker" | "lease_token") => Some(Type::String),
        ("JobQueueClaim", "lease_until_ms") => Some(Type::Int),
        ("JobQueueEvent", "sequence" | "attempts" | "timestamp_ms") => Some(Type::Int),
        ("JobQueueEvent", "state") => Some(Type::Named("JobQueueState".to_string())),
        ("JobQueueEvent", "reason" | "worker") => Some(Type::Option(Box::new(Type::String))),
        ("JobQueueEvent", "duration_ms") => Some(Type::Option(Box::new(Type::Int))),
        ("JobQueueRecord", "receipt") => Some(Type::Named("JobQueueReceipt".to_string())),
        ("JobQueueRecord", "payload") => Some(Type::Option(Box::new(Type::Named(
            "JobPayload".to_string(),
        )))),
        ("JobQueueRecord", "result") => Some(Type::Option(Box::new(Type::Named(
            "JobResult".to_string(),
        )))),
        ("JobQueueRecord", "error") => Some(Type::Option(Box::new(Type::Named(
            "JobError".to_string(),
        )))),
        ("JobQueueRecord", "started_at_ms" | "finished_at_ms") => {
            Some(Type::Option(Box::new(Type::Int)))
        }
        ("JobQueueStatus", "queue" | "authority") => Some(Type::String),
        (
            "JobQueueStatus",
            "queued"
            | "running"
            | "retrying"
            | "completed"
            | "failed"
            | "dead_lettered"
            | "cancelled"
            | "depth"
            | "wait_ms"
            | "throughput"
            | "capacity"
            | "freshness_ms",
        ) => Some(Type::Int),
        ("JobQueueStatus", "paused") => Some(Type::Bool),
        // D-LSDIR1=A: DirEntry has name (bare filename), path (full path), is_dir.
        ("DirEntry", "name" | "path") => Some(Type::String),
        ("DirEntry", "is_dir") => Some(Type::Bool),
        // D-FSOPS1=A: typed filesystem metadata and recursive walk entries.
        ("Stat", "size" | "modified_ms" | "created_ms" | "mode") => Some(Type::Int),
        ("Stat", "readonly" | "is_file" | "is_dir" | "is_symlink") => Some(Type::Bool),
        ("Stat", "kind") => Some(Type::String),
        ("WalkEntry", "path" | "relative") => Some(Type::String),
        ("WalkEntry", "is_dir") => Some(Type::Bool),
        ("WalkEntry", "depth") => Some(Type::Int),
        // D-RENDERTGT2=A (c133 M1): UI geometry fields.
        ("Point", "x" | "y") => Some(Type::Float),
        ("Size", "width" | "height") => Some(Type::Float),
        ("Rect", "x" | "y" | "width" | "height") => Some(Type::Float),
        ("SizeConstraint", "min_width" | "min_height" | "max_width" | "max_height") => {
            Some(Type::Float)
        }
        // D-SPACE-GEOMETRY1=A: stock aliases expose the same scalar fields;
        // their space is carried by the nominal type.
        (
            "ScreenPoint" | "WorldPoint" | "ViewPoint" | "CameraPoint" | "DevicePoint"
                | "ScreenDelta" | "WorldDelta" | "ViewDelta" | "CameraDelta" | "DeviceDelta",
            "x" | "y",
        ) => Some(Type::Float),
        // D-FOUND-PLATFORM1=A: shared font and host value fields.
        ("FontFace", "family") => Some(Type::String),
        ("FontFace", "size") => Some(Type::Float),
        ("FontFace", "style") => Some(Type::Named("FontStyle".to_string())),
        ("Glyph", "id" | "cluster") => Some(Type::Int),
        ("Glyph", "x" | "y" | "advance_x" | "advance_y") => Some(Type::Float),
        ("GlyphRun", "glyphs") => Some(Type::List(Box::new(Type::Named("Glyph".to_string())))),
        ("GlyphRun", "advance_x" | "advance_y") => Some(Type::Float),
        ("GlyphRun", "shaper") => Some(Type::Named("GlyphShaper".to_string())),
        ("GlyphRun", "deterministic" | "approximate") => Some(Type::Bool),
        ("UiNode", "label") => Some(Type::String),
        ("UiNode", "width" | "height") => Some(Type::Float),
        ("UiNode", "accessibility") => {
            Some(Type::Option(Box::new(Type::Named("UiAccessibility".to_string()))))
        }
        ("UiNode", "ime") => {
            Some(Type::Option(Box::new(Type::Named("UiImeMode".to_string()))))
        }
        ("UiNode", "shortcut") => Some(Type::Option(Box::new(Type::Named("UiShortcut".to_string())))),
        ("UiFileFilter", "label") => Some(Type::String),
        ("UiFileFilter", "extensions" | "mime_types") => {
            Some(Type::List(Box::new(Type::String)))
        }
        ("UiFsGrant", "root") => Some(Type::String),
        ("UiFsGrant", "rights") => Some(Type::Named("UiFsRights".to_string())),
        ("UiGrantedPath", "path" | "grant_root") => Some(Type::String),
        ("UiGrantedPath", "access") => Some(Type::Named("UiFsAccess".to_string())),
        ("UiFileDialogRequest", "title") => Some(Type::String),
        ("UiFileDialogRequest", "kind") => Some(Type::Named("UiFileDialogKind".to_string())),
        ("UiFileDialogRequest", "grant") => Some(Type::Named("UiFsGrant".to_string())),
        ("UiFileDialogRequest", "initial_directory") => {
            Some(Type::Option(Box::new(Type::Named("UiGrantedPath".to_string()))))
        }
        ("UiFileDialogRequest", "filters") => {
            Some(Type::List(Box::new(Type::Named("UiFileFilter".to_string()))))
        }
        ("UiFileDialogRequest", "allow_multiple") => Some(Type::Bool),
        ("UiFileDialogSelection", "files") => {
            Some(Type::List(Box::new(Type::Named("UiGrantedPath".to_string()))))
        }
        ("UiClipboardText", "text") => Some(Type::String),
        ("UiClipboardText", "selection") => {
            Some(Type::Option(Box::new(Type::Named("UiTextRange".to_string()))))
        }
        ("UiClipboardWrite", "characters") => Some(Type::Int),
        ("UiTextRange", "start" | "end") => Some(Type::Int),
        ("UiImeComposition", "text") => Some(Type::String),
        ("UiImeComposition", "selection") => Some(Type::Named("UiTextRange".to_string())),
        ("UiImeComposition", "marked") => {
            Some(Type::Option(Box::new(Type::Named("UiTextRange".to_string()))))
        }
        ("UiImeEvent", "target") => Some(Type::Named("UiNodeId".to_string())),
        ("UiImeEvent", "phase") => Some(Type::Named("UiImePhase".to_string())),
        ("UiImeEvent", "composition") => {
            Some(Type::Option(Box::new(Type::Named("UiImeComposition".to_string()))))
        }
        ("UiDragEvent", "target") => Some(Type::Named("UiNodeId".to_string())),
        ("UiDragEvent", "phase") => Some(Type::Named("UiDragPhase".to_string())),
        ("UiDragEvent", "operation") => Some(Type::Named("UiDragOperation".to_string())),
        ("UiDragEvent", "items") => Some(Type::List(Box::new(Type::Named("UiDropItem".to_string())))),
        ("UiShortcut", "key") => Some(Type::String),
        ("UiShortcut", "modifiers") => Some(Type::Named("UiShortcutModifiers".to_string())),
        ("UiShortcutBinding", "shortcut") => Some(Type::Named("UiShortcut".to_string())),
        ("UiShortcutBinding", "action") => Some(Type::String),
        ("UiShortcutBinding", "node") => {
            Some(Type::Option(Box::new(Type::Named("UiNodeId".to_string()))))
        }
        ("UiAccessibility", "name" | "description") => Some(Type::Option(Box::new(Type::String))),
        // D-FOUND-REALTIME1=A: bounded callback accounting is a readable receipt.
        (
            "RealtimeReceipt",
            "requested_rate_hz"
                | "requested_frames"
                | "completed_callbacks"
                | "completed_frames"
                | "missed"
                | "max_lateness_ns"
                | "start_identity"
                | "end_identity",
        ) => Some(Type::Int),
        ("ProcessResult" | "ProcessReceipt", "code") => Some(Type::Int),
        ("ProcessResult" | "ProcessReceipt", "success" | "timed_out" | "redacted") => {
            Some(Type::Bool)
        }
        ("ProcessResult" | "ProcessReceipt", "signal") => Some(Type::Option(Box::new(Type::Int))),
        ("ProcessResult" | "ProcessReceipt", "limit_hit") => Some(Type::Option(Box::new(
            Type::Named("ProcessResourceLimit".to_string()),
        ))),
        (
            "ProcessResult" | "ProcessReceipt",
            "output"
            | "errors"
            | "executable_identity"
            | "input_digest"
            | "policy_digest"
            | "backend"
            | "descendants",
        ) => Some(Type::String),
        ("ProcessResult" | "ProcessReceipt", "argv" | "authority" | "limits" | "outputs") => {
            Some(Type::List(Box::new(Type::String)))
        }
        ("ProcessResult" | "ProcessReceipt", "pid") => Some(Type::Int),
        (
            "ProcessPlan",
            "executable_identity" | "input_digest" | "policy_digest" | "backend" | "descendants",
        ) => Some(Type::String),
        ("ProcessPlan", "argv" | "authority" | "limits" | "outputs") => {
            Some(Type::List(Box::new(Type::String)))
        }
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
        // D-HTTPLIB1=A: TLS constructor fields are public PEM values.
        ("HTTPServerTls", "cert_pem" | "key_pem") => Some(Type::String),
        // D-LOGTRACE1=A: typed logging values are Prelude structs, so their
        // published fields are readable like every other core record.
        ("LogField", "key" | "value" | "kind") => Some(Type::String),
        ("LogField", "redacted") => Some(Type::Bool),
        ("LogSpan", "id") => Some(Type::Int),
        ("LogSpan", "name") => Some(Type::String),
        // D-GAME-*: scene-owned headless game substrate fields.
        ("GameScene", "assets") => Some(Type::Named("GameAssets".to_string())),
        ("GameScene", "input") => Some(Type::Named("GameInputMap".to_string())),
        ("GameFrame", "index") => Some(Type::Int),
        ("GameFrame", "input") => Some(Type::Named("GameInputSnapshot".to_string())),
        ("WebRouterField", "name") => Some(Type::String),
        ("WebRouterField", "value_type") => Some(Type::Named("WebRouterValueType".to_string())),
        ("WebRouterField", "required") => Some(Type::Bool),
        ("WebRouterField", "default") => Some(Type::Option(Box::new(Type::String))),
        ("WebRouterCacheState", "identity" | "data") => Some(Type::String),
        ("WebRouterCacheState", "status") => Some(Type::Named("WebRouterCacheStatus".to_string())),
        ("WebRouterCacheState", "dependencies") => Some(Type::List(Box::new(Type::String))),
        ("WebRouterCacheState", "generation") => Some(Type::Int),
        ("WebNavigation", "url" | "route") => Some(Type::String),
        ("WebNavigation", "params" | "search") => Some(Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(Type::String),
        }),
        ("WebNavigationState", "status") => {
            Some(Type::Named("WebNavigationStatus".to_string()))
        }
        ("WebNavigationState", "current") => {
            Some(Type::Option(Box::new(Type::Named("WebNavigation".to_string()))))
        }
        ("WebNavigationState", "data" | "error") => Some(Type::String),
        ("WebNavigationState", "pending_boundary_id" | "error_boundary_id" | "island_identity" | "hydration_trigger") => {
            Some(Type::Option(Box::new(Type::String)))
        }
        ("WebQueryState", "status") => Some(Type::Named("WebQueryStatus".to_string())),
        ("WebQueryState", "value" | "error") => Some(Type::String),
        ("WebQueryState", "generation" | "queued") => Some(Type::Int),
        ("WebQueryState", "offline") => Some(Type::Bool),
        ("WebMutationState", "status") => Some(Type::Named("WebMutationStatus".to_string())),
        ("WebMutationState", "generation") => Some(Type::Int),
        ("WebMutationState", "optimistic" | "rollback" | "paused") => Some(Type::Bool),
        ("WebMutationState", "value" | "result" | "error" | "context") => Some(Type::String),
        ("WebMutationState", "queued" | "replayed") => Some(Type::Int),
        ("WebFormFieldState", "name" | "value") => Some(Type::String),
        ("WebFormFieldState", "value_type") => Some(Type::Named("WebFormValueType".to_string())),
        ("WebFormFieldState", "required" | "touched" | "validating") => Some(Type::Bool),
        ("WebFormFieldState", "errors") => Some(Type::List(Box::new(Type::String))),
        ("WebFormFieldSpec", "name" | "label" | "wire_name") => Some(Type::String),
        ("WebFormFieldSpec", "value_type") => Some(Type::Named("WebFormValueType".to_string())),
        ("WebFormFieldSpec", "required") => Some(Type::Bool),
        ("WebFormFieldSpec", "default" | "group") => Some(Type::Option(Box::new(Type::String))),
        ("WebFormFieldSpec", "control") => Some(Type::Named("WebFormControl".to_string())),
        ("WebFormDecodedInput", "type_name") => Some(Type::String),
        ("WebFormDecodedInput", "values" | "wire_values") => Some(Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(Type::String),
        }),
        ("WebFormActionError", "field_errors") => Some(Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(Type::List(Box::new(Type::String))),
        }),
        ("WebFormActionError", "form_errors") => Some(Type::List(Box::new(Type::String))),
        ("WebFormErrorState", "fields") => Some(Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(Type::List(Box::new(Type::String))),
        }),
        ("WebFormErrorState", "form") => Some(Type::List(Box::new(Type::String))),
        ("WebFormLifecycle", "status") => Some(Type::Named("WebFormLifecycleStatus".to_string())),
        ("WebFormLifecycle", "result" | "error") => Some(Type::String),
        ("WebFormLifecycle", "generation") => Some(Type::Int),
        ("WebTableSort", "column") | ("WebTableFilter", "column") => Some(Type::String),
        ("WebTableSort", "direction") => Some(Type::Named("WebTableSortDirection".to_string())),
        ("WebTableFilter", "value") => Some(Type::String),
        ("WebTableState", "sort") => Some(Type::Option(Box::new(Type::Named(
            "WebTableSort".to_string(),
        )))),
        ("WebTableState", "filter") => Some(Type::Option(Box::new(Type::Named(
            "WebTableFilter".to_string(),
        )))),
        ("WebTableState", "page_index" | "page_size") => Some(Type::Int),
        ("WebTableState", "selected_keys") => Some(Type::List(Box::new(Type::String))),
        ("WebTableState", "selection_anchor" | "focus_key") => Some(Type::Option(Box::new(Type::String))),
        ("WebTableState", "page_mode") => Some(Type::Named("WebTablePageMode".to_string())),
        ("WebVirtualWindow",
            "total_count" | "scroll_offset" | "viewport_size" | "estimated_item_size"
            | "overscan" | "start" | "end" | "total_size" | "measured_count") => Some(Type::Int),
        ("WebVirtualPlan",
            "total_count" | "scroll_offset" | "viewport_width" | "viewport_height"
            | "estimated_item_size" | "overscan" | "start" | "end" | "total_size"
            | "measured_count" | "anchor_index" | "anchor_offset") => Some(Type::Int),
        ("WebVirtualPlan", "measurements") => Some(Type::List(Box::new(Type::Tuple(vec![
            ("index".to_string(), Box::new(Type::Int)),
            ("size".to_string(), Box::new(Type::Int)),
        ])))),
        ("WebVirtualPlanViewport",
            "start" | "end" | "total_size" | "measured_count" | "anchor_index"
            | "anchor_offset") => Some(Type::Int),
        ("WebStore", "name") => Some(Type::String),
        ("WebStoreEvent", "sequence" | "generation" | "cursor") => Some(Type::Int),
        ("WebStoreEvent", "action") => Some(Type::String),
        ("WebStoreEvent", "changed_fields") => Some(Type::List(Box::new(Type::String))),
        ("WebStoreEvent", "kind") => Some(Type::String),
        ("WebStoreInspection", "generation" | "cursor") => Some(Type::Int),
        ("WebStoreInspection", "history_enabled") => Some(Type::Bool),
        ("WebStoreInspection", "history_limit") => Some(Type::Int),
        ("WebStoreInspection", "events") => Some(Type::List(Box::new(
            Type::Named("WebStoreEvent".to_string()),
        ))),
        ("WebStoreInspection", "history") => Some(Type::List(Box::new(
            Type::Named("WebStoreTransaction".to_string()),
        ))),
        // Generic record fields are handled by `core_generic_struct_field`.
        // Every remaining CORE struct users can construct answers from the one
        // constructable-field table instead of a second hand-kept allowlist:
        // a shape spelled once for `Type.{ … }` cannot then disagree with the
        // same shape read back through `.field`. Card 2021 — the allowlist form
        // of this fallback had silently omitted `XMLRenderOptions`,
        // `XMLCanonical` and `TextWidth`, so their fields constructed but did
        // not read.
        _ => core_constructable_fields(type_name)?
            .into_iter()
            .find(|(name, _)| name == field)
            .map(|(_, ty)| ty),
    }
}

fn compiler_package_struct_field(type_name: &str, field: &str) -> Option<Type> {
    let string_list = || Type::List(Box::new(Type::String));
    let optional_string = || Type::Option(Box::new(Type::String));
    match type_name {
        "CompilerPackageError" => match field {
            "code" | "message" | "file" | "cause" => Some(Type::String),
            _ => None,
        },
        "CompilerDependency" => match field {
            "name" | "source" => Some(Type::String),
            _ => None,
        },
        "CompilerPackageTarget" => match field {
            "name" => Some(Type::String),
            "targets" => Some(string_list()),
            _ => None,
        },
        "CompilerPackageOutput" => match field {
            "name" | "kind" => Some(Type::String),
            "entry" => Some(optional_string()),
            _ => None,
        },
        "CompilerBuildProfile" => match field {
            "name" | "optimize" => Some(Type::String),
            "debug_info" | "small" => Some(Type::Bool),
            "panic" => Some(optional_string()),
            _ => None,
        },
        "CompilerManifest" | "CompilerPackage" => match field {
            "schema_version" => Some(Type::Int),
            "file" => Some(Type::String),
            "jet" | "edition" | "description" | "license" | "repository" | "layer" | "target" => {
                Some(optional_string())
            }
            "dependencies" => Some(Type::List(Box::new(Type::Named(
                "CompilerDependency".to_string(),
            )))),
            "packages" => Some(Type::List(Box::new(Type::Named(
                "CompilerPackageTarget".to_string(),
            )))),
            "outputs" => Some(Type::List(Box::new(Type::Named(
                "CompilerPackageOutput".to_string(),
            )))),
            "build_profiles" => Some(Type::List(Box::new(Type::Named(
                "CompilerBuildProfile".to_string(),
            )))),
            _ => None,
        },
        "CompilerLockedPackage" => match field {
            "name" | "version" | "source_kind" | "fingerprint" => Some(Type::String),
            "source" | "revision" | "content_hash" | "layer" | "inferred_layer" => {
                Some(optional_string())
            }
            "dependencies" => Some(string_list()),
            _ => None,
        },
        "CompilerLock" => match field {
            "schema_version" | "version" => Some(Type::Int),
            "file" => Some(Type::String),
            "root_dependencies" => Some(string_list()),
            "packages" => Some(Type::List(Box::new(Type::Named(
                "CompilerLockedPackage".to_string(),
            )))),
            _ => None,
        },
        "CompilerKeyValue" => match field {
            "key" | "value" => Some(Type::String),
            _ => None,
        },
        "CompilerProfile" => match field {
            "name" => Some(Type::String),
            "extends" | "packages" | "sources" => Some(string_list()),
            "collisions" => Some(Type::List(Box::new(Type::Named(
                "CompilerKeyValue".to_string(),
            )))),
            _ => None,
        },
        "CompilerProfileSet" => match field {
            "schema_version" => Some(Type::Int),
            "file" => Some(Type::String),
            "profiles" => Some(Type::List(Box::new(Type::Named(
                "CompilerProfile".to_string(),
            )))),
            _ => None,
        },
        _ => None,
    }
}

/// D-MIGRATE3=A / card 2021: the ONE Core-struct field-type oracle every tier
/// reads. Sema owns it because sema is what typed the field read in the first
/// place (I3); AOT lowering, the Cranelift JIT and the TIR interpreter are
/// marshalling adapters that must not re-declare a Core record's shape (I9).
///
/// `Type::Named` receivers go through [`core_struct_field`]; a reserved core
/// GENERIC (`DataJoin<L, R>`, `VjpRun<T>`, `Rotation<T>`)
/// goes through [`core_generic_struct_field`], which needs the type arguments.
/// Neither answers for a USER struct: every caller resolves its own struct
/// table first (D-SHIFT1 user-type-wins), exactly as `Checker::field_type`
/// does.
pub fn core_struct_field_type(type_name: &str, field: &str, args: &[Type]) -> Option<Type> {
    if args.is_empty() {
        core_struct_field(type_name, field)
    } else {
        core_generic_struct_field(type_name, field, args)
            .or_else(|| core_struct_field(type_name, field))
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

/// Field access on reserved generic core values. Mirrors [`core_struct_field`]
/// for core types that carry generic type arguments (`Type::Apply`, not
/// `Type::Named`); see the `Type::Apply` arm in `CheckerInfer/expr.rs`.
pub(crate) fn core_generic_struct_field(
    type_name: &str,
    field: &str,
    args: &[Type],
) -> Option<Type> {
    if type_name == "TypedHistoryCase" && args.len() == 1 {
        return match field {
            "case" => Some(Type::Named("HistoryCase".to_string())),
            "commands" => Some(Type::List(Box::new(args[0].clone()))),
            _ => None,
        };
    }
    if type_name == "HistoryStrategy" && args.len() == 1 {
        let command = args[0].clone();
        return match field {
            "generate" => Some(Type::Fn {
                params: vec![
                    Type::Named("HistoryRng".to_string()),
                    Type::Named("Count".to_string()),
                    Type::Named("Count".to_string()),
                ],
                ret: Some(Box::new(Type::Option(Box::new(Type::Apply {
                    name: "TypedHistoryCase".to_string(),
                    args: vec![command.clone()],
                })))),
                effect_bound: None,
                param_contract: None,
                call_metadata: None,
                return_view_provenance: None,
            }),
            "rebuild" => Some(Type::Fn {
                params: vec![Type::Named("HistoryCase".to_string())],
                ret: Some(Box::new(Type::Option(Box::new(Type::Apply {
                    name: "TypedHistoryCase".to_string(),
                    args: vec![command.clone()],
                })))),
                effect_bound: None,
                param_contract: None,
                call_metadata: None,
                return_view_provenance: None,
            }),
            "valid" => Some(Type::Fn {
                params: vec![Type::Apply {
                    name: "TypedHistoryCase".to_string(),
                    args: vec![command],
                }],
                ret: Some(Box::new(Type::Bool)),
                effect_bound: None,
                param_contract: None,
                call_metadata: None,
                return_view_provenance: None,
            }),
            "bounds" => Some(Type::Named("HistoryBounds".to_string())),
            "distributions" => Some(Type::List(Box::new(Type::Named(
                "HistoryDistribution".to_string(),
            )))),
            _ => None,
        };
    }
    if type_name == "FfiCallbackEvent" && args.len() == 1 {
        return (field == "value").then_some(args[0].clone());
    }
    // D-SPACE-GEOMETRY1=A: generic coordinate carriers have only numeric
    // payload fields; the second (and third for Transform2) arguments remain
    // compile-time nominal space identities.
    if matches!(type_name, "Point2" | "Delta2") && args.len() == 2 {
        return matches!(field, "x" | "y").then_some(args[0].clone());
    }
    if type_name == "Ray2" && args.len() == 3 {
        let output_space = args[2].clone();
        return match field {
            "origin" => Some(Type::Apply {
                name: "Point2".to_string(),
                args: vec![args[0].clone(), output_space.clone()],
            }),
            "direction" => Some(Type::Apply {
                name: "Delta2".to_string(),
                args: vec![args[0].clone(), output_space],
            }),
            _ => None,
        };
    }
    if type_name == "Transform" && args.len() == 2 {
        return matches!(field, "m00" | "m01" | "m10" | "m11" | "tx" | "ty")
            .then_some(Type::Float);
    }
    if type_name == "Transform2" && args.len() == 3 {
        return matches!(field, "m00" | "m01" | "m10" | "m11" | "tx" | "ty")
            .then_some(args[0].clone());
    }
    if type_name == "WebTableColumn" && args.len() == 1 {
        return match field {
            "name" | "cell_type" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "WebTablePage" && args.len() == 1 {
        return match field {
            "rows" => Some(Type::List(Box::new(args[0].clone()))),
            "row_keys" => Some(Type::List(Box::new(Type::String))),
            "total_rows" | "page_index" | "page_size" | "page_count" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "WebTableRow" && args.len() == 1 {
        return match field {
            "key" => Some(Type::String),
            "value" => Some(args[0].clone()),
            _ => None,
        };
    }
    if type_name == "WebTable" && args.len() == 1 {
        return match field {
            "name" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "WebStoreTransaction" && args.len() == 1 {
        return match field {
            "generation" => Some(Type::Int),
            "action" => Some(Type::String),
            "changed_fields" => Some(Type::List(Box::new(Type::String))),
            "before" | "after" => Some(args[0].clone()),
            _ => None,
        };
    }
    if type_name == "WebVirtualPlan" && args.is_empty() {
        return match field {
            "total_count" | "scroll_offset" | "viewport_width" | "viewport_height"
            | "estimated_item_size" | "overscan" | "start" | "end" | "total_size"
            | "measured_count" | "anchor_index" | "anchor_offset" => Some(Type::Int),
            "measurements" => Some(Type::List(Box::new(Type::Tuple(vec![
                ("index".to_string(), Box::new(Type::Int)),
                ("size".to_string(), Box::new(Type::Int)),
            ])))),
            _ => None,
        };
    }
    if type_name == "WebVirtualPlanViewport" && args.is_empty() {
        return match field {
            "start" | "end" | "total_size" | "measured_count" | "anchor_index"
            | "anchor_offset" => Some(Type::Int),
            _ => None,
        };
    }
    if type_name == "WebStore" && args.len() == 1 {
        return match field {
            "name" => Some(Type::String),
            _ => None,
        };
    }
    if type_name == "WebStoreInspection" && args.len() == 1 {
        return match field {
            "value" => Some(args[0].clone()),
            "generation" | "cursor" | "history_limit" => Some(Type::Int),
            "history" => Some(Type::List(Box::new(Type::Apply {
                name: "WebStoreTransaction".to_string(),
                args: vec![args[0].clone()],
            }))),
            "events" => Some(Type::List(Box::new(Type::Named(
                "WebStoreEvent".to_string(),
            )))),
            "history_enabled" => Some(Type::Bool),
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
    if type_name == "Group" && args.len() == 2 {
        return match field {
            "key" => Some(args[0].clone()),
            "value" => Some(args[1].clone()),
            _ => None,
        };
    }
    if type_name == "DataLoader" && args.len() == 1 {
        return match field {
            "source" => Some(Type::Named("DataSourceIdentity".to_string())),
            "format" => Some(Type::Named("DataFormat".to_string())),
            "authority" => Some(Type::Named("DataAuthority".to_string())),
            "limits" => Some(Type::Named("DataLimits".to_string())),
            "payload" | "last_good" => Some(Type::Option(Box::new(Type::List(Box::new(
                u8_ty(),
            ))))),
            "cancelled" | "offline" => Some(Type::Bool),
            "status" => Some(Type::Named("DataLoaderStatus".to_string())),
            _ => None,
        };
    }
    if type_name == "DataSnapshot" && args.len() == 1 {
        return match field {
            "value" => Some(args[0].clone()),
            "identity" => Some(Type::Named("DataSnapshotIdentity".to_string())),
            "provenance" => Some(Type::Named("DataProvenance".to_string())),
            "schema" => Some(Type::Named("DataSchema".to_string())),
            "status" => Some(Type::Named("DataLoaderStatus".to_string())),
            "content" => Some(Type::List(Box::new(u8_ty()))),
            _ => None,
        };
    }
    if type_name == "JetDataPlotColumn" && args.len() == 1 {
        return match field {
            "field" => Some(Type::Named("JetDataPlotField".to_string())),
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
                call_metadata: None,
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
/// D-DX-QUEUE1=A: method return contracts for the typed durable queue.
/// Arity is part of recognition so an invalid call remains a recognized queue
/// method and can receive the shared wrong-arity diagnostic in method sema.
pub(crate) fn job_queue_method_return(
    ty: &Type,
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    let Type::Named(name) = ty else {
        return None;
    };
    if name != "JobQueue" {
        return None;
    }
    let service_error = || Type::Named("ServiceError".to_string());
    let result = |ok| result_ty(ok, service_error());
    let (valid, ret) = match method {
        "enqueue" => ((2..=3).contains(&n_args), result(Type::Named(
            "JobQueueReceipt".to_string(),
        ))),
        "delay" => (
            n_args == 3,
            result(Type::Named("JobQueueReceipt".to_string())),
        ),
        "receipt" => (
            n_args == 1,
            result(Type::Named("JobQueueReceipt".to_string())),
        ),
        "inspect" => (
            n_args == 2,
            result(Type::List(Box::new(Type::Named(
                "JobQueueRecord".to_string(),
            )))),
        ),
        "events" => (
            n_args == 1,
            result(Type::List(Box::new(Type::Named(
                "JobQueueEvent".to_string(),
            )))),
        ),
        "claim" => (
            n_args == 2,
            result(Type::List(Box::new(Type::Named(
                "JobQueueClaim".to_string(),
            )))),
        ),
        "heartbeat" | "acknowledge" | "fail" | "cancel" | "dead_letter" => {
            let valid = match method {
                "heartbeat" => n_args == 1,
                "acknowledge" | "fail" => n_args == 2,
                "cancel" | "dead_letter" => (2..=3).contains(&n_args),
                _ => false,
            };
            (
                valid,
                result(Type::Named("JobQueueReceipt".to_string())),
            )
        }
        "recover_expired" => (n_args == 0, result(unit_ty())),
        "status" | "pause" | "resume" => (
            n_args == 0,
            result(Type::Named("JobQueueStatus".to_string())),
        ),
        "wait" => (n_args == 1, result(Type::Named("JobQueueStatus".to_string()))),
        "prune" => (n_args == 0, result(Type::Int)),
        _ => return None,
    };
    Some(valid.then_some(ret))
}

/// D-FLAGSHIP-WEBAPI1=A: instance method return types for the headless web
/// suite. Element-bearing records stay generic through their `Type::Apply`
/// arguments; argument validation remains in the method-call checker.
pub fn web_method_return(
    ty: &Type,
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    let (name, args) = match ty {
        Type::Named(name) => (name.as_str(), &[][..]),
        Type::Apply { name, args } => (name.as_str(), args.as_slice()),
        _ => return None,
    };
    let element = |expected: &str| {
        (name == expected && args.len() == 1).then(|| args[0].clone())
    };
    match name {
        "JetDataPlot" => {
            let element = element("JetDataPlot")?;
            let plot = || Type::Apply {
                name: "JetDataPlot".to_string(),
                args: vec![element.clone()],
            };
            let fallible = || result_ty(plot(), Type::Named("DataError".to_string()));
            match method {
                "line" | "bar" | "point" => Some(Some(plot())),
                "x" | "y" | "color" | "size" | "text" | "detail"
                | "with_transform" | "with_scale" | "with_axis" | "with_legend"
                | "facet" | "with_layer" | "with_interaction" | "accessibility"
                | "layout" | "select_indices" => Some(Some(fallible())),
                _ => None,
            }
        }
        "WebTable" => {
            let element = element("WebTable")?;
            match method {
                "with_column" | "paginate" => Some(Some(Type::Apply {
                    name: "WebTable".to_string(),
                    args: vec![element],
                })),
                "page" => Some(Some(result_ty(
                    Type::Apply {
                        name: "WebTablePage".to_string(),
                        args: vec![element],
                    },
                    Type::String,
                ))),
                _ => None,
            }
        }
        "WebStore" => {
            let element = element("WebStore")?;
            match method {
                "set" => Some(Some(Type::Apply {
                    name: "WebStoreTransaction".to_string(),
                    args: vec![element.clone()],
                })),
                "signal" | "state_signal" => Some(Some(Type::Apply {
                    name: "Signal".to_string(),
                    args: vec![element],
                })),
                "facts_json" => Some(Some(Type::String)),
                _ => None,
            }
        }
        // D-DX-QUERY1 / D-DX-FORMS1: receiver spellings project onto the same
        // Core rows as the module calls; no second dispatch table.
        "WebQuery" if args.is_empty() => match method {
            "state" => Some(Some(Type::Named("WebQueryState".to_string()))),
            "state_signal" => Some(Some(Type::Apply {
                name: "Signal".to_string(),
                args: vec![Type::Named("WebQueryState".to_string())],
            })),
            "mutation_state" => Some(Some(Type::Named("WebMutationState".to_string()))),
            "mutation_signal" => Some(Some(Type::Apply {
                name: "Signal".to_string(),
                args: vec![Type::Named("WebMutationState".to_string())],
            })),
            "invalidate" => Some(Some(Type::Int)),
            "get" | "show" | "facts" => Some(Some(Type::String)),
            "cancel" => Some(Some(Type::Bool)),
            "refresh" => Some(Some(result_ty(unit_ty(), Type::String))),
            _ => None,
        },
        "WebFormTyped" if args.is_empty() => match method {
            "validate" if n_args == 0 => Some(Some(result_ty(unit_ty(), Type::String))),
            "validate" => Some(Some(Type::Named("WebFormValidationChain".to_string()))),
            "set_async_validator" | "blur" | "set" | "focus" => {
                Some(Some(result_ty(unit_ty(), Type::String)))
            }
            "set_action" | "cancel" => Some(None),
            "submit" | "no_script" | "post" => Some(Some(result_ty(Type::String, Type::String))),
            "state" => Some(Some(Type::Named("WebFormState".to_string()))),
            "lifecycle" => Some(Some(Type::Named("WebFormLifecycle".to_string()))),
            "errors" => Some(Some(Type::Named("WebFormErrorState".to_string()))),
            "render" | "show" => Some(Some(Type::String)),
            _ => None,
        },
        "WebFormValidationChain" if args.is_empty() => match method {
            "render" => Some(Some(Type::String)),
            _ => None,
        },
        "WebStorePatch" => {
            let element = element("WebStorePatch")?;
            match method {
                "generation" => Some(Some(Type::Int)),
                "transaction" | "commit" => Some(Some(Type::Apply {
                    name: "WebStoreTransaction".to_string(),
                    args: vec![element],
                })),
                "active" => Some(Some(Type::Bool)),
                "rollback" => Some(Some(Type::Option(Box::new(element)))),
                _ => None,
            }
        }
        "WebStoreSubscription" if args.is_empty() => match method {
            "unsubscribe" => Some(None),
            "active" => Some(Some(Type::Bool)),
            _ => None,
        },
        "WebVirtualWindow" if method == "facts_json" => Some(Some(Type::String)),
        _ => None,
    }
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
        // `Number` is the compiler-only lexical carrier used by typed JSON
        // parsing. It is accepted in generated union decoder patterns, but
        // has no public `DataTree.Number(...)` constructor.
        "Number" => Some(vec![Type::String]),
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

/// D-FOUND-LIFECYCLE1=A: process shutdown signals are the closed `.Term`,
/// `.Hup`, and `.Int` Core enum values.
pub(crate) fn core_process_signal_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    let zero = Span::new(0, 0);
    ["Term", "Hup", "Int"]
        .into_iter()
        .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
        .collect()
}

/// D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum (`.Stream`,
/// `.Inherit`, `.Capture` — exactly the three ratified stream modes), not in
/// the user registry. Mirrors `core_key_variants`.
pub(crate) fn core_process_stream_mode_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let mut m = std::collections::HashMap::new();
    for name in &["Stream", "Inherit", "Capture"] {
        m.insert((*name).to_string(), (zero, VariantPayload::Unit));
    }
    m
}

/// D-PROCESS-RESOURCE1=A: process resource exhaustion is a closed core enum,
/// resolved through the same dot-literal path as `ProcessStreamMode`.
pub(crate) fn core_process_resource_limit_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    Syntax::PROCESS_RESOURCE_LIMIT_VARIANTS
        .iter()
        .map(|name| ((*name).to_string(), (zero, VariantPayload::Unit)))
        .collect()
}

/// D-PROCESS-SESSION2=D: expert terminal mode has the two owner-ratified
/// variants. The portable default is Cooked; Raw is explicit.
pub(crate) fn core_terminal_mode_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    ["Raw", "Cooked"]
        .into_iter()
        .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
        .collect()
}

pub(crate) fn core_env_error_variants(
) -> std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)> {
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    ["InvalidName", "InvalidValue", "NonUnicode"]
        .into_iter()
        .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
        .collect()
}

pub(crate) fn core_net_control_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
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
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    if enum_name == "NetDnsError" {
        for name in ["NotFound", "Failure"] {
            variants.insert(
                name.to_string(),
                (zero, VariantPayload::Single(Type::String, zero)),
            );
        }
        return Some(variants);
    }
    if enum_name != "NetError" {
        return None;
    }
    for name in [
        "InvalidInput",
        "PermissionDenied",
        "AddressInUse",
        "AddressUnavailable",
        "ConnectionRefused",
        "ConnectionReset",
        "NotConnected",
        "Closed",
        "Timeout",
        "Cancelled",
        "Unsupported",
        "TLS",
        "Protocol",
        "Other",
    ] {
        variants.insert(
            name.to_string(),
            (
                zero,
                VariantPayload::Single(Type::Named("NetErrorDetail".to_string()), zero),
            ),
        );
    }
    variants.insert(
        "DNS".to_string(),
        (
            zero,
            VariantPayload::Single(Type::Named("NetDnsError".to_string()), zero),
        ),
    );
    Some(variants)
}

/// D-HTTP-CORE2=A / D-HTTP-UNSUPPORTED1=A: the one closed HTTP error tree.
pub(crate) fn core_http_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::{VariantField, VariantPayload};
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
        "InvalidMethod",
        "InvalidUrl",
        "InvalidHeader",
        "InvalidStatus",
        "BodyConsumed",
        "InvalidFraming",
        "UnsupportedEncoding",
        "Cancelled",
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
        (
            "UnsupportedTarget",
            "operation",
            Type::Named("HTTPOperation".to_string()),
        ),
    ] {
        variants.insert(
            name.to_string(),
            (
                zero,
                VariantPayload::Named(vec![VariantField {
                    name: field.to_string(),
                    name_span: zero,
                    ty,
                    ty_span: zero,
                }]),
            ),
        );
    }
    Some(variants)
}

pub(crate) fn core_io_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    if enum_name == Syntax::TYPE_IO_OPERATION {
        for name in Syntax::IO_OPERATION_VARIANTS {
            variants.insert((*name).to_string(), (zero, VariantPayload::Unit));
        }
        return Some(variants);
    }
    if !is_io_error_type_name(enum_name) {
        return None;
    }
    for name in Syntax::IO_ERROR_VARIANTS {
        let payload = if *name == "ResourceLimit" {
            VariantPayload::Single(
                Type::Named(Syntax::TYPE_PROCESS_RESOURCE_LIMIT.to_string()),
                zero,
            )
        } else {
            VariantPayload::Single(Type::Named(Syntax::TYPE_IO_CONTEXT.to_string()), zero)
        };
        variants.insert((*name).to_string(), (zero, payload));
    }
    Some(variants)
}

/// D-TEXTWIDTH1=B: the two `TextWidth` field enums (`.Narrow`/`.Wide`,
/// `.Zero`/`.Reject`) — synthesised the same way as `ProcessStreamMode`.
pub(crate) fn core_text_width_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
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

/// stdlib-api-laws D4 (#2055): `WatchEvent`'s two closed field enums
/// (`.File`/`.Process`/`.Port`, `.Created`/`.Modified`/`.Removed`/`.Error`/
/// `.Exited`/`.Ready`) — synthesised the same way as `TextWidth`'s pair, so a
/// switch over `ev.domain`/`ev.kind` is exhaustive without a wildcard.
pub(crate) fn core_watch_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let names: &[&str] = match enum_name {
        "WatchDomain" => &["File", "Process", "Port"],
        "WatchKind" => &["Created", "Modified", "Removed", "Error", "Exited", "Ready"],
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
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
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
/// D-FOUND-COREAPI1 / #2853: the closed late-event disposition policy used by
/// `Stream.window`.
pub(crate) fn core_late_event_disposition_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::AST::VariantPayload;
    use crate::Diagnostics::Span;
    if enum_name != "LateEventDisposition" {
        return None;
    }
    let zero = Span::new(0, 0);
    Some(
        ["Drop", "SideOutput"]
            .into_iter()
            .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
            .collect(),
    )
}

pub(crate) fn core_event_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let unit = |names: &[&str]| {
        names
            .iter()
            .map(|name| ((*name).to_string(), (zero, VariantPayload::Unit)))
            .collect()
    };
    match enum_name {
        "Overflow" => Some(unit(&["Block", "DropNewest", "DropOldest"])),
        "FailurePolicy" => Some(unit(&["StopFirst", "Collect", "Log", "Ignore"])),
        "DispatchState" => Some(unit(&[
            "Delivered",
            "HandlerFailed",
            "DroppedNewest",
            "DroppedOldest",
            "Closed",
            "Cancelled",
            "DeadlineExceeded",
        ])),
        "HookPolicy" => Some(unit(&["FirstCancelElseTransform"])),
        "HookDecision" => Some(
            [
                ("Continue".to_string(), (zero, VariantPayload::Unit)),
                (
                    "Transform".to_string(),
                    (
                        zero,
                        VariantPayload::Single(Type::Named("Unknown".to_string()), zero),
                    ),
                ),
                ("Cancel".to_string(), (zero, VariantPayload::Unit)),
                (
                    "Fail".to_string(),
                    (
                        zero,
                        VariantPayload::Single(Type::Named("Unknown".to_string()), zero),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        "HookOutcome" => Some(
            [
                (
                    "Continue".to_string(),
                    (
                        zero,
                        VariantPayload::Single(Type::Named("Unknown".to_string()), zero),
                    ),
                ),
                ("Cancel".to_string(), (zero, VariantPayload::Unit)),
                (
                    "Fail".to_string(),
                    (
                        zero,
                        VariantPayload::Single(Type::Named("Unknown".to_string()), zero),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        _ => None,
    }
}

/// D-REDUCE-VALUE1=A: the closed Core enum passed to SIMD `reduce`.
pub(crate) fn core_reduce_op_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
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

/// D-SERVICE-RECEIPT2=A: lifecycle is the only public delivery sum. Attempts,
/// retention, deadlines, idempotency, and authority generation are facts on
/// the handle's signed receipt, not additional variants.
pub(crate) fn core_delivery_state_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    if enum_name != "DeliveryState" {
        return None;
    }
    let zero = Span::new(0, 0);
    Some(
        [
            "Pending",
            "Accepted",
            "Delivering",
            "Delivered",
            "DeadLettered",
            "Cancelled",
        ]
        .into_iter()
        .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
        .collect(),
    )
}

/// D-SERVICE-WORKFLOW1=D / D-CONC-OUTCOME1: recorded workflow attempts use
/// the same closed result and wait-state names on every execution tier.
pub(crate) fn core_workflow_outcome_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let variants = match enum_name {
        "TaskOutcome" => vec![
            ("Finished", VariantPayload::Unit),
            ("Panicked", VariantPayload::Single(Type::String, zero)),
            ("Cancelled", VariantPayload::Unit),
            ("DeadlineBlown", VariantPayload::Unit),
        ],
        "TaskStatus" => vec![
            ("Running", VariantPayload::Unit),
            ("Paused", VariantPayload::Unit),
            ("CancelRequested", VariantPayload::Unit),
        ],
        _ => return None,
    };
    Some(
        variants
            .into_iter()
            .map(|(name, payload)| (name.to_string(), (zero, payload)))
            .collect(),
    )
}

/// D-SERVICE1=D: service failures remain a typed closed sum across AOT,
/// ambient, and persisted/comptime boundaries.
pub(crate) fn core_service_error_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
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

/// D-FOUND-REALTIME1=A: typed lifecycle methods on the callback stream.
pub fn realtime_stream_method_return(
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    match (method, n_args) {
        ("next_deadline", 0) => Some(Some(Type::Named("Instant".to_string()))),
        ("receipt", 0) => Some(Some(Type::Named("RealtimeReceipt".to_string()))),
        ("cancel", 0) => Some(Some(unit_ty())),
        ("is_cancelled", 0) => Some(Some(Type::Bool)),
        _ => None,
    }
}

/// D-CALLVALUE1=B: does a Core handle type own a method under this name?
///
/// The plugin method set is interface-specific and is resolved by
/// `check_plugin_method`; it must not be treated as a closed Core handle table.
/// Keeping it out of this predicate prevents a stale compatibility alias from
/// hijacking ordinary function-value projection.
pub fn core_handle_owns_method(handle_ty: &str, method: &str) -> bool {
    match handle_ty {
        "Mod" => super::mod_method_return_ty(method).is_some(),
        _ => false,
    }
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
        "MappedFile" => match (method, n_args) {
            ("window" | "window_len", 2) => Some(Some(result_ty(
                Type::Apply {
                    name: "View".to_string(),
                    args: vec![Type::List(Box::new(u8_ty()))],
                },
                io.clone(),
            ))),
            ("lines", 0) => Some(Some(crate::Collections::view_iter_ty(Type::Apply {
                name: "View".to_string(),
                args: vec![Type::List(Box::new(u8_ty()))],
            }))),
            ("len", 0) => Some(Some(Type::Int)),
            ("is_empty", 0) => Some(Some(Type::Bool)),
            _ => None,
        },
        "FileScope" => match (method, n_args) {
            ("read", 1) => Some(Some(result_ty(Type::String, io))),
            _ => None,
        },
        "FileReader" => match method {
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
            Type::Option(Box::new(Type::Named("CSVRow".to_string()))),
            error,
        ))),
        // DataStream<T>.next is handled specially in method_calls (needs T).
        ("CSVWriter", "write", 1) | ("CSVWriter", "flush" | "finish", 0) => {
            Some(Some(result_ty(unit, error)))
        }
        ("XMLReader", "next", 0) => Some(Some(result_ty(
            Type::Option(Box::new(Type::Named("DataTree".to_string()))),
            error,
        ))),
        ("XMLWriter", "write", 1) | ("XMLWriter", "flush" | "finish", 0) => {
            Some(Some(result_ty(unit, error)))
        }
        ("CBORReader", "next", 0) => Some(Some(result_ty(
            Type::Option(Box::new(Type::Named("DataEvent".to_string()))),
            error,
        ))),
        ("CBORWriter", "write", 1) | ("CBORWriter", "flush" | "finish", 0) => {
            Some(Some(result_ty(unit, error)))
        }
        _ => None,
    }
}

/// E2-M10: field definitions for compiler-known constructable struct types.
/// Returns `Some(fields)` when the named type is a prelude struct users can construct.
pub(crate) fn core_constructable_fields(type_name: &str) -> Option<Vec<(String, Type)>> {
    let str_ty = Type::String;
    match type_name {
        // D-FAIL-ERROR1=A: constructor calls normalize to this private shape.
        name if name == Syntax::TYPE_ERR => Some(vec![
            ("message".to_string(), Type::String),
            ("code".to_string(), Type::Option(Box::new(Type::String))),
            (
                "cause".to_string(),
                Type::Option(Box::new(Type::Named(Syntax::TYPE_ERR.to_string()))),
            ),
        ]),
        "HandleId" | "TaskId" | "EventId" => Some(vec![("value".to_string(), Type::Named("Count".to_string()))]),
        "HistoryCase" => Some(vec![
            ("case_id".to_string(), Type::String),
            ("seed".to_string(), Type::Named("Count".to_string())),
            ("operations".to_string(), Type::List(Box::new(Type::Named("HistoryOperation".to_string())))),
            ("schedule".to_string(), Type::List(Box::new(Type::Named("HistoryScheduleChoice".to_string())))),
        ]),
        "HistoryOperation" => Some(vec![
            ("index".to_string(), Type::Named("Count".to_string())),
            ("name".to_string(), Type::String),
            ("arguments".to_string(), Type::List(Box::new(Type::Named("HistoryValue".to_string())))),
            ("creates".to_string(), Type::List(Box::new(Type::Named("HandleId".to_string())))),
            ("consumes".to_string(), Type::List(Box::new(Type::Named("HandleId".to_string())))),
            ("preconditions".to_string(), Type::List(Box::new(Type::Named("HistoryPrecondition".to_string())))),
            ("depends_on".to_string(), Type::List(Box::new(Type::Named("Count".to_string())))),
            ("task".to_string(), Type::Option(Box::new(Type::Named("TaskId".to_string())))),
            ("event".to_string(), Type::Option(Box::new(Type::Named("EventId".to_string())))),
        ]),
        "HistoryScheduleChoice" => Some(vec![
            ("operation".to_string(), Type::Named("Count".to_string())),
            ("task".to_string(), Type::Option(Box::new(Type::Named("TaskId".to_string())))),
            ("event".to_string(), Type::Option(Box::new(Type::Named("EventId".to_string())))),
            ("choice".to_string(), Type::String),
        ]),
        "HistoryBounds" => Some(vec![
            ("max_steps".to_string(), Type::Named("Count".to_string())),
            ("max_resources".to_string(), Type::Named("Count".to_string())),
            ("max_shrink_attempts".to_string(), Type::Named("Count".to_string())),
            ("max_discarded_cases".to_string(), Type::Named("Count".to_string())),
        ]),
        "HistoryDistribution" => Some(vec![
            ("operation".to_string(), Type::String),
            ("weight".to_string(), Type::Named("Count".to_string())),
        ]),
        "HistoryRng" => Some(vec![]),
        "TypedHistoryCase" => None,
        "HistoryStrategy" => None,
        "TestComparison" => Some(vec![
            ("status".to_string(), Type::String),
            ("relation".to_string(), Type::String),
            ("source".to_string(), Type::String),
            ("tool".to_string(), Type::String),
            ("target".to_string(), Type::String),
            ("seed".to_string(), Type::Option(Box::new(Type::Int))),
            (
                "case_ids".to_string(),
                Type::List(Box::new(Type::String)),
            ),
            (
                "inputs".to_string(),
                Type::List(Box::new(Type::Named("DataTree".to_string()))),
            ),
            (
                "reference".to_string(),
                Type::List(Box::new(Type::Named("DataTree".to_string()))),
            ),
            (
                "candidate".to_string(),
                Type::List(Box::new(Type::Named("DataTree".to_string()))),
            ),
            ("first_difference".to_string(), Type::Int),
            ("reason".to_string(), Type::String),
            ("universal_proof".to_string(), Type::Bool),
        ]),
        // D-LIB-CALLGRANT1=A: host policy is explicit and path-scoped.
        "ModGrant" => Some(vec![(
            "read".to_string(),
            Type::List(Box::new(Type::String)),
        )]),
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
            (
                "ambiguous".to_string(),
                Type::Named("TextWidthAmbiguous".to_string()),
            ),
            (
                "controls".to_string(),
                Type::Named("TextWidthControls".to_string()),
            ),
        ]),
        "IOContext" => Some(vec![
            (
                "operation".to_string(),
                Type::Named(Syntax::TYPE_IO_OPERATION.to_string()),
            ),
            ("resource".to_string(), Type::Option(Box::new(Type::String))),
            ("os_code".to_string(), Type::Option(Box::new(Type::Int))),
            ("cause".to_string(), Type::Option(Box::new(Type::String))),
        ]),
        "AsyncPolicy" => Some(vec![
            ("capacity".to_string(), Type::Int),
            ("overflow".to_string(), Type::Named("Overflow".to_string())),
        ]),
        // D-DX-FORM1=A: generated field descriptors are compiler-known
        // records. `web.form(Model, action: handler)` constructs these
        // literals after projecting the model fields, so they must use the
        // same core-record path as explicit prelude struct literals.
        "WebFormFieldSpec" => Some(vec![
            ("name".to_string(), str_ty.clone()),
            (
                "value_type".to_string(),
                Type::Named("WebFormValueType".to_string()),
            ),
            ("required".to_string(), Type::Bool),
            (
                "default".to_string(),
                Type::Option(Box::new(Type::String)),
            ),
            ("label".to_string(), str_ty.clone()),
            (
                "control".to_string(),
                Type::Named("WebFormControl".to_string()),
            ),
            ("group".to_string(), Type::Option(Box::new(Type::String))),
            ("wire_name".to_string(), str_ty),
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
            (
                "max_total_bytes".to_string(),
                Type::Option(Box::new(Type::Int)),
            ),
            ("max_expansion_depth".to_string(), Type::Int),
            ("max_expansion_bytes".to_string(), Type::Int),
        ]),
        "CSVRow" => Some(vec![
            ("fields".to_string(), Type::List(Box::new(Type::String))),
            ("line".to_string(), Type::Int),
        ]),
        "DataLimits" => Some(vec![
            (
                "encoding".to_string(),
                Type::Named("EncodingLimits".to_string()),
            ),
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
            (
                "cause".to_string(),
                Type::Option(Box::new(Type::Named("EncodingError".to_string()))),
            ),
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
            (
                "security".to_string(),
                Type::Named("SMTPSecurity".to_string()),
            ),
            ("auth".to_string(), Type::Named("SMTPAuth".to_string())),
            (
                "recipient_policy".to_string(),
                Type::Named("RecipientPolicy".to_string()),
            ),
            ("trust".to_string(), Type::Named("TLSTrust".to_string())),
            ("limits".to_string(), Type::Named("Limits".to_string())),
            (
                "dkim".to_string(),
                Type::Option(Box::new(Type::Named("DkimConfig".to_string()))),
            ),
        ]),
        "DkimConfig" => Some(vec![
            ("domain".to_string(), Type::String),
            ("selector".to_string(), Type::String),
            (
                "private_key".to_string(),
                crate::Sema::Diagnostics::core_crypto_nominal(Type::Named("Secret".to_string())),
            ),
            (
                "signed_headers".to_string(),
                Type::List(Box::new(Type::String)),
            ),
        ]),
        "EncodingCause" => Some(vec![
            ("kind".to_string(), Type::String),
            ("os_code".to_string(), Type::Option(Box::new(Type::Int))),
            ("message".to_string(), Type::String),
        ]),
        "EncodingError" => Some(vec![
            (
                "format".to_string(),
                Type::Named("EncodingFormat".to_string()),
            ),
            (
                "kind".to_string(),
                Type::Named("EncodingErrorKind".to_string()),
            ),
            ("byte_offset".to_string(), Type::Int),
            ("line".to_string(), Type::Option(Box::new(Type::Int))),
            ("column".to_string(), Type::Option(Box::new(Type::Int))),
            ("path".to_string(), Type::String),
            ("reason".to_string(), Type::String),
            (
                "cause".to_string(),
                Type::Option(Box::new(Type::Named("EncodingCause".to_string()))),
            ),
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
            (
                "entities".to_string(),
                Type::Named("XMLEntityPolicy".to_string()),
            ),
            ("limits".to_string(), Type::Named("XMLLimits".to_string())),
        ]),
        "XMLRenderOptions" => Some(vec![
            (
                "encoding".to_string(),
                Type::Named("XMLEncoding".to_string()),
            ),
            (
                "lexical".to_string(),
                Type::Named("XMLLexicalPolicy".to_string()),
            ),
        ]),
        "XMLCanonical" => Some(vec![
            (
                "mode".to_string(),
                Type::Named("XMLCanonicalMode".to_string()),
            ),
            ("comments".to_string(), Type::Bool),
            (
                "inclusive_prefixes".to_string(),
                Type::List(Box::new(Type::String)),
            ),
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
            (
                "accepted".to_string(),
                Type::List(Box::new(Type::Named("RecipientReport".to_string()))),
            ),
            (
                "rejected".to_string(),
                Type::List(Box::new(Type::Named("RecipientReport".to_string()))),
            ),
            ("response_code".to_string(), Type::Int),
            ("response".to_string(), Type::String),
            ("accepted_at".to_string(), Type::String),
        ]),
        _ => None,
    }
}

/// Constructable fields for generic compiler-owned records. The ordinary
/// constructable table intentionally has no type arguments, so callers with a
/// `Type::Apply` resolve these shapes here.
pub(crate) fn core_generic_constructable_fields(
    type_name: &str,
    args: &[Type],
) -> Option<Vec<(String, Type)>> {
    if args.len() != 1 {
        return None;
    }
    let command = args[0].clone();
    let typed_case = || Type::Apply {
        name: "TypedHistoryCase".to_string(),
        args: vec![command.clone()],
    };
    match type_name {
        "TypedHistoryCase" => Some(vec![
            ("case".to_string(), Type::Named("HistoryCase".to_string())),
            ("commands".to_string(), Type::List(Box::new(command))),
        ]),
        "HistoryStrategy" => {
            let typed_case = typed_case();
            Some(vec![
                (
                    "generate".to_string(),
                    Type::Fn {
                        params: vec![
                            Type::Named("HistoryRng".to_string()),
                            Type::Named("Count".to_string()),
                            Type::Named("Count".to_string()),
                        ],
                        ret: Some(Box::new(Type::Option(Box::new(typed_case.clone())))),
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: None,
                    },
                ),
                (
                    "rebuild".to_string(),
                    Type::Fn {
                        params: vec![Type::Named("HistoryCase".to_string())],
                        ret: Some(Box::new(Type::Option(Box::new(typed_case.clone())))),
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: None,
                    },
                ),
                (
                    "valid".to_string(),
                    Type::Fn {
                        params: vec![typed_case],
                        ret: Some(Box::new(Type::Bool)),
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: None,
                    },
                ),
                (
                    "bounds".to_string(),
                    Type::Named("HistoryBounds".to_string()),
                ),
                (
                    "distributions".to_string(),
                    Type::List(Box::new(Type::Named("HistoryDistribution".to_string()))),
                ),
            ])
        }
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
            "Configuration",
            "DNS",
            "Connect",
            "TLS",
            "Auth",
            "Protocol",
            "Rejected",
            "Transient",
            "TimedOut",
            "Cancelled",
            "DeliveryUnknown",
        ] {
            let fields = [
                ("operation", Type::String),
                ("server", Type::Option(Box::new(Type::String))),
                ("code", Type::Option(Box::new(Type::Int))),
                ("reason", Type::String),
            ]
            .into_iter()
            .map(|(field, ty)| VariantField {
                name: field.to_string(),
                name_span: zero,
                ty,
                ty_span: zero,
            })
            .collect();
            variants.insert(name.to_string(), (zero, VariantPayload::Named(fields)));
        }
    } else if enum_name == "SMTPAuth" {
        variants.insert("None".to_string(), (zero, VariantPayload::Unit));
        variants.insert(
            "Password".to_string(),
            (
                zero,
                VariantPayload::Named(vec![
                    VariantField {
                        name: "username".to_string(),
                        name_span: zero,
                        ty: Type::String,
                        ty_span: zero,
                    },
                    VariantField {
                        name: "password".to_string(),
                        name_span: zero,
                        ty: crate::Sema::Diagnostics::core_crypto_nominal(Type::Named(
                            "Secret".to_string(),
                        )),
                        ty_span: zero,
                    },
                ]),
            ),
        );
    } else if enum_name == "TLSTrust" {
        variants.insert("System".to_string(), (zero, VariantPayload::Unit));
        variants.insert(
            "SystemPlusCa".to_string(),
            (
                zero,
                VariantPayload::Named(vec![VariantField {
                    name: "pem".to_string(),
                    name_span: zero,
                    ty: Type::List(Box::new(Type::IntN {
                        signed: false,
                        bits: 8,
                    })),
                    ty_span: zero,
                }]),
            ),
        );
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
            variants.insert(
                "SystemPlus".to_string(),
                (zero, VariantPayload::Single(roots.clone(), zero)),
            );
            variants.insert(
                "CustomOnly".to_string(),
                (zero, VariantPayload::Single(roots, zero)),
            );
        }
        _ => return None,
    }
    Some(variants)
}

/// D-ENCSTREAM-SURFACE1=A: closed shared stream enums.
pub(crate) fn core_encoding_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::VariantPayload;
    let zero = Span::new(0, 0);
    let mut variants = std::collections::HashMap::new();
    let units: &[&str] = match enum_name {
        "EncodingFormat" => &["JSON", "JSONL", "CSV", "TOML", "YAML", "XML", "CBOR"],
        "DataErrorKind" => &[
            "Decode",
            "Limit",
            "IO",
            "Empty",
            "InvalidArgument",
            "NonFinite",
            "Overflow",
            "State",
            "Bridge",
        ],
        "DataEvent" => &["Null", "ArrayStart", "ArrayEnd", "ObjectStart", "ObjectEnd"],
        "DataFormat" => &["CSV", "JSON", "JSONL", "Parquet", "Arrow"],
        "DataLoaderKind" => &["File", "Url", "Database", "Value"],
        "DataFreshness" => &["Pending", "Fresh", "Stale", "Error", "Offline", "Cancelled"],
        "DataInvalidationCause" => &[
            "None",
            "Loader",
            "Input",
            "ArchiveMember",
            "Parameters",
            "Credential",
            "Capability",
            "Manual",
        ],
        "JobQueueState" => &[
            "Queued",
            "Running",
            "Retrying",
            "Completed",
            "Failed",
            "DeadLettered",
            "Cancelled",
        ],
        "JobQueueDeliveryPolicy" => &["AtLeastOnce"],
        "JetDataPlotMark" => &["Line", "Bar", "Point"],
        "JetDataPlotChannel" => &["X", "Y", "Color", "Size", "Text", "Detail"],
        "JetDataPlotAggregate" => &["None", "Count", "Sum", "Mean", "Min", "Max"],
        "JetDataPlotFilterOp" => &[
            "Equal",
            "NotEqual",
            "Less",
            "LessEqual",
            "Greater",
            "GreaterEqual",
        ],
        "JetDataPlotScaleKind" => &["Linear", "Log", "Band", "Point"],
        "JetDataPlotLegendPosition" => &["Top", "Right", "Bottom", "Left"],
        "JetDataPlotFacetKind" => &["Row", "Column"],
        "JetDataPlotInteraction" => &["Hover", "Select", "Zoom", "Pan", "Brush"],
        "JetDataPlotBackend" => &["Terminal", "Browser", "Native", "Export"],
        "JetDataPlotSupport" => &["Supported", "Degraded", "Unsupported"],
        "JetDataPlotErrorKind" => &["InvalidArgument", "NonFinite", "Unsupported", "Empty", "Limit"],
        "JetDataPlotRenderFormat" => &["Text", "Svg"],
        "CBORErrorKind" => &[
            "Syntax",
            "Truncated",
            "Unsupported",
            "Limit",
            "TypeMismatch",
            "TrailingData",
            "NonCanonical",
        ],
        "XMLReason" => &[
            "InvalidEncoding",
            "Malformed",
            "MismatchedTag",
            "InvalidName",
            "Namespace",
            "DuplicateAttribute",
            "Entity",
            "EntityCycle",
            "Limit",
            "Canonicalization",
            "Shape",
            "Unsupported",
        ],
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
            ("Bool", Type::Bool),
            ("Int", Type::Int),
            ("Float", Type::Float),
            ("Text", Type::String),
            (Syntax::TYPE_BYTES, Type::List(Box::new(u8_ty()))),
            ("Key", Type::String),
        ] {
            variants.insert(name.to_string(), (zero, VariantPayload::Single(ty, zero)));
        }
    }
    if enum_name == "XMLEntityPolicy" {
        variants.insert(
            "Resolve".to_string(),
            (
                zero,
                VariantPayload::Single(
                    Type::Map {
                        key: Box::new(Type::String),
                        key_span: None,
                        value: Box::new(Type::String),
                    },
                    zero,
                ),
            ),
        );
    }
    if enum_name == "JetDataPlotValue" {
        for (name, ty) in [
            ("Text", Type::String),
            ("Integer", Type::Int),
            ("Number", Type::Float),
            ("Boolean", Type::Bool),
        ] {
            variants.insert(name.to_string(), (zero, VariantPayload::Single(ty, zero)));
        }
    }
    if enum_name == "JetDataPlotValue" {
        variants.insert("Null".to_string(), (zero, VariantPayload::Unit));
    }
    if enum_name == "JetDataPlotDomain" {
        variants.insert(
            "Numeric".to_string(),
            (
                zero,
                VariantPayload::Named(vec![
                    VariantField {
                        name: "min".to_string(),
                        name_span: zero,
                        ty: Type::Float,
                        ty_span: zero,
                    },
                    VariantField {
                        name: "max".to_string(),
                        name_span: zero,
                        ty: Type::Float,
                        ty_span: zero,
                    },
                ]),
            ),
        );
        variants.insert(
            "Categories".to_string(),
            (
                zero,
                VariantPayload::Single(Type::List(Box::new(Type::String)), zero),
            ),
        );
    }
    if enum_name == "JetDataPlotTransform" {
        variants.insert(
            "Filter".to_string(),
            (
                zero,
                VariantPayload::Named(vec![
                    VariantField {
                        name: "field".to_string(),
                        name_span: zero,
                        ty: Type::Named("JetDataPlotField".to_string()),
                        ty_span: zero,
                    },
                    VariantField {
                        name: "op".to_string(),
                        name_span: zero,
                        ty: Type::Named("JetDataPlotFilterOp".to_string()),
                        ty_span: zero,
                    },
                    VariantField {
                        name: "value".to_string(),
                        name_span: zero,
                        ty: Type::Named("JetDataPlotValue".to_string()),
                        ty_span: zero,
                    },
                ]),
            ),
        );
        variants.insert(
            "Sort".to_string(),
            (
                zero,
                VariantPayload::Named(vec![
                    VariantField {
                        name: "field".to_string(),
                        name_span: zero,
                        ty: Type::Named("JetDataPlotField".to_string()),
                        ty_span: zero,
                    },
                    VariantField {
                        name: "descending".to_string(),
                        name_span: zero,
                        ty: Type::Bool,
                        ty_span: zero,
                    },
                ]),
            ),
        );
        variants.insert(
            "Bin".to_string(),
            (
                zero,
                VariantPayload::Named(vec![
                    VariantField {
                        name: "field".to_string(),
                        name_span: zero,
                        ty: Type::Named("JetDataPlotField".to_string()),
                        ty_span: zero,
                    },
                    VariantField {
                        name: "step".to_string(),
                        name_span: zero,
                        ty: Type::Float,
                        ty_span: zero,
                    },
                ]),
            ),
        );
        variants.insert(
            "Aggregate".to_string(),
            (
                zero,
                VariantPayload::Named(vec![
                    VariantField {
                        name: "group_by".to_string(),
                        name_span: zero,
                        ty: Type::List(Box::new(Type::Named("JetDataPlotField".to_string()))),
                        ty_span: zero,
                    },
                    VariantField {
                        name: "field".to_string(),
                        name_span: zero,
                        ty: Type::Named("JetDataPlotField".to_string()),
                        ty_span: zero,
                    },
                    VariantField {
                        name: "aggregate".to_string(),
                        name_span: zero,
                        ty: Type::Named("JetDataPlotAggregate".to_string()),
                        ty_span: zero,
                    },
                ]),
            ),
        );
    }
    Some(variants)
}

/// D-AUTH-TOKENPOLICY1=A: inspectable verifier failures.
pub(crate) fn core_auth_variants(
    enum_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    use crate::Diagnostics::Span;
    use crate::AST::{VariantField, VariantPayload};
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
    for name in [
        "MalformedToken",
        "UnsupportedToken",
        "MissingClaim",
        "DecodeError",
    ] {
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
    variants.insert("TokenNotYetValid".to_string(), (zero, VariantPayload::Unit));
    Some(variants)
}
