mod jet_std {
    // The one outcome carrier: from the flat Prelude under AOT, from the host
    // module when another tier includes this file.
    #[allow(unused_imports)]
    use super::*;
    // D-IOERROR-TREE1=A: one public context shape for every byte-stream error.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum IOOperation {
        Read,
        Write,
        Flush,
        Connect,
        Accept,
        Close,
        Resolve,
        Codec,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct IOContext {
        pub operation: IOOperation,
        pub resource: JetOutcome<String, JetAbsent>,
        pub os_code: JetOutcome<i64, JetAbsent>,
        pub cause: JetOutcome<String, JetAbsent>,
    }

    /// D-PROCESS-RESOURCE1=A: the limit that stopped a process session. A
    /// receipt keeps the typed wall-time fact; failed launches use the same
    /// enum in `IOError::ResourceLimit`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum ProcessResourceLimit {
        WallTime,
        CpuTime,
        Memory,
        OpenFiles,
        Output,
    }

    impl IOContext {
        // The constructor still takes Rust plumbing so every host call site reads
        // the same; the carrier starts here, once.
        pub fn new(operation: IOOperation, resource: Option<String>, os_code: Option<i64>, cause: Option<String>) -> Self {
            Self {
                operation,
                resource: jet_outcome_of(resource),
                os_code: jet_outcome_of(os_code),
                cause: jet_outcome_of(cause),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum IOError {
        InvalidInput(IOContext),
        NotFound(IOContext),
        PermissionDenied(IOContext),
        TimedOut(IOContext),
        Cancelled(IOContext),
        Closed(IOContext),
        Protocol(IOContext),
        Other(IOContext),
        ResourceLimit(ProcessResourceLimit),
    }

    impl IOError {
        pub fn other(operation: IOOperation, resource: Option<String>, cause: impl ToString) -> Self {
            Self::Other(IOContext::new(operation, resource, None, Some(cause.to_string())))
        }
    }

    // D-ENV-MUTATE1=A: failures never carry input or host-backend text.
    #[derive(Clone, Debug, PartialEq)]
    pub enum EnvError {
        InvalidName,
        InvalidValue,
        NonUnicode,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct UTF8Error {
        pub message: String,
    }

    // D-TEXTWIDTH1=B: `TextWidth.{ ambiguous: .Wide, controls: .Reject }` —
    // the explicit-policy override for `core.text.display_width`. The
    // one-arg call uses the portable default (Narrow/Zero) directly and
    // never constructs this type.
    #[derive(Clone, Debug, PartialEq)]
    pub enum TextWidthAmbiguous {
        Narrow,
        Wide,
    }
    #[derive(Clone, Debug, PartialEq)]
    pub enum TextWidthControls {
        Zero,
        Reject,
    }
    #[derive(Clone, Debug, PartialEq)]
    pub struct TextWidth {
        pub ambiguous: TextWidthAmbiguous,
        pub controls: TextWidthControls,
    }
    #[derive(Clone, Debug, PartialEq)]
    pub struct TextError {
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct ProcessReceipt {
        pub code: i64,
        pub output: String,
        pub errors: String,
        pub success: bool,
        // D-FAIL-CARRIER1=A: sema declares `ProcessResult.signal` an
        // `Option<Int>`, and the one Rust spelling of a Jet `?T` is
        // `JetOutcome<T, JetAbsent>` (Codegen/Context.rs `rust_type`). A raw
        // `Option<i64>` here was a SECOND optional representation, so a
        // `.Val`/`.None` pattern on the field emitted the carrier's `Ok`/`Err`
        // arms against a Rust `Option` and rustc rejected generated code.
        pub signal: JetOutcome<i64, JetAbsent>,
        pub timed_out: bool,
        // D-AGENT-EXEC2: a receipt is the result plus the facts bound to the
        // launch transaction. These fields are deliberately ordinary data so
        // every engine can marshal the same record without reimplementing
        // policy semantics.
        pub executable_identity: String,
        pub argv: Vec<String>,
        pub input_digest: String,
        pub policy_digest: String,
        pub backend: String,
        pub authority: Vec<String>,
        pub descendants: String,
        pub limits: Vec<String>,
        pub outputs: Vec<String>,
        pub redacted: bool,
        pub pid: i64,
        pub limit_hit: JetOutcome<ProcessResourceLimit, JetAbsent>,
    }

    // The old internal spelling remains a Rust alias while the user-facing
    // execution result is the ratified ProcessReceipt type.
    pub type ProcessResult = ProcessReceipt;

    #[derive(Clone, Debug)]
    pub struct JetURL {
        pub scheme: String,
        pub username: Option<String>,
        pub password: Option<String>,
        pub host: Option<String>,
        pub port: Option<i64>,
        pub path: String,
        pub query: Vec<(String, String)>,
        pub fragment: Option<String>,
        pub typed_host: Option<Vec<(String, bool)>>,
        pub typed_path: Option<Vec<(String, bool)>>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JetMIME {
        pub top: String,
        pub sub: String,
        pub params: Vec<(String, String)>,
    }
