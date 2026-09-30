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
        // Exact Jet `Int` fields use the owned numeric representation so AOT
        // field lowering and the shared Prelude type agree on one ABI.
        pub code: jet_foundation::Numeric::JetInt,
        pub output: String,
        pub errors: String,
        pub success: bool,
        // D-FAIL-CARRIER1=A: sema declares `ProcessResult.signal` an
        // `Option<Int>`. The owned representation for `Int` is `JetInt`, and
        // the one Rust spelling of a Jet `?T` is
        // `JetOutcome<T, JetAbsent>`. A raw `Option<i64>` here was a SECOND
        // optional representation, so a `.Val`/`.None` pattern on the field
        // emitted the carrier's `Ok`/`Err` arms against a Rust `Option` and
        // rustc rejected generated code.
        pub signal: JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>,
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
        pub pid: jet_foundation::Numeric::JetInt,
        pub limit_hit: JetOutcome<ProcessResourceLimit, JetAbsent>,
    }

    // The old internal spelling remains a Rust alias while the user-facing
    // execution result is the ratified ProcessReceipt type.
    pub type ProcessResult = ProcessReceipt;

    /// The `core.net.url.URL` carrier: exactly the field layout declared in
    /// `Core/net/url.jet`, so generated struct literals and field reads name
    /// the same Rust fields (`port` is the native Int slot, -1 when absent).
    #[derive(Clone, Debug, PartialEq)]
    pub struct JetURL {
        pub scheme: String,
        pub user: String,
        pub password: String,
        pub host: String,
        pub port: i64,
        pub path: String,
        pub query: String,
        pub fragment: String,
        pub raw: String,
    }

    /// The Prelude URL kernel's working form (`UrlMime.rs`): optional
    /// components, decoded query pairs, and typed-literal hole boundaries.
    #[derive(Clone, Debug)]
    pub struct JetURLParts {
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

    impl JetURL {
        /// Project kernel parts onto the Core layout. An absent port takes the
        /// scheme default, as `core.net.url.parse` does.
        pub fn from_url_parts(parts: JetURLParts) -> Self {
            let raw = parts.to_string_value();
            let port = parts
                .port
                .or_else(|| parts.default_port().ok())
                .unwrap_or(-1);
            JetURL {
                query: jet_url_render_query(&parts.query),
                scheme: parts.scheme,
                user: parts.username.unwrap_or_default(),
                password: parts.password.unwrap_or_default(),
                host: parts.host.unwrap_or_default(),
                port,
                path: parts.path,
                fragment: parts.fragment.unwrap_or_default(),
                raw,
            }
        }

        fn parts(&self) -> JetURLParts {
            let some = |text: &String| (!text.is_empty()).then(|| text.clone());
            let mut parts = JetURLParts {
                scheme: self.scheme.clone(),
                username: some(&self.user),
                password: some(&self.password),
                host: some(&self.host),
                port: None,
                path: self.path.clone(),
                query: jet_url_parse_query(&self.query).unwrap_or_default(),
                fragment: some(&self.fragment),
                typed_host: None,
                typed_path: None,
            };
            if self.port >= 0 && parts.default_port().ok() != Some(self.port) {
                parts.port = Some(self.port);
            }
            parts
        }

        pub fn scheme(&self) -> String {
            self.scheme.clone()
        }
        pub fn username(&self) -> String {
            self.user.clone()
        }
        pub fn password(&self) -> String {
            self.password.clone()
        }
        pub fn userinfo(&self) -> String {
            self.parts().userinfo()
        }
        pub fn authority(&self) -> String {
            self.parts().authority()
        }
        pub fn host(&self) -> JetOutcome<String, JetAbsent> {
            self.parts().host()
        }
        pub fn port(&self) -> JetOutcome<i64, JetAbsent> {
            self.parts().port()
        }
        pub fn default_port(&self) -> JetOutcome<i64, JetAbsent> {
            self.parts().default_port()
        }
        pub fn path(&self) -> String {
            self.path.clone()
        }
        pub fn path_segments(&self) -> Vec<String> {
            self.parts().path_segments()
        }
        pub fn query(&self) -> String {
            self.query.clone()
        }
        pub fn query_pairs(&self) -> Vec<Vec<String>> {
            self.parts().query_pairs()
        }
        pub fn fragment(&self) -> JetOutcome<String, JetAbsent> {
            self.parts().fragment()
        }
        pub fn normalize(&self) -> Self {
            Self::from_url_parts(self.parts().normalize())
        }
        pub fn join(&self, rel: &String) -> Result<Self, String> {
            self.parts().join(rel).map(Self::from_url_parts)
        }
        pub fn set_query(&self, key: &String, value: &String) -> Self {
            Self::from_url_parts(self.parts().set_query(key, value))
        }
        pub fn add_query(&self, key: &String, value: &String) -> Self {
            Self::from_url_parts(self.parts().add_query(key, value))
        }
        pub fn to_string_value(&self) -> String {
            self.parts().to_string_value()
        }
    }

    impl crate::JetShow for JetURL {
        fn jet_show(&self) -> String {
            self.to_string_value()
        }
    }

    impl crate::JetDisplay for JetURL {
        fn jet_display(&self) -> String {
            self.to_string_value()
        }
    }

    impl crate::JetDebug for JetURL {
        fn jet_debug(&self) -> String {
            self.to_string_value()
        }
    }

    /// AOT typed `url"..."` heads build the Core carrier from the one kernel.
    pub fn jet_typed_url_literal(literals: &[&str], holes: Vec<String>) -> JetURL {
        JetURL::from_url_parts(jet_typed_url_parts_literal(literals, holes))
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JetMIME {
        pub top: String,
        pub sub: String,
        pub params: Vec<(String, String)>,
    }
