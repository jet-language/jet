    // Exactly the three stream modes. `Stream` and
    // `Capture` both pipe the child's stream (`Stdio::piped()`) — they differ
    // only in which Jet-level API is meant to drain them (`Child.stdout.lines()`
    // for `Stream`, the collected `ProcessResult.output`/`.errors` for `Capture`).
    #[derive(Clone, Debug, PartialEq)]
    pub enum ProcessStreamMode {
        Stream,
        Inherit,
        Capture,
    }

    // D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D: expert controls stay on
    // ProcessSpec and ProcessChild. The Unix PTY successor fills the session
    // handle while unsupported targets keep the launch fail-closed.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TerminalSize {
        pub cols: i64,
        pub rows: i64,
    }

    impl Default for TerminalSize {
        fn default() -> Self {
            Self {
                cols: super::terminal_default::JET_TERMINAL_DEFAULT_COLS,
                rows: super::terminal_default::JET_TERMINAL_DEFAULT_ROWS,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum TerminalMode {
        Raw,
        Cooked,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TerminalPolicy {
        pub size: TerminalSize,
        pub mode: TerminalMode,
    }

    impl Default for TerminalPolicy {
        fn default() -> Self {
            Self {
                size: TerminalSize::default(),
                mode: TerminalMode::Cooked,
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct TerminalSession {
        // The master stays shared by the child stream handles and resize. The
        // public Jet value remains an opaque, cloneable session handle.
        pub(crate) master: std::rc::Rc<std::fs::File>,
    }

    impl PartialEq for TerminalSession {
        fn eq(&self, other: &Self) -> bool {
            std::rc::Rc::ptr_eq(&self.master, &other.master)
        }
    }

    impl Eq for TerminalSession {}

    // The two enums keep the normal pipe path and the PTY path behind one
    // ProcessChild shape. A terminal has one byte stream, so it is exposed as
    // stdout; stderr is intentionally absent rather than a second reader on
    // the same PTY master.
    #[derive(Debug)]
    pub enum ProcessStdin {
        Pipe(std::process::ChildStdin),
        Terminal(std::fs::File),
    }

    #[derive(Debug)]
    pub enum ProcessReader {
        Stdout(std::process::ChildStdout),
        Stderr(std::process::ChildStderr),
        Terminal(std::fs::File),
    }

    impl EncodingError {
        /// D-ENCSTREAM-SURFACE1=A: handle-free IO snapshot when kind is IO.
        pub fn cause(&self) -> JetOutcome<EncodingCause, JetAbsent> {
            self.cause.clone()
        }
        fn display_text(&self) -> String {
            super::jet_encoding_error_kernel_show(
                &format!("{:?}", self.format),
                &format!("{:?}", self.kind),
                self.byte_offset,
                self.line.as_ref().ok().copied(),
                self.column.as_ref().ok().copied(),
                &self.path,
                &self.reason,
            )
        }
    }
    impl std::fmt::Display for EncodingError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.display_text())
        }
    }
    impl super::JetShow for EncodingError {
        fn jet_show(&self) -> String {
            self.display_text()
        }
    }
    impl super::JetDisplay for EncodingError {
        fn jet_display(&self) -> String {
            self.display_text()
        }
    }
    impl super::JetDebug for EncodingError {
        fn jet_debug(&self) -> String {
            self.display_text()
        }
    }
    #[derive(Clone, Debug, PartialEq)]
    pub enum DataEvent {
        Null, Bool(bool), Int(i64), Float(f64), Number(String), Text(String), Bytes(Vec<u8>),
        ArrayStart, ArrayEnd, ObjectStart, Key(String), ObjectEnd,
    }
    // D-ENC-CBOR-SURFACE1=A: whole-value CBOR policy and stable typed errors.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CBOROptions {
        pub max_depth: i64,
        pub max_items: i64,
        pub max_bytes: i64,
        pub require_canonical: bool,
    }
    impl CBOROptions {
        pub fn safe() -> Self {
            Self { max_depth: 256, max_items: 1_000_000, max_bytes: 1_073_741_824, require_canonical: false }
        }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum CBORErrorKind { Syntax, Truncated, Unsupported, Limit, TypeMismatch, TrailingData, NonCanonical }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CBORError {
        pub kind: CBORErrorKind,
        pub byte_offset: i64,
        pub path: String,
        pub reason: String,
    }
    impl CBORError {
        fn display_text(&self) -> String {
            super::jet_encoding_error_kernel_show(
                "CBOR",
                &format!("{:?}", self.kind),
                self.byte_offset,
                None,
                None,
                &self.path,
                &self.reason,
            )
        }
    }
    impl std::fmt::Display for CBORError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.display_text())
        }
    }
    impl super::JetShow for CBORError {
        fn jet_show(&self) -> String {
            self.display_text()
        }
    }
    impl super::JetDisplay for CBORError {
        fn jet_display(&self) -> String {
            self.display_text()
        }
    }
    impl super::JetDebug for CBORError {
        fn jet_debug(&self) -> String {
            self.display_text()
        }
    }
    // D-ENC-XML-SURFACE1=A: whole-value XML policy, limits, and stable errors.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum XMLReason {
        InvalidEncoding, Malformed, MismatchedTag, InvalidName, Namespace,
        DuplicateAttribute, Entity, EntityCycle, Limit, Canonicalization,
        Shape, Unsupported,
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct XMLError {
        pub kind: XMLReason,
        pub byte_offset: JetOutcome<i64, JetAbsent>,
        pub line: JetOutcome<i64, JetAbsent>,
        pub column: JetOutcome<i64, JetAbsent>,
        pub path: String,
        pub reason: String,
    }
    impl XMLError {
        fn display_text(&self) -> String {
            super::jet_encoding_error_kernel_show(
                "XML",
                &format!("{:?}", self.kind),
                self.byte_offset.as_ref().ok().copied().unwrap_or(0),
                self.line.as_ref().ok().copied(),
                self.column.as_ref().ok().copied(),
                &self.path,
                &self.reason,
            )
        }
    }
    impl std::fmt::Display for XMLError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.display_text())
        }
    }
    impl super::JetShow for XMLError {
        fn jet_show(&self) -> String {
            self.display_text()
        }
    }
    impl super::JetDisplay for XMLError {
        fn jet_display(&self) -> String {
            self.display_text()
        }
    }
    impl super::JetDebug for XMLError {
        fn jet_debug(&self) -> String {
            self.display_text()
        }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct XMLLimits {
        pub max_depth: i64,
        pub max_nodes: i64,
        pub max_attributes_per_element: i64,
        pub max_name_bytes: i64,
        pub max_text_bytes: i64,
        pub max_entity_declarations: i64,
        pub max_entity_depth: i64,
        pub max_entity_replacement_bytes: i64,
    }
    impl XMLLimits {
        pub fn safe() -> Self {
            Self { max_depth: 256, max_nodes: 1_000_000, max_attributes_per_element: 1024,
                max_name_bytes: 4096, max_text_bytes: 16_777_216, max_entity_declarations: 1024,
                max_entity_depth: 32, max_entity_replacement_bytes: 8_388_608 }
        }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum XMLEntityPolicy {
        Preserve,
        Reject,
        Resolve(std::collections::BTreeMap<String, String>),
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct XMLParseOptions { pub entities: XMLEntityPolicy, pub limits: XMLLimits }
    impl XMLParseOptions {
        pub fn safe() -> Self { Self { entities: XMLEntityPolicy::Preserve, limits: XMLLimits::safe() } }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum XMLEncoding { UTF8, UTF8BOM, UTF16LE, UTF16BE }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum XMLLexicalPolicy { PreserveValid, Deterministic }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct XMLRenderOptions { pub encoding: XMLEncoding, pub lexical: XMLLexicalPolicy }
    impl XMLRenderOptions {
        pub fn safe() -> Self { Self { encoding: XMLEncoding::UTF8, lexical: XMLLexicalPolicy::PreserveValid } }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum XMLCanonicalMode { Inclusive11, Exclusive10 }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct XMLCanonical { pub mode: XMLCanonicalMode, pub comments: bool, pub inclusive_prefixes: Vec<String> }
    pub struct JSONReader {
        pub(crate) input: super::JetFileReader,
        pub(crate) limits: EncodingLimits,
        pub(crate) total: i64,
        pub(crate) offset: i64,
        pub(crate) line: i64,
        pub(crate) column: i64,
        pub(crate) lookahead: Option<u8>,
        pub(crate) frames: Vec<super::JetJSONReadFrame>,
        pub(crate) root_started: bool,
        pub(crate) root_done: bool,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) eof: bool,
        pub(crate) record_mode: bool,
        pub(crate) typed_numbers: bool,
        pub(crate) allocation_budget: Option<super::JetEncodingAllocationBudget>,
        // A string event owns its decoded backing until `next_event` hands the
        // event to the caller.  Keeping that charge live through object-key
        // cloning makes the transient peak observable and releases it exactly
        // once on both success and terminal error.
        pub(crate) output_heap: usize,
    }
    pub struct JSONWriter {
        pub(crate) output: super::JetFileWriter,
        pub(crate) limits: EncodingLimits,
        pub(crate) frames: Vec<super::JetJSONWriteFrame>,
        pub(crate) root_written: bool,
        pub(crate) finished: bool,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) total: i64,
        pub(crate) canonical: bool,
        pub(crate) canonical_frames: Vec<super::JetJSONCanonicalFrame>,
        pub(crate) canonical_retained: usize,
    }
    pub struct JSONLReader {
        pub(crate) json: JSONReader,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) record_index: i64,
    }
    pub struct JSONLWriter {
        pub(crate) json: JSONWriter,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) record_index: i64,
        pub(crate) finished: bool,
        // D-ENCSTREAM-SURFACE1: record LF is stream closure; finish emits it.
        // Drop without finish leaves the last value unterminated on the wire.
        pub(crate) pending_lf: bool,
    }
    pub struct CSVReader {
        pub(crate) input: super::JetFileReader,
        pub(crate) limits: EncodingLimits,
        pub(crate) total: i64,
        pub(crate) offset: i64,
        pub(crate) line: i64,
        pub(crate) column: i64,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) eof: bool,
        pub(crate) record_index: i64,
    }
    pub struct CSVWriter {
        pub(crate) output: super::JetFileWriter,
        pub(crate) limits: EncodingLimits,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) total: i64,
        pub(crate) record_index: i64,
        pub(crate) finished: bool,
        // D-ENCSTREAM-SURFACE1: record CRLF is stream closure; finish emits it.
        // Drop without finish leaves the last row unterminated on the wire.
        pub(crate) pending_crlf: bool,
    }
    pub struct XMLReader {
        pub(crate) input: super::JetFileReader,
        pub(crate) limits: EncodingLimits,
        pub(crate) scanner: super::jet_xml_pull::StreamScanner,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) total: i64,
        pub(crate) eof: bool,
        // D-ENCSTREAM-SURFACE1=A: codec-owned live heap ceiling for retained events.
        pub(crate) allocation: super::JetEncodingAllocationBudget,
    }
    pub struct XMLWriter {
        pub(crate) output: super::JetFileWriter,
        pub(crate) limits: EncodingLimits,
        pub(crate) renderer: super::jet_xml_pull::StreamWriter,
        pub(crate) buffer: Vec<u8>,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) total: i64,
        pub(crate) finished: bool,
        pub(crate) allocation: super::JetEncodingAllocationBudget,
    }
    pub struct CBORReader {
        pub(crate) input: super::JetFileReader,
        pub(crate) limits: EncodingLimits,
        pub(crate) total: i64,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) eof: bool,
        pub(crate) root_done: bool,
        pub(crate) lookahead: Option<u8>,
        pub(crate) frames: Vec<super::JetCBORReadFrame>,
        pub(crate) retained: usize,
        pub(crate) workspace: usize,
        // D-ENCSTREAM-SURFACE1=A: codec-owned live heap ceiling (counting allocator).
        pub(crate) allocation: super::JetEncodingAllocationBudget,
    }
    pub struct CBORWriter {
        pub(crate) output: super::JetFileWriter,
        pub(crate) limits: EncodingLimits,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) total: i64,
        pub(crate) frames: Vec<super::JetCBORWriteFrame>,
        pub(crate) root_written: bool,
        // finish validates one complete root; Drop without finish never claims success
        // and leaves incomplete buffered containers unwritten (≠ finished wire).
        pub(crate) finished: bool,
        pub(crate) retained: usize,
        pub(crate) workspace: usize,
        // D-ENCSTREAM-SURFACE1=A: codec-owned live heap ceiling (counting allocator).
        pub(crate) allocation: super::JetEncodingAllocationBudget,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct ProcessSpec {
        pub cmd: Vec<String>,
        pub cwd: Option<String>,
        pub env_clear: bool,
        pub env_set: Vec<(String, String)>,
        pub env_remove: Vec<String>,
        // `None` (default) closes the child's stdin (`Stdio::null()`) — matches
        // The default is no accidental stdin inheritance.
        pub stdin: Option<ProcessStreamMode>,
        pub stdout: ProcessStreamMode,
        pub stderr: ProcessStreamMode,
        pub timeout_ms: Option<i64>,
        pub output_limit: Option<i64>,
        pub detached: bool,
        // D-PROCESS-SESSION1=A: `.terminal()` asks for a terminal-backed
        // session. Argv execution with no terminal stays the default, so this
        // flag is the one opt-in. A launch that asks for a terminal never runs
        // without one: it fails when no native PTY/ConPTY backend is available.
        pub terminal: Option<TerminalPolicy>,
    }

    #[derive(Clone, Debug)]
    pub struct ProcessChild {
        pub inner: std::rc::Rc<std::cell::RefCell<Option<std::process::Child>>>,
        pub wait_result: std::rc::Rc<std::cell::RefCell<Option<ProcessResult>>>,
        pub stdin: std::rc::Rc<std::cell::RefCell<Option<ProcessStdin>>>,
        pub stdout:
            std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
        pub stderr:
            std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
        pub terminal: JetOutcome<TerminalSession, JetAbsent>,
        pub timeout_ms: Option<i64>,
        pub started: std::time::Instant,
    }

    impl PartialEq for ProcessChild {
        fn eq(&self, other: &Self) -> bool {
            std::rc::Rc::ptr_eq(&self.inner, &other.inner)
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DirEntry {
        pub name: String,
        pub path: String,
        pub is_dir: bool,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Stat {
        pub size: i64,
        pub modified_ms: i64,
        pub created_ms: i64,
        pub readonly: bool,
        pub is_file: bool,
        pub is_dir: bool,
        pub is_symlink: bool,
        pub kind: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct WalkEntry {
        pub path: String,
        pub relative: String,
        pub is_dir: bool,
        pub depth: i64,
    }

    // D-WATCH-SCOPE1 + stdlib-api-laws D4: the watch domain and event kind are
    // closed sets, so they are dot-literal Core enums (same mechanism as
    // `ProcessStreamMode` above), not bare strings a consumer has to spell right.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum WatchDomain {
        File,
        Process,
        Port,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum WatchKind {
        Created,
        Modified,
        Removed,
        Error,
        Exited,
        Ready,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct WatchEvent {
        pub domain: WatchDomain,
        pub kind: WatchKind,
        pub path: String,
        pub detail: String,
        pub pid: i64,
        pub port: i64,
    }

    #[derive(Clone, Debug)]
    pub struct TempDir {
        pub path: String,
        pub cleanup: std::rc::Rc<()>,
    }

    #[derive(Clone, Debug)]
    pub struct TempFile {
        pub path: String,
        pub cleanup: std::rc::Rc<()>,
    }

    #[derive(Clone, Debug)]
    pub struct FileLock {
        pub path: String,
        pub cleanup: std::rc::Rc<()>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DataGroup {
        pub key: String,
        pub count: i64,
        pub sum: f64,
        pub mean: f64,
    }

    /// D-DATA-PLOT1=A: shared options for deterministic line renderers.
    #[derive(Clone, Debug, PartialEq)]
    pub struct DataLineOptions {
        pub title: String,
        pub x_label: String,
        pub y_label: String,
        pub markers: bool,
        pub reference: JetOutcome<f64, JetAbsent>,
        pub style: String,
        pub color: String,
        pub legend: String,
    }

    /// Typed streaming + invalid-data policy (edition 2027 surface).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum DataErrorKind {
        Decode,
        Limit,
        IO,
        Empty,
        InvalidArgument,
        NonFinite,
        Overflow,
        State,
        /// D-DATA-BRIDGE1: foreign/accelerator bridge unavailable or refused.
        Bridge,
    }
    impl std::fmt::Display for DataErrorKind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{self:?}")
        }
    }
    impl super::JetShow for DataErrorKind {
        fn jet_show(&self) -> String {
            format!("{self:?}")
        }
    }
    impl super::JetDebug for DataErrorKind {
        fn jet_debug(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetDisplay for DataErrorKind {
        fn jet_display(&self) -> String {
            format!("{self:?}")
        }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DataError {
        pub kind: DataErrorKind,
        pub operation: String,
        pub row: JetOutcome<i64, JetAbsent>,
        pub column: JetOutcome<i64, JetAbsent>,
        pub index: JetOutcome<i64, JetAbsent>,
        pub reason: String,
        pub cause: JetOutcome<EncodingError, JetAbsent>,
    }
    impl DataError {
        fn display_text(&self) -> String {
            let mut out = format!("{:?} {}", self.kind, self.operation);
            if let Ok(row) = self.row {
                out.push_str(&format!(", row {row}"));
            }
            if let Ok(column) = self.column {
                out.push_str(&format!(", column {column}"));
            }
            if let Ok(index) = self.index {
                out.push_str(&format!(", index {index}"));
            }
            out.push_str(&format!(": {}", self.reason));
            out
        }
    }
    impl std::fmt::Display for DataError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.display_text())
        }
    }
    impl super::JetShow for DataError {
        fn jet_show(&self) -> String {
            self.display_text()
        }
    }
    impl super::JetDebug for DataError {
        fn jet_debug(&self) -> String {
            self.display_text()
        }
    }
    impl super::JetDisplay for DataError {
        fn jet_display(&self) -> String {
            self.display_text()
        }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DataLimits {
        pub encoding: EncodingLimits,
        pub max_groups: i64,
        pub max_sort_rows: i64,
        pub max_join_rows: i64,
        pub max_output_rows: i64,
    }
    impl DataLimits {
        pub fn safe() -> Self {
            Self {
                encoding: EncodingLimits::safe(),
                max_groups: 100_000,
                max_sort_rows: 1_000_000,
                max_join_rows: 1_000_000,
                max_output_rows: 1_000_000,
            }
        }
    }
    #[derive(Clone, Debug, PartialEq)]
    pub struct DataPivotCell {
        pub row_key: String,
        pub column_key: String,
        pub count: i64,
        pub sum: f64,
        pub mean: f64,
    }
    pub enum DataStreamInner {
        CSV {
            reader: CSVReader,
            headers: Option<Vec<String>>,
        },
        JSON {
            reader: JSONReader,
            array_started: bool,
            array_done: bool,
        },
    }
    pub struct DataStream {
        pub(crate) inner: DataStreamInner,
        pub(crate) limits: DataLimits,
        pub(crate) terminal: Option<DataError>,
        pub(crate) eof: bool,
        pub(crate) row_index: i64,
    }

    /// D-DATAFRAME1=A: one typed column in a `Table`/`Series` schema.
    #[derive(Clone, Debug, PartialEq)]
    pub struct DataColumn {
        pub name: String,
        pub type_name: String,
    }

    /// D-DATA-STATUS1 / D-DATA-BRIDGE1: native or bridge step facts.
    /// Bridges must declare copy, ownership, trust, fallback, and replacement.
    #[derive(Clone, Debug, PartialEq)]
    pub struct DataStatus {
        pub step: String,
        pub path: String,
        pub copy: String,
        pub ownership: String,
        pub trust: String,
        pub fallback: String,
        pub replacement: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DataSummary {
        pub count: i64,
        pub sum: f64,
        pub mean: f64,
        pub min: f64,
        pub max: f64,
        pub median: f64,
        pub variance: f64,
        pub stddev: f64,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DataTable<T> {
        pub rows: Vec<T>,
        pub missing: i64,
        pub plan: Vec<String>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DataSeries<T> {
        pub values: Vec<T>,
        pub missing: i64,
    }

    #[derive(Clone)]
    pub enum DataLazyOperation<T> {
        Filter(std::sync::Arc<dyn Fn(T) -> bool>),
        SortBy(std::sync::Arc<dyn Fn(T) -> String>),
    }

    #[derive(Clone)]
    pub struct DataLazyFrame<T> {
        pub rows: Vec<T>,
        pub missing: i64,
        pub plan: Vec<String>,
        pub operations: Vec<DataLazyOperation<T>>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DataJoin<L, R> {
        pub left: L,
        pub right: R,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct LogField {
        pub key: String,
        pub value: String,
        pub kind: String,
        pub redacted: bool,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct LogSpan {
        pub id: i64,
        pub name: String,
    }

    #[derive(Clone, Debug)]
    pub struct Stopwatch {
        pub start: std::time::Instant,
    }

    // D-DET1: manual clocks are deterministic; system clocks
    // use monotonic elapsed time. Normal clones fork the timeline, while the
    // private observer lets ExpiringSecret follow its injected clock.
    #[derive(Debug)]
    pub enum ClockState {
        Manual(i64),
        System {
            started: std::time::Instant,
            offset_ms: i64,
        },
    }

    #[derive(Debug)]
    pub struct Clock {
        state: std::sync::Arc<std::sync::Mutex<ClockState>>,
    }

    #[derive(Clone, Debug)]
    pub struct ClockObserver {
        state: std::sync::Arc<std::sync::Mutex<ClockState>>,
    }

    fn clock_state_now(state: &ClockState) -> i64 {
        match state {
            ClockState::Manual(now) => *now,
            ClockState::System { started, offset_ms } => offset_ms.saturating_add(
                i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
            ),
        }
    }

    impl Clock {
        pub fn manual(now: i64) -> Self {
            Self {
                state: std::sync::Arc::new(std::sync::Mutex::new(ClockState::Manual(now))),
            }
        }

        pub fn system() -> Self {
            Self {
                state: std::sync::Arc::new(std::sync::Mutex::new(ClockState::System {
                    started: std::time::Instant::now(),
                    offset_ms: 0,
                })),
            }
        }

        pub fn now(&self) -> i64 {
            clock_state_now(&self.state.lock().unwrap_or_else(|e| e.into_inner()))
        }

        pub fn set(&mut self, now: i64) {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            match &mut *state {
                ClockState::Manual(current) => *current = now,
                ClockState::System { started, offset_ms } => {
                    let current = offset_ms.saturating_add(
                        i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
                    );
                    *started = std::time::Instant::now();
                    *offset_ms = now.max(current);
                }
            }
        }

        pub fn observer(&self) -> ClockObserver {
            ClockObserver {
                state: std::sync::Arc::clone(&self.state),
            }
        }
    }

    impl ClockObserver {
        pub fn now(&self) -> i64 {
            clock_state_now(&self.state.lock().unwrap_or_else(|e| e.into_inner()))
        }
    }

    impl Clone for Clock {
        fn clone(&self) -> Self {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            match &*state {
                ClockState::Manual(now) => Self::manual(*now),
                ClockState::System { .. } => Self {
                    state: std::sync::Arc::new(std::sync::Mutex::new(ClockState::System {
                        started: std::time::Instant::now(),
                        offset_ms: clock_state_now(&state),
                    })),
                },
            }
        }
    }

    impl PartialEq for Clock {
        fn eq(&self, other: &Self) -> bool {
            self.now() == other.now()
        }
    }

    // D-DET1: deterministic injected Rng capability. A SplitMix64 state stream
    // (std-only, no external crate — I6). The same seed yields the same draws on
    // every machine.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Rng {
        pub state: u64,
    }

    // Deterministic fake-data capability. `locale` is a closed
    // code: 0 = en, 1 = de. The Prelude owns all generation semantics.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Fake {
        pub state: u64,
        pub locale: u8,
    }

    // D-SOLVER-LIB1=A: explicit finite solver state. This first slice records
    // ordinary Bool constraints in insertion order; no hidden backtracking.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Solver {
        pub seed: i64,
        pub checked: i64,
        pub failures: i64,
    }

    // D-TIMERES1=A: a checked elapsed span stored as whole nanoseconds
    // (about 292 years). Whole-unit reads stay truncating.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Duration {
        pub ns: i64,
    }

    impl Duration {
        #[inline]
        pub fn as_millis(self) -> i64 {
            self.ns / 1_000_000
        }
    }

    // A duration renders as whole nanoseconds with its unit, the same text the
    // TIR evaluator and the JIT print, so `print("{1d}")` reads alike on every
    // tier (I9). Without this, AOT emitted `.jet_display()` on a type that had
    // no such method and rustc rejected the generated program (I2).
    impl super::JetDisplay for Duration {
        fn jet_display(&self) -> String {
            format!("{}ns", self.ns)
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum DurationUnit {
        Nanoseconds,
        Microseconds,
        Milliseconds,
        Seconds,
        Minutes,
        Hours,
    }

    impl DurationUnit {
        pub fn nanoseconds(self) -> i64 {
            match self {
                Self::Nanoseconds => 1,
                Self::Microseconds => 1_000,
                Self::Milliseconds => 1_000_000,
                Self::Seconds => 1_000_000_000,
                Self::Minutes => 60_000_000_000,
                Self::Hours => 3_600_000_000_000,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct RangeError {
        pub reason: String,
    }

    // D-INTBIG1: exact integer carrier (std-only limb arithmetic).
    // #1636: mirrors `CtBigInt` in `crates/jet-foundation/src/Numeric.rs`
    // limb-for-limb (sign-magnitude, little-endian base 10^9). This copy has
    // to stay separate, hand-mirrored text: AOT/JIT output is a standalone
    // Rust program that never links back into the compiler, so it can't
    // reference `jet_foundation` directly. `crates/jet-jit/src/enc_stream/mod.rs`
    // and `crates/jet-comptime/src/Comptime/EncodingLite.rs` both use
    // `CtBigInt` directly instead of keeping their own copy of this file.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetBigInt {
        negative: bool,
        limbs: Vec<u32>, // little-endian base 10^9
    }

    const BI_BASE: u64 = 1_000_000_000;

    impl JetBigInt {
        pub fn from_u64(mut value: u64) -> Self {
            if value == 0 {
                return Self::from_int(0);
            }
            let mut limbs = Vec::new();
            while value > 0 {
                limbs.push((value % BI_BASE) as u32);
                value /= BI_BASE;
            }
            Self {
                negative: false,
                limbs,
            }
        }

        pub fn from_int(n: i64) -> Self {
            if n == 0 {
                return JetBigInt {
                    negative: false,
                    limbs: vec![0],
                };
            }
            let negative = n < 0;
            let mut v = if negative {
                (n as i128).wrapping_neg() as u64
            } else {
                n as u64
            };
            let mut limbs = Vec::new();
            while v > 0 {
                limbs.push((v % BI_BASE) as u32);
                v /= BI_BASE;
            }
            JetBigInt { negative, limbs }
        }

        pub fn from_str(s: &str) -> Result<Self, String> {
            let t = s.trim();
            if t.is_empty() {
                return Err("empty exact Int string".to_string());
            }
            let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = t.strip_prefix('+') {
                (false, rest)
            } else {
                (false, t)
            };
            if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("invalid exact Int string `{s}`"));
            }
            let mut acc = JetBigInt {
                negative: false,
                limbs: vec![0],
            };
            for ch in body.chars() {
                let digit = ch.to_digit(10).unwrap() as u32;
                acc = acc.mul_small(10).add_small(digit);
            }
            acc.negative = negative && !(acc.limbs.len() == 1 && acc.limbs[0] == 0);
            Ok(acc)
        }

        fn normalize(mut self) -> Self {
            while self.limbs.len() > 1 && *self.limbs.last().unwrap() == 0 {
                self.limbs.pop();
            }
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                self.negative = false;
            }
            self
        }

        fn mul_small(&self, m: u32) -> Self {
            let mut carry = 0u64;
            let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
            for &limb in &self.limbs {
                let prod = limb as u64 * m as u64 + carry;
                limbs.push((prod % BI_BASE) as u32);
                carry = prod / BI_BASE;
            }
            if carry > 0 {
                limbs.push(carry as u32);
            }
            JetBigInt {
                negative: self.negative,
                limbs,
            }
            .normalize()
        }

        fn add_small(&self, n: u32) -> Self {
            self.add(&JetBigInt::from_int(n as i64))
        }

        pub fn add(&self, other: &JetBigInt) -> JetBigInt {
            if self.negative == other.negative {
                let mut carry = 0u64;
                let len = self.limbs.len().max(other.limbs.len());
                let mut limbs = Vec::with_capacity(len + 1);
                for i in 0..len {
                    let a = *self.limbs.get(i).unwrap_or(&0) as u64;
                    let b = *other.limbs.get(i).unwrap_or(&0) as u64;
                    let sum = a + b + carry;
                    limbs.push((sum % BI_BASE) as u32);
                    carry = sum / BI_BASE;
                }
                if carry > 0 {
                    limbs.push(carry as u32);
                }
                JetBigInt {
                    negative: self.negative,
                    limbs,
                }
                .normalize()
            } else {
                let cmp = self.cmp_abs(other);
                if cmp == 0 {
                    JetBigInt::from_int(0)
                } else if cmp > 0 {
                    self.sub_abs(other).with_sign(self.negative)
                } else {
                    other.sub_abs(self).with_sign(other.negative)
                }
            }
        }

        fn with_sign(self, negative: bool) -> Self {
            JetBigInt {
                negative,
                limbs: self.limbs,
            }
        }

        pub fn sub(&self, other: &JetBigInt) -> JetBigInt {
            let mut neg_other = other.clone();
            neg_other.negative = !neg_other.negative;
            self.add(&neg_other)
        }

        fn sub_abs(&self, other: &JetBigInt) -> JetBigInt {
            let mut borrow = 0i64;
            let len = self.limbs.len();
            let mut limbs = Vec::with_capacity(len);
            for i in 0..len {
                let a = self.limbs[i] as i64;
                let b = *other.limbs.get(i).unwrap_or(&0) as i64;
                let mut cur = a - b - borrow;
                if cur < 0 {
                    cur += BI_BASE as i64;
                    borrow = 1;
                } else {
                    borrow = 0;
                }
                limbs.push(cur as u32);
            }
            JetBigInt {
                negative: false,
                limbs,
            }
            .normalize()
        }

        fn cmp_abs(&self, other: &JetBigInt) -> i8 {
            match self.limbs.len().cmp(&other.limbs.len()) {
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => {
                    for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
                        match a.cmp(b) {
                            std::cmp::Ordering::Greater => return 1,
                            std::cmp::Ordering::Less => return -1,
                            std::cmp::Ordering::Equal => {}
                        }
                    }
                    0
                }
            }
        }

        pub fn mul(&self, other: &JetBigInt) -> JetBigInt {
            let mut out = JetBigInt::from_int(0);
            for (i, &limb) in other.limbs.iter().enumerate() {
                if limb == 0 {
                    continue;
                }
                let mut part = self.mul_small(limb);
                for _ in 0..i {
                    part = part.mul_small(BI_BASE as u32);
                }
                out = out.add(&part);
            }
            JetBigInt {
                negative: self.negative != other.negative,
                limbs: out.limbs,
            }
            .normalize()
        }

        pub fn neg(&self) -> JetBigInt {
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                self.clone()
            } else {
                JetBigInt {
                    negative: !self.negative,
                    limbs: self.limbs.clone(),
                }
            }
        }

        fn div_rem_small(&self, divisor: u32) -> (JetBigInt, u32) {
            let divisor = u64::from(divisor);
            let mut remainder = 0u64;
            let mut limbs = vec![0u32; self.limbs.len()];
            for index in (0..self.limbs.len()).rev() {
                let current = remainder * BI_BASE + u64::from(self.limbs[index]);
                limbs[index] = (current / divisor) as u32;
                remainder = current % divisor;
            }
            (
                JetBigInt {
                    negative: false,
                    limbs,
                }
                .normalize(),
                remainder as u32,
            )
        }

        fn bit_width(&self) -> usize {
            let mut value = self.abs();
            let mut width = 0usize;
            while !value.is_zero() {
                let (next, _) = value.div_rem_small(2);
                value = next;
                width += 1;
            }
            width
        }

        fn unsigned_bits(&self, width: usize) -> Vec<bool> {
            let mut value = self.abs();
            let mut bits = Vec::with_capacity(width);
            for _ in 0..width {
                let (next, remainder) = value.div_rem_small(2);
                bits.push(remainder != 0);
                value = next;
            }
            bits
        }

        fn from_unsigned_bits(bits: &[bool]) -> JetBigInt {
            let mut value = JetBigInt::from_int(0);
            for bit in bits.iter().rev() {
                value = value.mul_small(2);
                if *bit {
                    value = value.add_small(1);
                }
            }
            value
        }

        fn twos_complement(&self, width: usize) -> Vec<bool> {
            let mut bits = self.unsigned_bits(width);
            if self.negative {
                for bit in &mut bits {
                    *bit = !*bit;
                }
                let mut carry = true;
                for bit in &mut bits {
                    if !carry {
                        break;
                    }
                    if *bit {
                        *bit = false;
                    } else {
                        *bit = true;
                        carry = false;
                    }
                }
            }
            bits
        }

        fn from_twos_complement(mut bits: Vec<bool>) -> JetBigInt {
            let negative = bits.last().copied().unwrap_or(false);
            if !negative {
                return Self::from_unsigned_bits(&bits);
            }
            for bit in &mut bits {
                *bit = !*bit;
            }
            let mut carry = true;
            for bit in &mut bits {
                if !carry {
                    break;
                }
                if *bit {
                    *bit = false;
                } else {
                    *bit = true;
                    carry = false;
                }
            }
            Self::from_unsigned_bits(&bits).neg()
        }

        fn bitwise(&self, other: &JetBigInt, op: impl Fn(bool, bool) -> bool) -> JetBigInt {
            let width = self.bit_width().max(other.bit_width()).saturating_add(1);
            let left = self.twos_complement(width);
            let right = other.twos_complement(width);
            let bits = left
                .into_iter()
                .zip(right)
                .map(|(left, right)| op(left, right))
                .collect();
            Self::from_twos_complement(bits)
        }

        pub fn bit_and(&self, other: &JetBigInt) -> JetBigInt {
            self.bitwise(other, |left, right| left & right)
        }

        pub fn bit_or(&self, other: &JetBigInt) -> JetBigInt {
            self.bitwise(other, |left, right| left | right)
        }

        pub fn bit_xor(&self, other: &JetBigInt) -> JetBigInt {
            self.bitwise(other, |left, right| left ^ right)
        }

        pub fn bit_count(&self, width: u32, method: &str) -> Option<i64> {
            let width = usize::try_from(width).ok()?;
            if width == 0 {
                return None;
            }
            let bits = self.twos_complement(width);
            let ones = bits.iter().filter(|bit| **bit).count();
            let count = match method {
                "count_ones" => ones,
                "count_zeros" => width - ones,
                "leading_zeros" => bits.iter().rev().take_while(|bit| !**bit).count(),
                "trailing_zeros" => bits.iter().take_while(|bit| !**bit).count(),
                _ => return None,
            };
            i64::try_from(count).ok()
        }

        pub fn checked_widen(&self, target_f32: bool) -> Option<f64> {
            let precision = if target_f32 { 24 } else { 53 };
            let width = self.bit_width();
            let mut trailing = 0usize;
            let mut value = self.abs();
            while !value.is_zero() {
                let (next, remainder) = value.div_rem_small(2);
                if remainder != 0 {
                    break;
                }
                trailing += 1;
                value = next;
            }
            if width > precision && trailing < width - precision {
                return None;
            }
            let value = self.to_string_rep().parse::<f64>().ok()?;
            if !value.is_finite() {
                return None;
            }
            if target_f32 {
                let value = value as f32;
                value.is_finite().then_some(value as f64)
            } else {
                Some(value)
            }
        }

        fn shift_count(&self) -> Option<usize> {
            let count = self.try_i64()?;
            (count >= 0).then_some(count as usize)
        }

        pub fn shl(&self, count: &JetBigInt) -> Option<JetBigInt> {
            let count = count.shift_count()?;
            let mut value = self.clone();
            for _ in 0..count {
                value = value.mul_small(2);
            }
            Some(value)
        }

        pub fn shr(&self, count: &JetBigInt) -> Option<JetBigInt> {
            let count = count.shift_count()?;
            let mut value = self.clone();
            for _ in 0..count {
                let (quotient, remainder) = value.abs().div_rem_small(2);
                value = if self.negative && remainder != 0 {
                    quotient.add_small(1).neg()
                } else {
                    quotient.with_sign(self.negative)
                };
            }
            Some(value)
        }

        pub fn is_even(&self) -> bool {
            self.div_rem_small(2).1 == 0
        }

        pub fn is_odd(&self) -> bool {
            !self.is_even()
        }

        pub fn digits(&self) -> i64 {
            let digits = self.to_string_rep().trim_start_matches('-').len();
            i64::try_from(digits).unwrap_or(i64::MAX)
        }

        pub fn leading_ones(&self) -> i64 {
            let width = 64usize.max(self.bit_width().saturating_add(1));
            let count = self
                .twos_complement(width)
                .iter()
                .rev()
                .take_while(|bit| **bit)
                .count();
            i64::try_from(count).unwrap_or(i64::MAX)
        }

        pub fn trailing_ones(&self) -> i64 {
            let width = 64usize.max(self.bit_width().saturating_add(1));
            let count = self
                .twos_complement(width)
                .iter()
                .take_while(|bit| **bit)
                .count();
            i64::try_from(count).unwrap_or(i64::MAX)
        }

        pub fn isqrt(&self) -> Option<JetBigInt> {
            if self.negative {
                return None;
            }
            if self.is_zero() {
                return Some(self.clone());
            }
            let one = JetBigInt::from_int(1);
            let two = JetBigInt::from_int(2);
            let mut root = one.clone();
            for _ in 0..self.bit_width().saturating_add(1) / 2 {
                root = root.mul_small(2);
            }
            loop {
                let quotient = self.div_rem(&root)?.0;
                let next = root.add(&quotient).div_rem(&two)?.0;
                if next.compare(&root) != std::cmp::Ordering::Less {
                    break;
                }
                root = next;
            }
            while root.mul(&root).compare(self) == std::cmp::Ordering::Greater {
                root = root.sub(&one);
            }
            loop {
                let next = root.add(&one);
                if next.mul(&next).compare(self) == std::cmp::Ordering::Greater {
                    break;
                }
                root = next;
            }
            Some(root)
        }

        pub fn pow(&self, exponent: &JetBigInt) -> Option<JetBigInt> {
            if exponent.negative {
                return None;
            }
            let mut exponent = exponent.clone();
            let mut base = self.clone();
            let mut result = JetBigInt::from_int(1);
            while !exponent.is_zero() {
                let (next, bit) = exponent.div_rem_small(2);
                if bit != 0 {
                    result = result.mul(&base);
                }
                exponent = next;
                if !exponent.is_zero() {
                    base = base.mul(&base);
                }
            }
            Some(result)
        }

        pub fn gcd(left: &JetBigInt, right: &JetBigInt) -> JetBigInt {
            let mut a = left.abs();
            let mut b = right.abs();
            while !b.is_zero() {
                let (_, remainder) = a
                    .div_rem(&b)
                    .expect("gcd divisor is nonzero");
                a = b;
                b = remainder.abs();
            }
            a
        }

        pub fn lcm(left: &JetBigInt, right: &JetBigInt) -> JetBigInt {
            if left.is_zero() || right.is_zero() {
                return JetBigInt::from_int(0);
            }
            let divisor = Self::gcd(left, right);
            let quotient = left
                .abs()
                .div_rem(&divisor)
                .expect("lcm gcd is nonzero")
                .0;
            quotient.mul(&right.abs())
        }

        pub fn binomial(n: &JetBigInt, k: &JetBigInt) -> Option<JetBigInt> {
            if n.negative || k.negative || k.compare(n) == std::cmp::Ordering::Greater {
                return None;
            }
            let other = n.sub(k);
            let limit = if k.compare(&other) == std::cmp::Ordering::Greater {
                other
            } else {
                k.clone()
            };
            let one = JetBigInt::from_int(1);
            let mut index = one.clone();
            let mut result = one.clone();
            while index.compare(&limit) != std::cmp::Ordering::Greater {
                let numerator = n.sub(&limit).add(&index);
                result = result
                    .mul(&numerator)
                    .div_rem(&index)?
                    .0;
                index = index.add(&one);
            }
            Some(result)
        }

        pub fn compare(&self, other: &JetBigInt) -> std::cmp::Ordering {
            match (self.negative, other.negative) {
                (false, true) => std::cmp::Ordering::Greater,
                (true, false) => std::cmp::Ordering::Less,
                (false, false) => match self.cmp_abs(other) {
                    1 => std::cmp::Ordering::Greater,
                    -1 => std::cmp::Ordering::Less,
                    _ => std::cmp::Ordering::Equal,
                },
                (true, true) => match self.cmp_abs(other) {
                    1 => std::cmp::Ordering::Less,
                    -1 => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                },
            }
        }

        pub fn is_zero(&self) -> bool {
            self.limbs.len() == 1 && self.limbs[0] == 0
        }

        /// Return the value when it fits in a signed machine word.
        pub fn try_i64(&self) -> Option<i64> {
            let mut value = 0u128;
            for &limb in self.limbs.iter().rev() {
                value = value.checked_mul(BI_BASE as u128)?;
                value = value.checked_add(limb as u128)?;
            }
            let signed = if self.negative {
                -i128::try_from(value).ok()?
            } else {
                i128::try_from(value).ok()?
            };
            i64::try_from(signed).ok()
        }

        pub fn try_i128(&self) -> Option<i128> {
            let mut value = 0u128;
            for &limb in self.limbs.iter().rev() {
                value = value.checked_mul(BI_BASE as u128)?;
                value = value.checked_add(limb as u128)?;
            }
            let magnitude = i128::try_from(value).ok()?;
            Some(if self.negative { -magnitude } else { magnitude })
        }

        /// Truncating quotient and remainder. The remainder carries the
        /// dividend sign, matching Rust's integer rules.
        pub fn div_rem(&self, other: &JetBigInt) -> Option<(JetBigInt, JetBigInt)> {
            if other.is_zero() {
                return None;
            }
            let divisor = other.abs();
            let dividend = self.abs();
            if dividend.cmp_abs(&divisor) < 0 {
                return Some((JetBigInt::from_int(0), self.clone()));
            }

            let mut quotient = vec![0u32; dividend.limbs.len()];
            let mut remainder = JetBigInt::from_int(0);
            for index in (0..dividend.limbs.len()).rev() {
                remainder.limbs.insert(0, dividend.limbs[index]);
                remainder = remainder.normalize();
                let mut low = 0u32;
                let mut high = (BI_BASE - 1) as u32;
                while low < high {
                    let middle = low + (high - low) / 2 + 1;
                    if divisor.mul_small(middle).cmp_abs(&remainder) <= 0 {
                        low = middle;
                    } else {
                        high = middle - 1;
                    }
                }
                quotient[index] = low;
                if low != 0 {
                    remainder = remainder.sub_abs(&divisor.mul_small(low));
                }
            }
            let quotient = JetBigInt {
                negative: self.negative != other.negative,
                limbs: quotient,
            }
            .normalize();
            remainder.negative = self.negative && !remainder.is_zero();
            Some((quotient, remainder.normalize()))
        }

        pub fn abs(&self) -> JetBigInt {
            JetBigInt {
                negative: false,
                limbs: self.limbs.clone(),
            }
        }

        pub fn to_string_rep(&self) -> String {
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                return "0".to_string();
            }
            let mut s = String::new();
            let top = *self.limbs.last().unwrap();
            s.push_str(&top.to_string());
            for &limb in self.limbs.iter().rev().skip(1) {
                s.push_str(&format!("{:09}", limb));
            }
            if self.negative {
                format!("-{s}")
            } else {
                s
            }
        }
    }

    impl super::JetShow for JetBigInt {
        fn jet_show(&self) -> String {
            self.to_string_rep()
        }
    }

    // D-INTBIG1: default `Int` is one packed word at the language boundary.
    // Values in the signed 63-bit payload stay unboxed. Other values point at
    // this std-only arena and continue through the same limb implementation.
    // The representation is deliberately private to the generated Prelude:
    // user code and every execution tier still see only `Int`.
    const JET_INT_SMALL_MIN: i64 = -(1i64 << 62);
    const JET_INT_SMALL_MAX: i64 = (1i64 << 62) - 1;
    const JET_INT_BIG_TAG: i64 = i64::MIN;
    static JET_INT_BIG_VALUES: std::sync::OnceLock<std::sync::Mutex<Vec<JetBigInt>>> =
        std::sync::OnceLock::new();

    fn jet_int_big_values() -> &'static std::sync::Mutex<Vec<JetBigInt>> {
        JET_INT_BIG_VALUES.get_or_init(|| std::sync::Mutex::new(Vec::new()))
    }

    fn jet_int_is_tagged(value: i64) -> bool {
        value < JET_INT_SMALL_MIN
    }

    fn jet_int_big_value(value: i64) -> Option<JetBigInt> {
        if !jet_int_is_tagged(value) {
            return None;
        }
        let id = value.wrapping_sub(JET_INT_BIG_TAG) as usize;
        let values = jet_int_big_values()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        values.get(id).cloned()
    }

    fn jet_int_value(value: i64) -> JetBigInt {
        jet_int_big_value(value).unwrap_or_else(|| JetBigInt::from_int(value))
    }

    fn jet_int_pack(value: JetBigInt) -> i64 {
        if let Some(small) = value.try_i64() {
            if (JET_INT_SMALL_MIN..=JET_INT_SMALL_MAX).contains(&small) {
                return small;
            }
        }
        let mut values = jet_int_big_values()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = values.len();
        values.push(value);
        JET_INT_BIG_TAG.wrapping_add(id as i64)
    }

    pub fn jet_int_from_i64(value: i64) -> i64 {
        if (JET_INT_SMALL_MIN..=JET_INT_SMALL_MAX).contains(&value) {
            value
        } else {
            jet_int_pack(JetBigInt::from_int(value))
        }
    }

    pub fn jet_int_from_u64(value: u64) -> i64 {
        if value <= JET_INT_SMALL_MAX as u64 {
            value as i64
        } else {
            jet_int_pack(JetBigInt::from_u64(value))
        }
    }

    pub fn jet_int_from_str(value: &str) -> Result<i64, String> {
        Ok(jet_int_pack(JetBigInt::from_str(value)?))
    }

    pub fn jet_int_parse(value: &str) -> Result<i64, String> {
        jet_int_from_str(value.trim())
            .map_err(|_| format!("cannot parse `{value}` as an integer"))
    }

    pub fn jet_int_to_i64(value: i64) -> Option<i64> {
        if !jet_int_is_tagged(value) {
            Some(value)
        } else {
            jet_int_big_value(value)?.try_i64()
        }
    }

    pub fn jet_int_to_i128(value: i64) -> Option<i128> {
        if !jet_int_is_tagged(value) {
            Some(i128::from(value))
        } else {
            jet_int_big_value(value)?.try_i128()
        }
    }

    pub fn jet_int_is_zero(value: i64) -> bool {
        if !jet_int_is_tagged(value) {
            value == 0
        } else {
            jet_int_big_value(value).is_some_and(|value| value.is_zero())
        }
    }

    pub fn jet_int_is_negative(value: i64) -> bool {
        if !jet_int_is_tagged(value) {
            value < 0
        } else {
            jet_int_big_value(value).is_some_and(|value| value.negative)
        }
    }

    pub fn jet_int_to_string(value: i64) -> String {
        if !jet_int_is_tagged(value) {
            value.to_string()
        } else {
            jet_int_big_value(value)
                .map(|value| value.to_string_rep())
                .unwrap_or_else(|| value.to_string())
        }
    }

    pub fn jet_int_to_f64(value: i64) -> f64 {
        jet_int_to_string(value).parse::<f64>().unwrap_or_else(|_| {
            if jet_int_is_negative(value) {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        })
    }

    pub fn jet_int_checked_widen(value: i64, target_f32: bool, file: &str, line: u32) -> f64 {
        jet_int_value(value)
            .checked_widen(target_f32)
            .unwrap_or_else(|| crate::jet_panic(file, line, crate::JET_NUMERIC_WIDEN_TRAP))
    }

    pub fn jet_int_bit_count(value: i64, width: u32, method: &str) -> i64 {
        jet_int_value(value).bit_count(width, method).unwrap_or(0)
    }

    pub fn jet_int_compare(left: i64, right: i64) -> i64 {
        if !jet_int_is_tagged(left) && !jet_int_is_tagged(right) {
            return match left.cmp(&right) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
        }
        match jet_int_value(left).compare(&jet_int_value(right)) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    pub fn jet_int_add(left: i64, right: i64) -> i64 {
        if !jet_int_is_tagged(left) && !jet_int_is_tagged(right) {
            if let Some(value) = left.checked_add(right) {
                if (JET_INT_SMALL_MIN..=JET_INT_SMALL_MAX).contains(&value) {
                    return value;
                }
            }
        }
        jet_int_pack(jet_int_value(left).add(&jet_int_value(right)))
    }

    pub fn jet_int_sub(left: i64, right: i64) -> i64 {
        if !jet_int_is_tagged(left) && !jet_int_is_tagged(right) {
            if let Some(value) = left.checked_sub(right) {
                if (JET_INT_SMALL_MIN..=JET_INT_SMALL_MAX).contains(&value) {
                    return value;
                }
            }
        }
        jet_int_pack(jet_int_value(left).sub(&jet_int_value(right)))
    }

    pub fn jet_int_mul(left: i64, right: i64) -> i64 {
        if !jet_int_is_tagged(left) && !jet_int_is_tagged(right) {
            if let Some(value) = left.checked_mul(right) {
                if (JET_INT_SMALL_MIN..=JET_INT_SMALL_MAX).contains(&value) {
                    return value;
                }
            }
        }
        jet_int_pack(jet_int_value(left).mul(&jet_int_value(right)))
    }

    pub fn jet_int_bit_and(left: i64, right: i64) -> i64 {
        jet_int_pack(jet_int_value(left).bit_and(&jet_int_value(right)))
    }

    pub fn jet_int_bit_or(left: i64, right: i64) -> i64 {
        jet_int_pack(jet_int_value(left).bit_or(&jet_int_value(right)))
    }

    pub fn jet_int_bit_xor(left: i64, right: i64) -> i64 {
        jet_int_pack(jet_int_value(left).bit_xor(&jet_int_value(right)))
    }

    pub fn jet_int_neg(value: i64) -> i64 {
        if !jet_int_is_tagged(value) {
            if let Some(value) = value.checked_neg() {
                if (JET_INT_SMALL_MIN..=JET_INT_SMALL_MAX).contains(&value) {
                    return value;
                }
            }
        }
        jet_int_pack(jet_int_value(value).neg())
    }

    pub fn jet_int_abs(value: i64) -> i64 {
        if jet_int_is_negative(value) {
            jet_int_neg(value)
        } else {
            value
        }
    }

    pub fn jet_int_try_from(value: i64, kind: i64) -> Option<i128> {
        let value = jet_int_to_i128(value)?;
        let (lo, hi) = match kind {
            0 => (i8::MIN as i128, i8::MAX as i128),
            1 => (i16::MIN as i128, i16::MAX as i128),
            2 => (i32::MIN as i128, i32::MAX as i128),
            3 => (i64::MIN as i128, i64::MAX as i128),
            4 => (u8::MIN as i128, u8::MAX as i128),
            5 => (u16::MIN as i128, u16::MAX as i128),
            6 => (u32::MIN as i128, u32::MAX as i128),
            7 => (u64::MIN as i128, u64::MAX as i128),
            _ => return None,
        };
        (lo..=hi).contains(&value).then_some(value)
    }

    pub fn jet_int_not(value: i64) -> i64 {
        // `!x` is `-x - 1` for an exact signed integer. Reuse the same
        // packed arithmetic helpers so the small and spilled representations
        // stay one semantic path.
        jet_int_sub(jet_int_neg(value), jet_int_from_i64(1))
    }

    pub fn jet_int_shl(value: i64, count: i64, file: &str, line: u32) -> i64 {
        jet_int_value(value)
            .shl(&jet_int_value(count))
            .map(jet_int_pack)
            .unwrap_or_else(|| crate::jet_arithmetic_stop(file, line, "invalid shift count"))
    }

    pub fn jet_int_shr(value: i64, count: i64, file: &str, line: u32) -> i64 {
        jet_int_value(value)
            .shr(&jet_int_value(count))
            .map(jet_int_pack)
            .unwrap_or_else(|| crate::jet_arithmetic_stop(file, line, "invalid shift count"))
    }

    fn jet_int_div_rem(value: i64, divisor: i64, file: &str, line: u32) -> (i64, i64) {
        // D-FLOORDIV1: plain `Int` `/`, `%` and `/%` stop with THE
        // canonical arithmetic wording, never a second copy typed here. This site
        // carried the invented "division by zero" while `Core.rs`'s fixed-width
        // remainder, the TIR evaluator and the Cranelift host all raised
        // `JET_ARITHMETIC_DIVIDE_ZERO`, so one operator reported two sentences and
        // only the plain-`Int` AOT tier drifted (the same shape `jet-jit`'s
        // `Numeric.rs` records for the E3001/E3010 split it already closed).
        if jet_int_is_zero(divisor) {
            crate::jet_arithmetic_stop(file, line, crate::JET_ARITHMETIC_DIVIDE_ZERO);
        }
        if !jet_int_is_tagged(value) && !jet_int_is_tagged(divisor) {
            if let (Some(quotient), Some(remainder)) =
                (value.checked_div(divisor), value.checked_rem(divisor))
            {
                return (quotient, remainder);
            }
        }
        let (quotient, remainder) = jet_int_value(value)
            .div_rem(&jet_int_value(divisor))
            .expect("checked division by zero");
        (jet_int_pack(quotient), jet_int_pack(remainder))
    }

    pub fn jet_int_rem(value: i64, divisor: i64, file: &str, line: u32) -> i64 {
        jet_int_div_rem(value, divisor, file, line).1
    }

    pub fn jet_int_div(value: i64, divisor: i64, file: &str, line: u32) -> i64 {
        jet_int_div_rem(value, divisor, file, line).0
    }

    pub fn jet_int_floor_div(value: i64, divisor: i64, file: &str, line: u32) -> i64 {
        let (quotient, remainder) = jet_int_div_rem(value, divisor, file, line);
        if !jet_int_is_zero(remainder) && jet_int_is_negative(value) != jet_int_is_negative(divisor) {
            jet_int_sub(quotient, jet_int_from_i64(1))
        } else {
            quotient
        }
    }

    pub fn jet_int_mod(value: i64, divisor: i64, file: &str, line: u32) -> i64 {
        let (quotient, remainder) = jet_int_div_rem(value, divisor, file, line);
        if !jet_int_is_zero(remainder) && jet_int_is_negative(value) != jet_int_is_negative(divisor) {
            jet_int_add(remainder, divisor)
        } else {
            let _ = quotient;
            remainder
        }
    }

    pub fn jet_int_pow(value: i64, exponent: i64, file: &str, line: u32) -> i64 {
        let base = jet_int_value(value);
        let exponent_value = jet_int_value(exponent);
        if exponent_value.negative {
            crate::jet_arithmetic_stop(file, line, "negative default Int exponent");
        }
        jet_int_pack(
            base.pow(&exponent_value)
                .expect("checked default Int exponent is nonnegative"),
        )
    }

    pub fn jet_int_factorial(value: i64) -> Option<i64> {
        if jet_int_is_negative(value) {
            return None;
        }
        let mut current = jet_int_from_i64(2);
        let mut result = jet_int_from_i64(1);
        while jet_int_compare(current, value) <= 0 {
            result = jet_int_mul(result, current);
            current = jet_int_add(current, jet_int_from_i64(1));
        }
        Some(result)
    }

    pub fn jet_int_is_even(value: i64) -> bool {
        jet_int_value(value).is_even()
    }

    pub fn jet_int_is_odd(value: i64) -> bool {
        jet_int_value(value).is_odd()
    }

    pub fn jet_int_isqrt(value: i64) -> Option<i64> {
        jet_int_value(value).isqrt().map(jet_int_pack)
    }

    pub fn jet_int_binomial(n: i64, k: i64) -> Option<i64> {
        let n = jet_int_value(n);
        let k = jet_int_value(k);
        JetBigInt::binomial(&n, &k).map(jet_int_pack)
    }

    pub fn jet_int_digits(value: i64) -> i64 {
        jet_int_value(value).digits()
    }

    pub fn jet_int_leading_ones(value: i64) -> i64 {
        jet_int_value(value).leading_ones()
    }

    pub fn jet_int_trailing_ones(value: i64) -> i64 {
        jet_int_value(value).trailing_ones()
    }

    pub fn jet_int_checked_abs(value: i64) -> Option<i64> {
        Some(jet_int_abs(value))
    }

    pub fn jet_int_checked_neg(value: i64) -> Option<i64> {
        Some(jet_int_neg(value))
    }

    pub fn jet_int_checked_add(left: i64, right: i64) -> Option<i64> {
        Some(jet_int_add(left, right))
    }

    pub fn jet_int_checked_sub(left: i64, right: i64) -> Option<i64> {
        Some(jet_int_sub(left, right))
    }

    pub fn jet_int_checked_mul(left: i64, right: i64) -> Option<i64> {
        Some(jet_int_mul(left, right))
    }

    pub fn jet_int_checked_div(left: i64, right: i64, file: &str, line: u32) -> Option<i64> {
        if jet_int_is_zero(right) {
            return None;
        }
        Some(jet_int_div(left, right, file, line))
    }

    pub fn jet_int_checked_rem(left: i64, right: i64, file: &str, line: u32) -> Option<i64> {
        if jet_int_is_zero(right) {
            return None;
        }
        Some(jet_int_rem(left, right, file, line))
    }

    pub fn jet_int_checked_pow(left: i64, right: i64) -> Option<i64> {
        let left = jet_int_value(left);
        let right = jet_int_value(right);
        left.pow(&right).map(jet_int_pack)
    }

    pub fn jet_int_saturating_add(left: i64, right: i64) -> i64 {
        jet_int_add(left, right)
    }

    pub fn jet_int_saturating_sub(left: i64, right: i64) -> i64 {
        jet_int_sub(left, right)
    }

    pub fn jet_int_saturating_mul(left: i64, right: i64) -> i64 {
        jet_int_mul(left, right)
    }

    pub fn jet_int_wrapping_add(left: i64, right: i64) -> i64 {
        jet_int_add(left, right)
    }

    pub fn jet_int_wrapping_sub(left: i64, right: i64) -> i64 {
        jet_int_sub(left, right)
    }

    pub fn jet_int_wrapping_mul(left: i64, right: i64) -> i64 {
        jet_int_mul(left, right)
    }

    pub fn jet_int_int_pow(left: i64, right: i64) -> i64 {
        jet_int_checked_pow(left, right).unwrap_or_else(|| jet_int_from_i64(0))
    }

    pub fn jet_int_gcd(left: i64, right: i64) -> i64 {
        let left = jet_int_value(left);
        let right = jet_int_value(right);
        jet_int_pack(JetBigInt::gcd(&left, &right))
    }

    pub fn jet_int_lcm(left: i64, right: i64) -> i64 {
        let left = jet_int_value(left);
        let right = jet_int_value(right);
        jet_int_pack(JetBigInt::lcm(&left, &right))
    }

    pub fn jet_int_div_mod(value: i64, divisor: i64, file: &str, line: u32) -> (i64, i64) {
        let (quotient, remainder) = jet_int_div_rem(value, divisor, file, line);
        if !jet_int_is_zero(remainder)
            && jet_int_is_negative(value) != jet_int_is_negative(divisor)
        {
            (
                jet_int_sub(quotient, jet_int_from_i64(1)),
                jet_int_add(remainder, divisor),
            )
        } else {
            (quotient, remainder)
        }
    }

    pub fn jet_int_div_rem_pair(value: i64, divisor: i64, file: &str, line: u32) -> (i64, i64) {
        jet_int_div_rem(value, divisor, file, line)
    }

    // D-NUMTYPE1=A: an exact ratio of two whole numbers, always reduced, with
    // the sign carried on the top. A zero bottom has no value, so building one
    // answers nothing rather than a wrong number.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetFraction {
        pub numerator: i64,
        pub denominator: i64,
    }

    impl JetFraction {
        pub fn new(numerator: i64, denominator: i64) -> Option<Self> {
            if denominator == 0 {
                return None;
            }
            let (mut n, mut d) = (numerator, denominator);
            if d < 0 {
                n = n.checked_neg()?;
                d = d.checked_neg()?;
            }
            let mut a = n.checked_abs()?;
            let mut b = d;
            while b != 0 {
                let r = a % b;
                a = b;
                b = r;
            }
            let divisor = if a == 0 { 1 } else { a };
            Some(Self {
                numerator: n / divisor,
                denominator: d / divisor,
            })
        }

        pub fn add(&self, other: &Self) -> Option<Self> {
            let left = self.numerator.checked_mul(other.denominator)?;
            let right = other.numerator.checked_mul(self.denominator)?;
            Self::new(
                left.checked_add(right)?,
                self.denominator.checked_mul(other.denominator)?,
            )
        }

        pub fn sub(&self, other: &Self) -> Option<Self> {
            let left = self.numerator.checked_mul(other.denominator)?;
            let right = other.numerator.checked_mul(self.denominator)?;
            Self::new(
                left.checked_sub(right)?,
                self.denominator.checked_mul(other.denominator)?,
            )
        }

        pub fn mul(&self, other: &Self) -> Option<Self> {
            Self::new(
                self.numerator.checked_mul(other.numerator)?,
                self.denominator.checked_mul(other.denominator)?,
            )
        }

        pub fn div(&self, other: &Self) -> Option<Self> {
            Self::new(
                self.numerator.checked_mul(other.denominator)?,
                self.denominator.checked_mul(other.numerator)?,
            )
        }

        pub fn to_float(&self) -> f64 {
            self.numerator as f64 / self.denominator as f64
        }

        pub fn is_zero(&self) -> bool {
            self.numerator == 0
        }

        pub fn to_string_rep(&self) -> String {
            if let Some(decimal) = finite_fraction_decimal(self.numerator, self.denominator) {
                return decimal;
            }
            format!("{}/{}", self.numerator, self.denominator)
        }
    }

    /// Render a finite reduced ratio without passing through binary floating
    /// point. A denominator with any factor beyond 2 and 5 stays a fraction.
    fn finite_fraction_decimal(numerator: i64, denominator: i64) -> Option<String> {
        if denominator <= 0 {
            return None;
        }
        if numerator == 0 {
            return Some("0".to_string());
        }

        let mut factors = denominator as u64;
        let mut twos = 0u32;
        while factors % 2 == 0 {
            factors /= 2;
            twos += 1;
        }
        let mut fives = 0u32;
        while factors % 5 == 0 {
            factors /= 5;
            fives += 1;
        }
        if factors != 1 {
            return None;
        }

        let scale = twos.max(fives);
        let denominator = denominator as u128;
        let magnitude = numerator.unsigned_abs() as u128;
        let mut remainder = magnitude % denominator;
        let whole = magnitude / denominator;
        let sign = if numerator < 0 { "-" } else { "" };
        if scale == 0 {
            return Some(format!("{sign}{whole}"));
        }

        let mut fraction = String::with_capacity(scale as usize);
        for _ in 0..scale {
            remainder *= 10;
            fraction.push(char::from(b'0' + (remainder / denominator) as u8));
            remainder %= denominator;
        }
        while fraction.ends_with('0') {
            fraction.pop();
        }
        if fraction.is_empty() {
            Some(format!("{sign}{whole}"))
        } else {
            Some(format!("{sign}{whole}.{fraction}"))
        }
    }

    impl super::JetShow for JetFraction {
        fn jet_show(&self) -> String {
            self.to_string_rep()
        }
    }

    // D-DECIMAL1: exact base-10 decimal (scaled integer + scale).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetDecimal {
        negative: bool,
        digits: Vec<u8>, // big-endian mantissa digits 0-9, no dot
        scale: u32,
    }

    impl JetDecimal {
        pub fn from_str(s: &str) -> Result<Self, String> {
            let t = s.trim();
            if t.is_empty() {
                return Err("empty Decimal string".to_string());
            }
            let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = t.strip_prefix('+') {
                (false, rest)
            } else {
                (false, t)
            };
            let parts: Vec<&str> = body.split('.').collect();
            if parts.len() > 2 {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            let (int_part, frac_part) = (parts[0], parts.get(1).copied().unwrap_or(""));
            if int_part.is_empty() && frac_part.is_empty() {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            if !int_part.chars().all(|c| c.is_ascii_digit())
                || !frac_part.chars().all(|c| c.is_ascii_digit())
            {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            let mut digits: Vec<u8> = int_part
                .chars()
                .chain(frac_part.chars())
                .map(|c| (c as u8 - b'0'))
                .collect();
            while digits.len() > 1 && digits.first() == Some(&0) {
                digits.remove(0);
            }
            if digits.is_empty() {
                digits.push(0);
            }
            let scale = frac_part.len() as u32;
            Ok(JetDecimal {
                negative,
                digits,
                scale,
            }
            .normalize())
        }

        /// Project one JSON number token directly into base-10 digits. This
        /// preserves written scale and exponent without crossing binary64.
        pub fn from_json_number(s: &str) -> Result<Self, String> {
            let (negative, digits, scale) =
                crate::jet_json_number::json_decimal_lexeme(s)?;
            Ok(JetDecimal {
                negative,
                digits,
                scale,
            })
        }

        fn normalize(mut self) -> Self {
            // Trailing fractional zeros are insignificant; drop them with scale.
            // Popping digits without `scale -= 1` silently shifts the radix point
            // (`"10.50"` → 1.05) and breaks D-DECIMAL1 / R12 vs comptime.
            while self.scale > 0 && self.digits.len() > 1 && self.digits.last() == Some(&0) {
                self.digits.pop();
                self.scale -= 1;
            }
            if self.digits == [0] {
                self.negative = false;
                self.scale = 0;
            }
            self
        }

        fn align_scales(a: &JetDecimal, b: &JetDecimal) -> (JetDecimal, JetDecimal) {
            let scale = a.scale.max(b.scale);
            let mut left = a.clone();
            let mut right = b.clone();
            while left.scale < scale {
                left.digits.push(0);
                left.scale += 1;
            }
            while right.scale < scale {
                right.digits.push(0);
                right.scale += 1;
            }
            (left, right)
        }

        fn to_bigint(&self) -> JetBigInt {
            let mut s = String::new();
            for &d in &self.digits {
                s.push((b'0' + d) as char);
            }
            JetBigInt::from_str(&s).unwrap()
        }

        fn from_bigint(v: JetBigInt, scale: u32, negative: bool) -> JetDecimal {
            let s = v.to_string_rep();
            let body = if s.starts_with('-') { &s[1..] } else { &s };
            let digits: Vec<u8> = body.bytes().map(|b| b - b'0').collect();
            JetDecimal {
                negative,
                digits,
                scale,
            }
            .normalize()
        }

        pub fn add(&self, other: &JetDecimal) -> JetDecimal {
            let (a, b) = JetDecimal::align_scales(self, other);
            let sum = a.to_bigint().add(&b.to_bigint());
            let negative = if a.negative == b.negative {
                a.negative
            } else if a.to_bigint().cmp_abs(&b.to_bigint()) >= 0 {
                a.negative
            } else {
                b.negative
            };
            if a.negative == b.negative {
                JetDecimal::from_bigint(sum, a.scale, negative)
            } else {
                let diff = if a.to_bigint().cmp_abs(&b.to_bigint()) >= 0 {
                    a.to_bigint().sub_abs(&b.to_bigint())
                } else {
                    b.to_bigint().sub_abs(&a.to_bigint())
                };
                JetDecimal::from_bigint(diff, a.scale, negative)
            }
        }

        pub fn sub(&self, other: &JetDecimal) -> JetDecimal {
            let mut neg = other.clone();
            neg.negative = !neg.negative;
            self.add(&neg)
        }

        pub fn mul(&self, other: &JetDecimal) -> JetDecimal {
            let prod = self.to_bigint().mul(&other.to_bigint());
            JetDecimal::from_bigint(
                prod,
                self.scale + other.scale,
                self.negative != other.negative,
            )
        }

        pub fn to_string_rep(&self) -> String {
            if self.digits == [0] {
                return if self.scale == 0 {
                    "0".to_string()
                } else {
                    format!("0.{}", "0".repeat(self.scale as usize))
                };
            }
            let mut int_digits = self.digits.clone();
            let frac_len = self.scale as usize;
            let sign = if self.negative { "-" } else { "" };
            if frac_len == 0 {
                let s: String = int_digits.iter().map(|d| (b'0' + *d) as char).collect();
                return format!("{sign}{s}");
            }
            if int_digits.len() <= frac_len {
                let pad = frac_len - int_digits.len() + 1;
                int_digits.splice(0..0, vec![0; pad]);
            }
            let split = int_digits.len() - frac_len;
            let (whole, frac) = int_digits.split_at(split);
            let w: String = whole.iter().map(|d| (b'0' + *d) as char).collect();
            let f: String = frac.iter().map(|d| (b'0' + *d) as char).collect();
            format!("{sign}{w}.{f}")
        }
    }

    impl super::JetShow for JetDecimal {
        fn jet_show(&self) -> String {
            self.to_string_rep()
        }
    }

    impl super::JetDebug for JetDecimal {
        fn jet_debug(&self) -> String {
            self.to_string_rep()
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JSONError {
        pub line: i64,
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum JSON {
        Null,
        Boolean(bool),
        Number(f64),
        Integer(i64),
        Text(String),
        Array(Vec<JSON>),
        Object(std::collections::BTreeMap<String, JSON>),
    }

    impl super::JetDebug for IOContext {
        fn jet_debug(&self) -> String {
            crate::jet_debug_record(
                "IOContext",
                [
                    (
                        "operation".to_string(),
                        match self.operation {
                            IOOperation::Read => "Read",
                            IOOperation::Write => "Write",
                            IOOperation::Flush => "Flush",
                            IOOperation::Connect => "Connect",
                            IOOperation::Accept => "Accept",
                            IOOperation::Close => "Close",
                            IOOperation::Resolve => "Resolve",
                            IOOperation::Codec => "Codec",
                        }
                        .to_string(),
                    ),
                    (
                        "resource".to_string(),
                        super::JetDebug::jet_debug(&self.resource),
                    ),
                    (
                        "os_code".to_string(),
                        super::JetDebug::jet_debug(&self.os_code),
                    ),
                    (
                        "cause".to_string(),
                        super::JetDebug::jet_debug(&self.cause),
                    ),
                ],
            )
        }
    }

    impl super::JetShow for IOError {
        fn jet_show(&self) -> String {
            let (variant, context) = match self {
                IOError::InvalidInput(context) => (0, context),
                IOError::NotFound(context) => (1, context),
                IOError::PermissionDenied(context) => (2, context),
                IOError::TimedOut(context) => (3, context),
                IOError::Cancelled(context) => (4, context),
                IOError::Closed(context) => (5, context),
                IOError::Protocol(context) => (6, context),
                IOError::Other(context) => (7, context),
            };
            crate::jet_show_io_error(
                variant,
                context.operation as i64,
                context.resource.as_ref().ok().map(String::as_str),
                context.cause.as_ref().ok().map(String::as_str),
            )
        }
    }
    impl super::JetDisplay for IOError {
        fn jet_display(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetDebug for IOError {
        fn jet_debug(&self) -> String {
            let (variant, context) = match self {
                IOError::InvalidInput(context) => ("InvalidInput", context),
                IOError::NotFound(context) => ("NotFound", context),
                IOError::PermissionDenied(context) => ("PermissionDenied", context),
                IOError::TimedOut(context) => ("TimedOut", context),
                IOError::Cancelled(context) => ("Cancelled", context),
                IOError::Closed(context) => ("Closed", context),
                IOError::Protocol(context) => ("Protocol", context),
                IOError::Other(context) => ("Other", context),
            };
            crate::jet_debug_variant(variant, Some(super::JetDebug::jet_debug(context)))
        }
    }
    impl super::JetShow for EnvError {
        fn jet_show(&self) -> String {
            match self {
                EnvError::InvalidName => "invalid environment variable name".to_string(),
                EnvError::InvalidValue => "invalid environment variable value".to_string(),
                EnvError::NonUnicode => {
                    "environment contains a name or value that is not valid Unicode".to_string()
                }
            }
        }
    }
    impl super::JetDebug for EnvError {
        fn jet_debug(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetShow for UTF8Error {
        fn jet_show(&self) -> String {
            self.message.clone()
        }
    }
    impl super::JetDebug for UTF8Error {
        fn jet_debug(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetShow for RangeError {
        fn jet_show(&self) -> String {
            self.reason.clone()
        }
    }
    impl super::JetDebug for RangeError {
        fn jet_debug(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetShow for TextError {
        fn jet_show(&self) -> String {
            self.message.clone()
        }
    }
    impl super::JetDebug for TextError {
        fn jet_debug(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    // D-FAIL-CONV2=A: family members render failure text through one display hook.
    impl super::JetDisplay for EnvError {
        fn jet_display(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetDisplay for UTF8Error {
        fn jet_display(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetDisplay for RangeError {
        fn jet_display(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetDisplay for TextError {
        fn jet_display(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetShow for ProcessResult {
        fn jet_show(&self) -> String {
            format!("{:?}", self)
        }
    }
    impl super::JetShow for ProcessSpec {
        fn jet_show(&self) -> String {
            format!("ProcessSpec({:?})", self.cmd)
        }
    }
    impl super::JetShow for ProcessChild {
        fn jet_show(&self) -> String {
            "ProcessChild".to_string()
        }
    }
    impl super::JetShow for DirEntry {
        fn jet_show(&self) -> String {
            format!(
                "DirEntry {{ name: {:?}, path: {:?}, is_dir: {} }}",
                self.name, self.path, self.is_dir
            )
        }
    }
    impl super::JetShow for Stat {
        fn jet_show(&self) -> String {
            format!("Stat {{ kind: {}, size: {} }}", self.kind, self.size)
        }
    }
    impl super::JetShow for WalkEntry {
        fn jet_show(&self) -> String {
            format!(
                "WalkEntry {{ path: {:?}, depth: {} }}",
                self.path, self.depth
            )
        }
    }
    impl super::JetShow for WatchEvent {
        fn jet_show(&self) -> String {
            format!(
                "WatchEvent {{ domain: {:?}, kind: {:?}, path: {}, detail: {} }}",
                self.domain, self.kind, self.path, self.detail
            )
        }
    }
    impl super::JetShow for TempDir {
        fn jet_show(&self) -> String {
            self.path.clone()
        }
    }
    impl super::JetShow for TempFile {
        fn jet_show(&self) -> String {
            self.path.clone()
        }
    }
    impl super::JetShow for FileLock {
        fn jet_show(&self) -> String {
            self.path.clone()
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            if std::rc::Rc::strong_count(&self.cleanup) == 1 {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }
    impl Drop for TempFile {
        fn drop(&mut self) {
            if std::rc::Rc::strong_count(&self.cleanup) == 1 {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
    impl Drop for FileLock {
        fn drop(&mut self) {
            if std::rc::Rc::strong_count(&self.cleanup) == 1 {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
    impl super::JetShow for Stopwatch {
        fn jet_show(&self) -> String {
            format!("{:?}", self.start)
        }
    }
    impl super::JetShow for Clock {
        fn jet_show(&self) -> String {
            format!("Clock {{ now: {} }}", self.now())
        }
    }
    impl super::JetDebug for Clock {
        fn jet_debug(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetShow for Rng {
        fn jet_show(&self) -> String {
            format!("Rng {{ .. }}")
        }
    }
    impl super::JetShow for Fake {
        fn jet_show(&self) -> String {
            format!("Fake {{ locale: {} }}", if self.locale == 1 { "de" } else { "en" })
        }
    }
    impl super::JetShow for Solver {
        fn jet_show(&self) -> String {
            format!(
                "Solver {{ seed: {}, checked: {}, failures: {} }}",
                self.seed, self.checked, self.failures
            )
        }
    }
    impl super::JetShow for Duration {
        fn jet_show(&self) -> String {
            super::jet_duration_kernel_show(self.ns)
        }
    }
    impl super::JetDebug for Duration {
        fn jet_debug(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetShow for JSONError {
        fn jet_show(&self) -> String {
            format!("line {}: {}", self.line, self.message)
        }
    }
    impl super::JetDisplay for JSONError {
        fn jet_display(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetDebug for JSONError {
        fn jet_debug(&self) -> String {
            <Self as super::JetShow>::jet_show(self)
        }
    }
    impl super::JetShow for JSON {
        fn jet_show(&self) -> String {
            render_json(self, false, 0)
        }
    }

    // D-SERDE-ACCESS=B: accessor methods on JSON (= Data).
    impl JSON {
        pub fn field(&self, name: &str) -> Result<JSON, String> {
            match self {
                JSON::Object(map) => map
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("field `{}` not found", name)),
                _ => Err(format!(
                    "expected object, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn at(&self, i: i64) -> Result<JSON, String> {
            match self {
                JSON::Array(items) => {
                    let idx = if i < 0 {
                        items.len().wrapping_sub((-i) as usize)
                    } else {
                        i as usize
                    };
                    items
                        .get(idx)
                        .cloned()
                        .ok_or_else(|| format!("index {} out of bounds (len {})", i, items.len()))
                }
                _ => Err(format!(
                    "expected array, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn int(&self) -> Result<i64, String> {
            match self {
                JSON::Integer(n) => Ok(*n),
                JSON::Number(f) => {
                    let n = *f as i64;
                    if (n as f64 - f).abs() < 0.5 {
                        Ok(n)
                    } else {
                        Err(format!("{} is not an integer", f))
                    }
                }
                _ => Err(format!(
                    "expected number, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn text(&self) -> Result<String, String> {
            match self {
                JSON::Text(s) => Ok(s.clone()),
                _ => Err(format!(
                    "expected text, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn bool(&self) -> Result<bool, String> {
            match self {
                JSON::Boolean(b) => Ok(*b),
                _ => Err(format!(
                    "expected bool, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn float(&self) -> Result<f64, String> {
            match self {
                JSON::Integer(n) => Ok(*n as f64),
                JSON::Number(f) => Ok(*f),
                _ => Err(format!(
                    "expected number, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
    }
