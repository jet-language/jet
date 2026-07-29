    // D-PROCESS1=A: exactly the three ratified stream modes. `Stream` and
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
    // ProcessSpec and ProcessChild. Native PTY/ConPTY backends fill the session
    // handle in their successor slices.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TerminalSize {
        pub cols: i64,
        pub rows: i64,
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
                size: TerminalSize { cols: 80, rows: 24 },
                mode: TerminalMode::Cooked,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TerminalSession;

    // D-ENCSTREAM-SURFACE1=A: shared, handle-free encoding ABI.  These are
    // ordinary owned values; codec state itself remains behind non-Clone
    // format-native handles below.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct EncodingLimits {
        pub buffer_bytes: i64,
        pub max_depth: i64,
        pub max_item_bytes: i64,
        pub max_total_bytes: Option<i64>,
        pub max_expansion_depth: i64,
        pub max_expansion_bytes: i64,
    }
    impl EncodingLimits {
        pub fn safe() -> Self { Self { buffer_bytes: 65536, max_depth: 256, max_item_bytes: 16777216, max_total_bytes: None, max_expansion_depth: 32, max_expansion_bytes: 8388608 } }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum EncodingFormat { JSON, JSONL, CSV, XML, CBOR }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum EncodingErrorKind { Syntax, Truncated, Unsupported, Limit, IO, State }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct EncodingCause { pub kind: String, pub os_code: Option<i64>, pub message: String }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct EncodingError {
        pub format: EncodingFormat, pub kind: EncodingErrorKind, pub byte_offset: i64,
        pub line: Option<i64>, pub column: Option<i64>, pub path: String,
        pub reason: String, pub cause: Option<EncodingCause>,
    }
    impl EncodingError {
        /// D-ENCSTREAM-SURFACE1=A: handle-free IO snapshot when kind is IO.
        pub fn cause(&self) -> Option<EncodingCause> {
            self.cause.clone()
        }
        fn display_text(&self) -> String {
            let mut out = format!("{:?} {:?} at byte {}", self.format, self.kind, self.byte_offset);
            if let Some(line) = self.line {
                out.push_str(&format!(", line {line}"));
            }
            if let Some(column) = self.column {
                out.push_str(&format!(", column {column}"));
            }
            if !self.path.is_empty() {
                out.push_str(&format!(", path {}", self.path));
            }
            out.push_str(&format!(": {}", self.reason));
            out
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
    #[derive(Clone, Debug, PartialEq)]
    pub enum DataEvent {
        Null, Bool(bool), Int(i64), Float(f64), Text(String), Bytes(Vec<u8>),
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
        pub byte_offset: Option<i64>,
        pub line: Option<i64>,
        pub column: Option<i64>,
        pub path: String,
        pub reason: String,
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
        pub(crate) allocation_budget: Option<super::JetJSONAllocationBudget>,
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
        pub(crate) allocation: super::JetJSONAllocationBudget,
    }
    pub struct XMLWriter {
        pub(crate) output: super::JetFileWriter,
        pub(crate) limits: EncodingLimits,
        pub(crate) renderer: super::jet_xml_pull::StreamWriter,
        pub(crate) buffer: Vec<u8>,
        pub(crate) terminal: Option<EncodingError>,
        pub(crate) total: i64,
        pub(crate) finished: bool,
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
        pub(crate) allocation: super::JetJSONAllocationBudget,
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
        pub(crate) allocation: super::JetJSONAllocationBudget,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct ProcessSpec {
        pub cmd: Vec<String>,
        pub cwd: Option<String>,
        pub env_clear: bool,
        pub env_set: Vec<(String, String)>,
        pub env_remove: Vec<String>,
        // `None` (default) closes the child's stdin (`Stdio::null()`) — matches
        // the pre-D-PROCESS1 default of no accidental stdin inheritance.
        pub stdin: Option<ProcessStreamMode>,
        pub stdout: ProcessStreamMode,
        pub stderr: ProcessStreamMode,
        pub timeout_ms: Option<i64>,
        pub output_limit: Option<i64>,
        pub detached: bool,
        // D-PROCESS-SESSION1=A: `.terminal()` asks for a terminal-backed
        // session. Argv execution with no terminal stays the default, so this
        // flag is the one opt-in. A launch that asks for a terminal never runs
        // without one: it fails when no PTY/ConPTY backend is available.
        pub terminal: Option<TerminalPolicy>,
    }

    #[derive(Clone, Debug)]
    pub struct ProcessChild {
        pub inner: std::rc::Rc<std::cell::RefCell<Option<std::process::Child>>>,
        pub stdin: std::rc::Rc<std::cell::RefCell<Option<std::process::ChildStdin>>>,
        pub stdout:
            std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<std::process::ChildStdout>>>>,
        pub stderr:
            std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<std::process::ChildStderr>>>>,
        pub terminal: Option<TerminalSession>,
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

    #[derive(Clone, Debug, PartialEq)]
    pub struct WatchEvent {
        pub domain: String,
        pub kind: String,
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

    /// D-DATAFLOW1=A: typed streaming + invalid-data policy (edition 2027 surface).
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
    impl super::JetDisplay for DataErrorKind {
        fn jet_display(&self) -> String {
            format!("{self:?}")
        }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DataError {
        pub kind: DataErrorKind,
        pub operation: String,
        pub row: Option<i64>,
        pub column: Option<i64>,
        pub index: Option<i64>,
        pub reason: String,
        pub cause: Option<EncodingError>,
    }
    impl DataError {
        fn display_text(&self) -> String {
            let mut out = format!("{:?} {}", self.kind, self.operation);
            if let Some(row) = self.row {
                out.push_str(&format!(", row {row}"));
            }
            if let Some(column) = self.column {
                out.push_str(&format!(", column {column}"));
            }
            if let Some(index) = self.index {
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

    // D-DET1 / D-TTL-CLOCK2=A: manual clocks are deterministic; system clocks
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

    // D-SOLVER-LIB1=A: explicit finite solver state. This first slice records
    // ordinary Bool constraints in insertion order; no hidden backtracking.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Solver {
        pub seed: i64,
        pub checked: i64,
        pub failures: i64,
    }

    // D-SHAPE-DURATION1=A: a checked elapsed span stored canonically as whole
    // milliseconds. Static unit literals keep their existing compile-time path.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Duration {
        pub ms: i64,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum DurationUnit {
        Milliseconds,
        Seconds,
        Minutes,
        Hours,
    }

    impl DurationUnit {
        pub fn milliseconds(self) -> i64 {
            match self {
                Self::Milliseconds => 1,
                Self::Seconds => 1_000,
                Self::Minutes => 60_000,
                Self::Hours => 3_600_000,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct RangeError {
        pub reason: String,
    }

    // D-BIGINT1: arbitrary-precision integer (std-only limb arithmetic).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetBigInt {
        negative: bool,
        limbs: Vec<u32>, // little-endian base 10^9
    }

    const BI_BASE: u64 = 1_000_000_000;

    impl JetBigInt {
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
                return Err("empty BigInt string".to_string());
            }
            let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = t.strip_prefix('+') {
                (false, rest)
            } else {
                (false, t)
            };
            if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("invalid BigInt string `{s}`"));
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
        Text(String),
        Array(Vec<JSON>),
        Object(std::collections::BTreeMap<String, JSON>),
    }

    impl super::JetShow for IOError {
        fn jet_show(&self) -> String {
            let (kind, context) = match self {
                IOError::InvalidInput(context) => ("invalid input", context),
                IOError::NotFound(context) => ("not found", context),
                IOError::PermissionDenied(context) => ("permission denied", context),
                IOError::TimedOut(context) => ("timed out", context),
                IOError::Cancelled(context) => ("cancelled", context),
                IOError::Closed(context) => ("closed", context),
                IOError::Protocol(context) => ("protocol error", context),
                IOError::Other(context) => ("I/O error", context),
            };
            let operation = match context.operation {
                IOOperation::Read => "read", IOOperation::Write => "write",
                IOOperation::Flush => "flush", IOOperation::Connect => "connect",
                IOOperation::Accept => "accept", IOOperation::Close => "close",
                IOOperation::Resolve => "resolve", IOOperation::Codec => "codec",
            };
            let mut text = format!("{kind} during {operation}");
            if let Some(resource) = &context.resource { text.push_str(&format!(" `{resource}`")); }
            if let Some(cause) = &context.cause { text.push_str(&format!(": {cause}")); }
            text
        }
    }
    impl super::JetDebug for IOError {
        fn jet_debug(&self) -> String {
            format!("{:?}", self)
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
    impl super::JetShow for UTF8Error {
        fn jet_show(&self) -> String {
            self.message.clone()
        }
    }
    impl super::JetShow for RangeError {
        fn jet_show(&self) -> String {
            self.reason.clone()
        }
    }
    impl super::JetShow for TextError {
        fn jet_show(&self) -> String {
            self.message.clone()
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
                "WatchEvent {{ domain: {}, kind: {}, path: {}, detail: {} }}",
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
            format!("{}ms", self.ms)
        }
    }
    impl super::JetShow for JSONError {
        fn jet_show(&self) -> String {
            format!("line {}: {}", self.line, self.message)
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
                JSON::Number(f) => Ok(*f),
                _ => Err(format!(
                    "expected number, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
    }
