// Exactly the three stream modes. `Stream` and
// `Capture` both pipe the child's stream (`Stdio::piped()`) — they differ
// only in which Jet-level API exposes or collects the drained bytes
// (`Child.stdout.lines()` for `Stream`, the collected
// `ProcessResult.output`/`.errors` for `Capture`).
#[derive(Clone, Debug, PartialEq)]
pub enum ProcessStreamMode {
    Stream,
    Inherit,
    Capture,
}
// D-FOUND-LIFECYCLE1=A: OS stop requests are a closed typed set. The runtime
// maps each value onto the one shared signal/cancellation kernel; callers do
// not poll a flag or carry a second lifecycle object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessSignal {
    Term,
    Hup,
    Int,
}

impl ProcessSignal {
    pub const fn mask(self) -> usize {
        match self {
            Self::Int => 1 << 0,
            Self::Hup => 1 << 1,
            Self::Term => 1 << 2,
        }
    }
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

// JET_VETTED_UNSAFE_BEGIN: jet_conpty_control
#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct ConPtyControl {
    handle: std::cell::Cell<usize>,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "ClosePseudoConsole"]
    fn close_pseudo_console(handle: *mut std::ffi::c_void);
}

#[cfg(windows)]
impl ConPtyControl {
    pub(crate) fn new(handle: usize) -> Self {
        Self {
            handle: std::cell::Cell::new(handle),
        }
    }

    pub(crate) fn raw(&self) -> usize {
        self.handle.get()
    }

    pub(crate) fn close(&self) {
        let handle = self.handle.replace(0);
        if handle != 0 {
            // SAFETY: this control owns the one live HPCON; replacing the
            // handle with zero makes close idempotent across wait and drop.
            unsafe { close_pseudo_console(handle as *mut std::ffi::c_void) };
        }
    }
}

#[cfg(windows)]
impl Drop for ConPtyControl {
    fn drop(&mut self) {
        self.close();
    }
}
// JET_VETTED_UNSAFE_END: jet_conpty_control

#[derive(Clone, Debug)]
pub struct TerminalSession {
    // Unix resize uses the PTY master. Windows resize uses the owned HPCON;
    // keeping the control handle separate avoids holding an output-pipe
    // clone open after the child exits.
    #[cfg(unix)]
    pub(crate) master: std::rc::Rc<std::fs::File>,
    #[cfg(windows)]
    pub(crate) control: std::rc::Rc<ConPtyControl>,
}

impl PartialEq for TerminalSession {
    fn eq(&self, other: &Self) -> bool {
        #[cfg(unix)]
        return std::rc::Rc::ptr_eq(&self.master, &other.master);
        #[cfg(windows)]
        return std::rc::Rc::ptr_eq(&self.control, &other.control);
        #[cfg(not(any(unix, windows)))]
        false
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
    Shared(std::sync::Arc<ProcessOutputState>),
}

#[derive(Debug)]
pub(crate) struct ProcessOutputState {
    pub(crate) bytes: std::sync::Mutex<ProcessOutputBuffer>,
    pub(crate) ready: std::sync::Condvar,
}

#[derive(Debug)]
pub(crate) struct ProcessOutputBuffer {
    pub(crate) bytes: Vec<u8>,
    pub(crate) cursor: usize,
    pub(crate) closed: bool,
    pub(crate) error: Option<(std::io::ErrorKind, String)>,
}

impl EncodingError {
    /// D-ENCSTREAM-SURFACE1=A: handle-free IO snapshot when kind is IO.
    pub fn cause(&self) -> JetOutcome<EncodingCause, JetAbsent> {
        self.cause.clone()
    }
    fn display_text(&self) -> String {
        super::jet_encoding_error_kernel_show(
            self.format.as_str(),
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
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Number(String),
    Text(String),
    Bytes(Vec<u8>),
    ArrayStart,
    ArrayEnd,
    ObjectStart,
    Key(String),
    ObjectEnd,
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
        Self {
            max_depth: 256,
            max_items: 1_000_000,
            max_bytes: 1_073_741_824,
            require_canonical: false,
        }
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
        Self {
            max_depth: 256,
            max_nodes: 1_000_000,
            max_attributes_per_element: 1024,
            max_name_bytes: 4096,
            max_text_bytes: 16_777_216,
            max_entity_declarations: 1024,
            max_entity_depth: 32,
            max_entity_replacement_bytes: 8_388_608,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XMLEntityPolicy {
    Preserve,
    Reject,
    Resolve(std::collections::BTreeMap<String, String>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XMLParseOptions {
    pub entities: XMLEntityPolicy,
    pub limits: XMLLimits,
}
impl XMLParseOptions {
    pub fn safe() -> Self {
        Self {
            entities: XMLEntityPolicy::Preserve,
            limits: XMLLimits::safe(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XMLEncoding {
    UTF8,
    UTF8BOM,
    UTF16LE,
    UTF16BE,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XMLLexicalPolicy {
    PreserveValid,
    Deterministic,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XMLRenderOptions {
    pub encoding: XMLEncoding,
    pub lexical: XMLLexicalPolicy,
}
impl XMLRenderOptions {
    pub fn safe() -> Self {
        Self {
            encoding: XMLEncoding::UTF8,
            lexical: XMLLexicalPolicy::PreserveValid,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XMLCanonicalMode {
    Inclusive11,
    Exclusive10,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XMLCanonical {
    pub mode: XMLCanonicalMode,
    pub comments: bool,
    pub inclusive_prefixes: Vec<String>,
}
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CSVRow {
    pub fields: Vec<String>,
    pub line: i64,
}
pub struct CSVReader {
    pub(crate) input: super::JetFileReader,
    pub(crate) limits: EncodingLimits,
    pub(crate) delimiter: char,
    pub(crate) header: bool,
    pub(crate) skip_blank: bool,
    pub(crate) parser: super::jet_csv_kernel::CsvParser,
    pub(crate) allocation: super::JetEncodingAllocationBudget,
    pub(crate) utf8: [u8; 4],
    pub(crate) utf8_len: usize,
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
    /// CPU budget in milliseconds. Unix applies the native CPU rlimit;
    /// Windows applies a Job Object process-time limit.
    pub cpu_time_limit_ms: Option<i64>,
    /// Address-space/resident-set ceiling in bytes, according to the
    /// platform backend's documented enforcement.
    pub memory_limit_bytes: Option<i64>,
    /// Maximum descriptors/handles the child may keep open.
    pub open_file_limit: Option<i64>,
    pub detached: bool,
    // D-PROCESS-SESSION1=A: `.terminal()` asks for a terminal-backed
    // session. Argv execution with no terminal stays the default, so this
    // flag is the one opt-in. A launch that asks for a terminal never runs
    // without one: it fails when no native PTY/ConPTY backend is available.
    pub terminal: Option<TerminalPolicy>,
    // D-AGENT-EXEC1: an authority-bound spec carries the canonical policy
    // wire value. It is deliberately opaque here; ProcessPolicy.rs owns
    // interpretation, digesting, planning, and backend refusal.
    pub policy_wire: Option<String>,
}

/// D-AGENT-EXEC1: the dry-run record shared by AOT, JIT, and interpreter.
/// A plan is only returned after executable identity resolution and backend
/// selection. The public argv is redacted; input_digest binds the exact
/// argv used by launch without placing command secrets in the plan.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessPlan {
    pub executable_identity: String,
    pub argv: Vec<String>,
    pub input_digest: String,
    pub policy_digest: String,
    pub backend: String,
    pub authority: Vec<String>,
    pub descendants: String,
    pub limits: Vec<String>,
    pub outputs: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum ProcessHandle {
    Std {
        child: std::process::Child,
        job: Option<std::rc::Rc<std::fs::File>>,
    },
    #[cfg(windows)]
    Native {
        process: std::fs::File,
        job: std::rc::Rc<std::fs::File>,
        pid: u32,
    },
}

#[derive(Clone, Debug)]
pub struct ProcessChild {
    pub inner: std::rc::Rc<std::cell::RefCell<Option<ProcessHandle>>>,
    pub wait_result: std::rc::Rc<std::cell::RefCell<Option<ProcessReceipt>>>,
    // Cancellation/drop cleanup runs outside the caller's result path.
    // Retain a native cleanup failure so a later wait reports the typed
    // process error instead of silently losing it.
    pub cleanup_error: std::rc::Rc<std::cell::RefCell<Option<IOError>>>,
    pub stdin: std::rc::Rc<std::cell::RefCell<Option<ProcessStdin>>>,
    pub stdout: std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
    pub stderr: std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
    pub stdout_state: Option<std::sync::Arc<ProcessOutputState>>,
    pub stderr_state: Option<std::sync::Arc<ProcessOutputState>>,
    pub stdout_worker:
        std::rc::Rc<std::cell::RefCell<Option<std::thread::JoinHandle<std::io::Result<()>>>>>,
    pub stderr_worker:
        std::rc::Rc<std::cell::RefCell<Option<std::thread::JoinHandle<std::io::Result<()>>>>>,
    pub output_limit_hit: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub output_read_error: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub terminal: JetOutcome<TerminalSession, JetAbsent>,
    pub process_group: bool,
    pub detached: bool,
    pub timeout_ms: Option<i64>,
    // Keep the limit on the child so `spawn().wait()` enforces the same
    // bound as `run()`, before captured bytes can grow without bound.
    pub output_limit: Option<i64>,
    pub audit_spec: ProcessSpec,
    pub audit_plan: Option<ProcessPlan>,
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
    pub mode: i64,
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

/// D-QUERY-RETAIN1=A: grouped query results retain both the nominal key and
/// the reducer's exact value type.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupValue<K, V> {
    pub key: K,
    pub value: V,
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
    /// D-QUERY-LIVE1: the requested maintenance operation is not eligible.
    Unsupported,
    /// A source identity already has a retained row.
    DuplicateKey,
    /// A source identity was not found.
    MissingKey,
    /// A Shared snapshot was captured from another owner.
    WrongOwner,
    /// A Shared snapshot no longer names the current source revision.
    StaleRevision,
    /// A value violates the checked source contract.
    InvalidValue,
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
    pub(crate) fn simple(kind: DataErrorKind, operation: &str, reason: impl Into<String>) -> Self {
        Self {
            kind,
            operation: operation.to_string(),
            row: Err(JetAbsent),
            column: Err(JetAbsent),
            index: Err(JetAbsent),
            reason: reason.into(),
            cause: Err(JetAbsent),
        }
    }

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
    JSONL {
        reader: JSONLReader,
    },
    Provider {
        format: DataFormat,
        rows: Vec<DataTree>,
        cursor: usize,
    },
}
pub struct DataStream {
    pub(crate) inner: DataStreamInner,
    pub(crate) limits: DataLimits,
    pub(crate) terminal: Option<DataError>,
    pub(crate) eof: bool,
    pub(crate) row_index: i64,
    pub(crate) cancelled: bool,
}

/// D-DATAFRAME1=A: one typed column in an ordinary list schema or loader
/// snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct DataColumn {
    pub id: String,
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
}

/// The public schema carrier for loader snapshots and typed list values.
#[derive(Clone, Debug, PartialEq)]
pub struct DataSchema {
    pub identity: String,
    pub format: DataFormat,
    pub columns: Vec<DataColumn>,
    pub projection: Option<jet_foundation::Shape::ShapeProjection>,
}

impl DataSchema {
    /// Infer one deterministic schema across every row in a decoded tree.
    ///
    /// Missing fields and explicit nulls are nullable. Conflicting concrete
    /// types widen to `Any`, while integers and floats share `float`.
    pub fn infer(tree: &DataTree, format: DataFormat) -> Self {
        fn merge_type(left: &'static str, right: &'static str) -> (&'static str, bool) {
            if left == right {
                return (
                    match left {
                        "null" => "null",
                        "bool" => "bool",
                        "int" => "int",
                        "float" => "float",
                        "text" => "text",
                        "bytes" => "bytes",
                        "array" => "array",
                        "object" => "object",
                        _ => "Any",
                    },
                    left == "null",
                );
            }
            if left == "null" {
                return (right, true);
            }
            if right == "null" {
                return (left, true);
            }
            if (left == "int" && right == "float") || (left == "float" && right == "int") {
                return ("float", false);
            }
            ("Any", false)
        }
        fn value_type(value: &DataTree) -> &'static str {
            match value {
                DataTree::Null => "null",
                DataTree::Bool(_) => "bool",
                DataTree::Int(_) => "int",
                DataTree::Float(_) | DataTree::Number(_) => "float",
                DataTree::TypedText(_) | DataTree::Text(_) => "text",
                DataTree::Bytes(_) => "bytes",
                DataTree::Array(_) => "array",
                DataTree::Object(_) => "object",
            }
        }
        fn absorb(
            fields: &mut std::collections::BTreeMap<String, (&'static str, bool)>,
            name: String,
            value: &DataTree,
            nullable: bool,
        ) {
            let current_type = value_type(value);
            let (type_name, merged_nullable) = fields
                .get(&name)
                .map(|(current, current_nullable)| {
                    let (merged, type_nullable) = merge_type(current, current_type);
                    (merged, *current_nullable || nullable || type_nullable)
                })
                .unwrap_or((current_type, nullable || current_type == "null"));
            fields.insert(name, (type_name, merged_nullable));
        }

        let mut fields = std::collections::BTreeMap::<String, (&'static str, bool)>::new();
        match tree {
            DataTree::Object(entries) => {
                for (name, value) in entries {
                    absorb(&mut fields, name.clone(), value, matches!(value, DataTree::Null));
                }
            }
            DataTree::Array(values) => {
                let mut rows_seen = 0usize;
                for value in values {
                    let DataTree::Object(entries) = value else {
                        absorb(
                            &mut fields,
                            "$".to_string(),
                            value,
                            rows_seen > 0 || matches!(value, DataTree::Null),
                        );
                        rows_seen = rows_seen.saturating_add(1);
                        continue;
                    };
                    let present = entries
                        .iter()
                        .map(|(name, _)| name)
                        .collect::<std::collections::BTreeSet<_>>();
                    for (name, value) in entries {
                        absorb(
                            &mut fields,
                            name.clone(),
                            value,
                            rows_seen > 0 || matches!(value, DataTree::Null),
                        );
                    }
                    for name in fields
                        .keys()
                        .filter(|name| !present.contains(name))
                        .cloned()
                        .collect::<Vec<_>>()
                    {
                        if let Some((type_name, _)) = fields.get(&name).copied() {
                            fields.insert(name, (type_name, true));
                        }
                    }
                    rows_seen = rows_seen.saturating_add(1);
                }
            }
            value => absorb(
                &mut fields,
                "$".to_string(),
                value,
                matches!(value, DataTree::Null),
            ),
        }
        let columns = fields
            .into_iter()
            .enumerate()
            .map(|(order, (name, (type_name, nullable)))| DataColumn {
                id: format!("column-{order}-{name}"),
                name,
                type_name: type_name.to_string(),
                nullable,
            })
            .collect::<Vec<_>>();
        let mut identity = String::new();
        let format_name = format.as_str();
        identity.push_str(&format_name.len().to_string());
        identity.push(':');
        identity.push_str(format_name);
        identity.push(';');
        for column in &columns {
            for value in [
                column.id.as_str(),
                column.name.as_str(),
                column.type_name.as_str(),
                if column.nullable { "true" } else { "false" },
            ] {
                identity.push_str(&value.len().to_string());
                identity.push(':');
                identity.push_str(value);
                identity.push(';');
            }
        }
        Self {
            identity,
            format,
            columns,
            projection: None,
        }
    }
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
/// D-DX-LOADERS1=A: one dependency declaration shared by local loaders and
/// provider packages. Locator/member/parameters are public identity only;
/// credential material never belongs here.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DataLoaderKind {
    File,
    Url,
    Database,
    Value,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum DataFormat {
    CSV,
    JSON,
    JSONL,
    Parquet,
    Arrow,
}

impl DataFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CSV => "csv",
            Self::JSON => "json",
            Self::JSONL => "jsonl",
            Self::Parquet => "parquet",
            Self::Arrow => "arrow",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "csv" => Some(Self::CSV),
            "json" => Some(Self::JSON),
            "jsonl" | "ndjson" => Some(Self::JSONL),
            "parquet" => Some(Self::Parquet),
            "arrow" | "arrow-ipc" | "feather" => Some(Self::Arrow),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DataFreshness {
    Pending,
    Fresh,
    Stale,
    Error,
    Offline,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DataInvalidationCause {
    None,
    Loader,
    Input,
    ArchiveMember,
    Parameters,
    Credential,
    Capability,
    Manual,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DataAuthority {
    pub scope: String,
    pub revision: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DataSourceIdentity {
    pub kind: DataLoaderKind,
    pub locator: String,
    pub member: String,
    pub parameters: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DataProvenance {
    pub source: DataSourceIdentity,
    pub format: DataFormat,
    pub authority: DataAuthority,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DataSnapshotIdentity {
    pub id: String,
    pub source: String,
    pub content: String,
    pub schema: String,
    pub format: DataFormat,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DataLoaderStatus {
    pub identity: String,
    pub freshness: DataFreshness,
    pub invalidated_by: DataInvalidationCause,
    pub error: String,
    pub cleanup: String,
    pub buffered_bytes: i64,
    pub backpressure: bool,
    pub last_good: bool,
}

/// A typed, lazy declaration. `payload` is provider-owned input, not a cache:
/// local files are read at snapshot time and URL/database providers bind one
/// response into this value before the same snapshot path decodes it.
/// `last_good` is the sole retained snapshot payload for offline/stale reuse;
/// no separate loader cache or credential store is implied.
pub struct DataLoader<T> {
    pub source: DataSourceIdentity,
    pub format: DataFormat,
    pub authority: DataAuthority,
    pub limits: DataLimits,
    pub payload: Option<Vec<u8>>,
    pub last_good: Option<Vec<u8>>,
    pub cancelled: bool,
    pub offline: bool,
    pub status: DataLoaderStatus,
    pub(crate) raw_locator: String,
    pub marker: std::marker::PhantomData<fn() -> T>,
}

#[derive(Clone, Debug)]
pub struct DataSnapshot<T> {
    pub value: T,
    pub identity: DataSnapshotIdentity,
    pub provenance: DataProvenance,
    pub schema: DataSchema,
    pub status: DataLoaderStatus,
    pub content: Vec<u8>,
}

/// Safe surface carrier for a checked Arrow owner. The foreign C importer is
/// demand-emitted separately; this declaration keeps `core.data.arrow` types
/// nameable from the always-present data surface without importing that ABI.
pub trait DataArrowOps<T> {
    fn row_count(&self) -> usize;
    fn row(&self, index: usize) -> Result<T, String>;
}

pub struct DataArrowBatch<T> {
    // A batch has one release owner.  Sharing is expressed by the immutable
    // row operations below, never by duplicating this owning carrier.
    owner: Box<dyn DataArrowOps<T>>,
}

impl<T> std::fmt::Debug for DataArrowBatch<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataArrowBatch")
            .field("rows", &self.row_count())
            .finish()
    }
}

impl<T> DataArrowBatch<T> {
    pub(crate) fn from_ops(owner: Box<dyn DataArrowOps<T>>) -> Self {
        Self { owner }
    }

    pub fn row_count(&self) -> usize {
        self.owner.row_count()
    }
    pub(crate) fn row(&self, index: usize) -> Result<T, String> {
        self.owner.row(index)
    }
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


/// One immutable node in a deferred Query's internal row source.  This is not
/// a second public materialized data carrier: only Query exposes it.
pub(crate) enum DataQueryPlan<T> {
    Scan {
        rows: std::sync::Arc<Vec<T>>,
    },
    Filter {
        input: std::sync::Arc<DataQueryPlan<T>>,
        predicate: std::sync::Arc<dyn Fn(T) -> bool>,
    },
    SortBy {
        input: std::sync::Arc<DataQueryPlan<T>>,
        key: std::sync::Arc<dyn Fn(T) -> String>,
    },
}
impl<T> Clone for DataQueryPlan<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Scan { rows } => Self::Scan { rows: rows.clone() },
            Self::Filter { input, predicate } => Self::Filter {
                input: input.clone(),
                predicate: predicate.clone(),
            },
            Self::SortBy { input, key } => Self::SortBy {
                input: input.clone(),
                key: key.clone(),
            },
        }
    }
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DataQueryNodeKind {
    Scan,
    Filter,
    SortBy,
}

impl DataQueryNodeKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Filter => "filter",
            Self::SortBy => "sort_by",
        }
    }
}

impl<T> DataQueryPlan<T> {
    pub(crate) fn node_kind(&self) -> DataQueryNodeKind {
        match self {
            Self::Scan { .. } => DataQueryNodeKind::Scan,
            Self::Filter { .. } => DataQueryNodeKind::Filter,
            Self::SortBy { .. } => DataQueryNodeKind::SortBy,
        }
    }

    pub(crate) fn is_streaming(&self) -> bool {
        match self {
            Self::Scan { .. } => true,
            Self::Filter { input, .. } => input.is_streaming(),
            Self::SortBy { .. } => false,
        }
    }

    pub(crate) fn streaming_fallback(&self) -> Option<&'static str> {
        match self {
            Self::Scan { .. } => None,
            Self::Filter { input, .. } => input.streaming_fallback(),
            Self::SortBy { .. } => Some("sort_by"),
        }
    }
}

/// Query's private reusable list source.  It owns logical plan metadata and a
/// pull cursor without exposing a second data carrier in the language surface.
pub(crate) struct DataQueryRows<T> {
    pub(crate) root: std::sync::Arc<DataQueryPlan<T>>,
    pub(crate) missing: i64,
    pub(crate) plan: crate::JetTablePlan,
}
impl<T> Clone for DataQueryRows<T> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            missing: self.missing,
            plan: self.plan.clone(),
        }
    }
}


impl<T> DataQueryRows<T> {
    pub(crate) fn from_rows(rows: Vec<T>, missing: i64, plan: crate::JetTablePlan) -> Self {
        Self {
            root: std::sync::Arc::new(DataQueryPlan::Scan {
                rows: std::sync::Arc::new(rows),
            }),
            missing,
            plan,
        }
    }

    pub(crate) fn filter<F>(&self, predicate: F) -> Self
    where
        F: Fn(T) -> bool + 'static,
    {
        let mut plan = self.plan.clone();
        let schema = plan.output_schema().cloned().unwrap_or_else(|| {
            crate::JetTableSchema::new(
                crate::JetTableTypeId::new(std::any::type_name::<T>()),
                Vec::new(),
            )
        });
        let row_type = schema.row_type.clone();
        let callable = crate::JetTableCallable::new(
            "data.query.filter",
            row_type,
            crate::JetTableTypeId::boolean(),
        );
        plan.append_filter(schema, callable)
            .expect("typed query filter plan must remain valid");
        Self {
            root: std::sync::Arc::new(DataQueryPlan::Filter {
                input: self.root.clone(),
                predicate: std::sync::Arc::new(predicate),
            }),
            missing: self.missing,
            plan,
        }
    }

    pub(crate) fn sort_by<F>(&self, key: F) -> Self
    where
        F: Fn(T) -> String + 'static,
    {
        let mut plan = self.plan.clone();
        let schema = plan.output_schema().cloned().unwrap_or_else(|| {
            crate::JetTableSchema::new(
                crate::JetTableTypeId::new(std::any::type_name::<T>()),
                Vec::new(),
            )
        });
        let row_type = schema.row_type.clone();
        let callable = crate::JetTableCallable::new(
            "data.query.sort_by",
            row_type,
            crate::JetTableTypeId::string(),
        );
        plan.append_sort(schema, callable)
            .expect("typed query sort plan must remain valid");
        Self {
            root: std::sync::Arc::new(DataQueryPlan::SortBy {
                input: self.root.clone(),
                key: std::sync::Arc::new(key),
            }),
            missing: self.missing,
            plan,
        }
    }

    pub(crate) fn is_streaming(&self) -> bool {
        self.root.is_streaming()
    }

    pub(crate) fn streaming_fallback(&self) -> Option<&'static str> {
        self.root.streaming_fallback()
    }

    pub(crate) fn explain(&self) -> Vec<String> {
        let mut steps = self
            .plan
            .nodes
            .iter()
            .map(|node| node.operation.as_str().to_string())
            .collect::<Vec<_>>();
        if self
            .plan
            .output_node()
            .is_some_and(|node| !node.stream.is_streaming())
        {
            if let Some(node) = self.plan.output_node() {
                steps.push(format!("streaming_fallback:{}", node.operation.as_str()));
            }
        }
        steps
    }
}
/// D-QUERY-RETAIN1=A: one deferred query carrier covers both reusable
/// list-backed plans and one-shot streams.  Stream state is shared by derived
/// queries, so a second collect cannot reopen or silently replay the source.
pub(crate) enum DataQueryOperation<T> {
    Filter(std::sync::Arc<dyn Fn(T) -> bool>),
    SortBy(std::sync::Arc<dyn Fn(T) -> String>),
}
/// D-QUERY-LIVE1=A: a tracked source owns one canonical row set. Its
/// publication revision is the `JetShared` revision; the source does not keep
/// a second epoch in its payload.
#[derive(Clone)]
pub(crate) enum DataTrackedChangeKind<T> {
    Insert { row: T },
    Replace { before: T, after: T },
    Remove { row: T },
}

#[derive(Clone)]
pub(crate) struct DataTrackedChange<T> {
    pub(crate) revision: u64,
    pub(crate) order: u64,
    pub(crate) kind: DataTrackedChangeKind<T>,
}

#[derive(Clone)]
struct DataTrackedState<T, K> {
    rows: Vec<T>,
    keys: Vec<K>,
    orders: Vec<u64>,
    next_order: u64,
    changes: Vec<DataTrackedChange<T>>,
    key: std::sync::Arc<dyn Fn(T) -> K>,
}

pub(crate) trait DataTrackedReader<T> {
    fn snapshot_rows(&self) -> Vec<T>;
    fn snapshot_entries(&self) -> Vec<(u64, T)>;
    fn snapshot_entries_at_revision(&self) -> (u64, Vec<(u64, T)>);
    fn changes_since(&self, revision: u64) -> Result<Vec<DataTrackedChange<T>>, DataError>;
    fn revision(&self) -> u64;
    fn retained_rows(&self) -> usize;
    fn plan(&self, row_count: usize) -> crate::JetTablePlan;
}

pub(crate) trait DataTrackedMeta {
    fn revision(&self) -> u64;
    fn retained_rows(&self) -> usize;
}

/// A mutable, keyed source for maintained typed queries. K is deliberately
/// retained in the carrier: duplicate detection and reducer output never pass
/// through a textual projection.
pub struct DataTracked<T, K> {
    state: std::rc::Rc<JetShared<DataTrackedState<T, K>>>,
}

impl<T, K> Clone for DataTracked<T, K> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<T: Clone + 'static, K: Clone + PartialEq + 'static> DataTracked<T, K> {
    pub fn new(rows: Vec<T>, key: impl Fn(T) -> K + 'static) -> Result<Self, DataError> {
        let key = std::sync::Arc::new(key);
        let max_rows = DataLimits::safe().max_output_rows;
        if rows.len() as i64 > max_rows {
            return Err(DataError::simple(
                DataErrorKind::Limit,
                "data.track",
                format!("retained rows {} exceed max_output_rows {max_rows}", rows.len()),
            ));
        }
        let mut keys = Vec::with_capacity(rows.len());
        for row in &rows {
            let candidate = key(row.clone());
            if keys.iter().any(|existing| existing == &candidate) {
                return Err(DataError::simple(
                    DataErrorKind::DuplicateKey,
                    "data.track",
                    "source keys must be unique",
                ));
            }
            keys.push(candidate);
        }
        let orders = (0..rows.len()).map(|order| order as u64).collect();
        let next_order = keys.len() as u64;
        Ok(Self {
            state: std::rc::Rc::new(JetShared::new(DataTrackedState {
                rows,
                keys,
                orders,
                next_order,
                changes: Vec::new(),
                key,
            })),
        })
    }

    fn update<F>(&self, operation: &str, update: F) -> Result<(), DataError>
    where
        F: FnOnce(&mut DataTrackedState<T, K>, u64) -> Result<(), DataError>,
    {
        let snapshot = self.state.capture();
        let source_revision = snapshot.revision();
        let next_revision = source_revision.checked_add(1).ok_or_else(|| {
            DataError::simple(
                DataErrorKind::Overflow,
                operation,
                "source revision exhausted",
            )
        })?;
        let mut next = snapshot.value();
        update(&mut next, next_revision)?;
        match self.state.try_replace(snapshot, next) {
            Ok(true) => Ok(()),
            Ok(false) => Err(DataError::simple(
                DataErrorKind::StaleRevision,
                operation,
                "source changed before publication",
            )),
            Err(JetSharedRevisionError::WrongOwner) => Err(DataError::simple(
                DataErrorKind::WrongOwner,
                operation,
                "source snapshot belongs to another owner",
            )),
            Err(JetSharedRevisionError::GenerationExhausted) => Err(DataError::simple(
                DataErrorKind::Overflow,
                operation,
                "source revision exhausted",
            )),
        }
    }

    fn ensure_change_capacity(
        state: &DataTrackedState<T, K>,
        operation: &str,
    ) -> Result<(), DataError> {
        let limit = usize::try_from(DataLimits::safe().max_output_rows).unwrap_or(usize::MAX);
        if state.changes.len() >= limit {
            return Err(DataError::simple(
                DataErrorKind::Limit,
                operation,
                format!("retained change state exceeds max_output_rows {limit}"),
            ));
        }
        Ok(())
    }

    pub fn insert(&self, row: T) -> Result<(), DataError> {
        self.update("data.track.insert", move |state, revision| {
            let candidate = (state.key)(row.clone());
            if state.keys.iter().any(|existing| existing == &candidate) {
                return Err(DataError::simple(
                    DataErrorKind::DuplicateKey,
                    "data.track.insert",
                    "source key already exists",
                ));
            }
            let max_rows = DataLimits::safe().max_output_rows;
            if state.rows.len() as i64 >= max_rows {
                return Err(DataError::simple(
                    DataErrorKind::Limit,
                    "data.track.insert",
                    format!("retained rows would exceed max_output_rows {max_rows}"),
                ));
            }
            Self::ensure_change_capacity(state, "data.track.insert")?;
            let order = state.next_order;
            state.next_order = state.next_order.checked_add(1).ok_or_else(|| {
                DataError::simple(
                    DataErrorKind::Overflow,
                    "data.track.insert",
                    "source order exhausted",
                )
            })?;
            state.rows.push(row.clone());
            state.keys.push(candidate);
            state.orders.push(order);
            state.changes.push(DataTrackedChange {
                revision,
                order,
                kind: DataTrackedChangeKind::Insert { row },
            });
            Ok(())
        })
    }

    /// Replace the row identified by `key`. The identity key is immutable;
    /// grouping keys may still change when a query derives them from the row.
    pub fn replace(&self, key: K, row: T) -> Result<(), DataError> {
        self.update("data.track.replace", move |state, revision| {
            let Some(index) = state.keys.iter().position(|existing| existing == &key) else {
                return Err(DataError::simple(
                    DataErrorKind::MissingKey,
                    "data.track.replace",
                    "source key does not exist",
                ));
            };
            if (state.key)(row.clone()) != key {
                return Err(DataError::simple(
                    DataErrorKind::InvalidValue,
                    "data.track.replace",
                    "replacement row must retain its source key",
                ));
            }
            Self::ensure_change_capacity(state, "data.track.replace")?;
            let before = std::mem::replace(&mut state.rows[index], row.clone());
            let order = state.orders[index];
            state.changes.push(DataTrackedChange {
                revision,
                order,
                kind: DataTrackedChangeKind::Replace {
                    before,
                    after: row,
                },
            });
            Ok(())
        })
    }

    pub fn remove(&self, key: K) -> Result<(), DataError> {
        self.update("data.track.remove", move |state, revision| {
            let Some(index) = state.keys.iter().position(|existing| existing == &key) else {
                return Err(DataError::simple(
                    DataErrorKind::MissingKey,
                    "data.track.remove",
                    "source key does not exist",
                ));
            };
            Self::ensure_change_capacity(state, "data.track.remove")?;
            let row = state.rows.remove(index);
            state.keys.remove(index);
            let order = state.orders.remove(index);
            state.changes.push(DataTrackedChange {
                revision,
                order,
                kind: DataTrackedChangeKind::Remove { row },
            });
            Ok(())
        })
    }
}

impl<T: Clone + 'static, K: Clone + PartialEq + 'static> DataTrackedReader<T>
    for DataTracked<T, K>
{
    fn snapshot_rows(&self) -> Vec<T> {
        self.state.capture().value().rows
    }

    fn snapshot_entries(&self) -> Vec<(u64, T)> {
        self.snapshot_entries_at_revision().1
    }

    fn snapshot_entries_at_revision(&self) -> (u64, Vec<(u64, T)>) {
        let snapshot = self.state.capture_with(|state| {
            state
                .orders
                .iter()
                .copied()
                .zip(state.rows.iter().cloned())
                .collect()
        });
        (snapshot.revision(), snapshot.value())
    }

    fn changes_since(&self, revision: u64) -> Result<Vec<DataTrackedChange<T>>, DataError> {
        let current = self.state.revision();
        self.state.read(|state| {
            if revision > current {
                return Err(DataError::simple(
                    DataErrorKind::StaleRevision,
                    "data.query.watch",
                    "requested revision is newer than the source",
                ));
            }
            if let Some(first) = state.changes.first() {
                if revision.saturating_add(1) < first.revision {
                    return Err(DataError::simple(
                        DataErrorKind::StaleRevision,
                        "data.query.watch",
                        "requested revision is no longer retained",
                    ));
                }
            }
            Ok(state
                .changes
                .iter()
                .filter(|change| change.revision > revision)
                .cloned()
                .collect())
        })
    }

    fn revision(&self) -> u64 {
        self.state.revision()
    }

    fn retained_rows(&self) -> usize {
        self.state.read(|state| state.rows.len())
    }

    fn plan(&self, row_count: usize) -> crate::JetTablePlan {
        let schema = crate::JetTableSchema::new(
            crate::JetTableTypeId::new(std::any::type_name::<T>()),
            Vec::new(),
        );
        let source = crate::JetTableSource::new("data.track", schema).with_rows(row_count as u128);
        crate::JetTablePlan::from_source("data.track", source)
    }
}

impl<T: Clone + 'static, K: Clone + PartialEq + 'static> DataTrackedMeta
    for DataTracked<T, K>
{
    fn revision(&self) -> u64 {
        <Self as DataTrackedReader<T>>::revision(self)
    }

    fn retained_rows(&self) -> usize {
        <Self as DataTrackedReader<T>>::retained_rows(self)
    }
}

impl<T, K> super::JetShow for DataTracked<T, K> {
    fn jet_show(&self) -> String {
        "DataTracked(..)".to_string()
    }
}
impl<T, K> super::JetDisplay for DataTracked<T, K> {
    fn jet_display(&self) -> String {
        self.jet_show()
    }
}
impl<T, K> super::JetDebug for DataTracked<T, K> {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}

#[derive(Clone)]
pub(crate) struct DataWatchEntry<T> {
    pub(crate) order: u64,
    pub(crate) key: Option<String>,
    pub(crate) value: T,
}

pub(crate) struct DataWatchMaterialized<T> {
    pub(crate) entries: Vec<DataWatchEntry<T>>,
    pub(crate) revision: u64,
    pub(crate) mode: &'static str,
    pub(crate) recomputed: bool,
    pub(crate) changed: bool,
    pub(crate) retained_rows: usize,
}


pub(crate) trait DataWatchSource<T> {
    fn initial(&self) -> Result<DataWatchMaterialized<T>, DataError>;
    fn refresh(
        &self,
        revision: u64,
        prior: &[DataWatchEntry<T>],
    ) -> Result<DataWatchMaterialized<T>, DataError>;
}

/// A maintained source adapter for operations added after a derived watch
/// source. It retains the inner materialization so refreshes use the inner
/// source's canonical prior state rather than re-reading or reopening it.
struct DataWatchOperationSource<T> {
    inner: std::rc::Rc<dyn DataWatchSource<T>>,
    operation: DataQueryOperation<T>,
    base: std::cell::RefCell<Option<Vec<DataWatchEntry<T>>>>,
}

impl<T: Clone + 'static> DataWatchOperationSource<T> {
    fn apply_operation(
        &self,
        mut entries: Vec<DataWatchEntry<T>>,
        retained_rows: usize,
    ) -> Result<Vec<DataWatchEntry<T>>, DataError> {
        match &self.operation {
            DataQueryOperation::Filter(predicate) => {
                entries.retain(|entry| predicate(entry.value.clone()));
            }
            DataQueryOperation::SortBy(key) => {
                let limit = DataLimits::safe().max_sort_rows;
                if retained_rows as i64 > limit {
                    return Err(DataError::simple(
                        DataErrorKind::Limit,
                        "data.query.watch",
                        format!("max_sort_rows {limit} exceeded"),
                    ));
                }
                for entry in &mut entries {
                    entry.key = Some(key(entry.value.clone()));
                }
                entries.sort_by(|left, right| left.key.cmp(&right.key));
            }
        }
        Ok(entries)
    }
}

impl<T: Clone + 'static> DataWatchSource<T> for DataWatchOperationSource<T> {
    fn initial(&self) -> Result<DataWatchMaterialized<T>, DataError> {
        let materialized = self.inner.initial()?;
        let next_base = materialized.entries.clone();
        let entries = self.apply_operation(next_base.clone(), materialized.retained_rows)?;
        *self.base.borrow_mut() = Some(next_base);
        Ok(DataWatchMaterialized {
            entries,
            revision: materialized.revision,
            mode: materialized.mode,
            recomputed: materialized.recomputed,
            changed: materialized.changed,
            retained_rows: materialized.retained_rows,
        })

    }

    fn refresh(
        &self,
        revision: u64,
        prior: &[DataWatchEntry<T>],
    ) -> Result<DataWatchMaterialized<T>, DataError> {
        let base = self.base.borrow().clone().ok_or_else(|| {
            DataError::simple(
                DataErrorKind::State,
                "data.query.watch",
                "derived watch was not initialized",
            )
        })?;
        let materialized = self.inner.refresh(revision, &base)?;
        if !materialized.changed && materialized.revision == revision {
            return Ok(DataWatchMaterialized {
                entries: prior.to_vec(),
                revision: materialized.revision,
                mode: materialized.mode,
                recomputed: false,
                changed: false,
                retained_rows: materialized.retained_rows,
            });
        }
        let next_base = materialized.entries.clone();
        let entries = self.apply_operation(next_base.clone(), materialized.retained_rows)?;
        *self.base.borrow_mut() = Some(next_base);
        Ok(DataWatchMaterialized {
            entries,
            revision: materialized.revision,
            mode: materialized.mode,
            recomputed: materialized.recomputed,
            changed: materialized.changed,
            retained_rows: materialized.retained_rows,
        })
    }
}


struct DataTrackedWatchSource<T> {
    source: std::rc::Rc<dyn DataTrackedReader<T>>,
    operations: Vec<DataQueryOperation<T>>,
}

impl<T: Clone + 'static> DataTrackedWatchSource<T> {
    fn accepts(
        operations: &[DataQueryOperation<T>],
        row: &T,
    ) -> (bool, Option<String>) {
        let mut sort_key = None;
        for operation in operations {
            match operation {
                DataQueryOperation::Filter(predicate) => {
                    if !(predicate)(row.clone()) {
                        return (false, None);
                    }
                }
                DataQueryOperation::SortBy(key) => {
                    sort_key = Some(key(row.clone()));
                }
            }
        }
        (true, sort_key)
    }

    fn insert_entry(entries: &mut Vec<DataWatchEntry<T>>, entry: DataWatchEntry<T>) {
        let position = entries
            .iter()
            .position(|existing| match (&entry.key, &existing.key) {
                (Some(left), Some(right)) => {
                    left.cmp(right) == std::cmp::Ordering::Less
                        || (left == right && entry.order < existing.order)
                }
                _ => entry.order < existing.order,
            })
            .unwrap_or(entries.len());
        entries.insert(position, entry);
    }

    fn entries_from_snapshot(
        &self,
        rows: Vec<(u64, T)>,
    ) -> Vec<DataWatchEntry<T>> {
        let mut entries = Vec::new();
        for (order, value) in rows {
            let (accepted, key) = Self::accepts(&self.operations, &value);
            if accepted {
                Self::insert_entry(&mut entries, DataWatchEntry { order, key, value });
            }
        }
        entries
    }
    fn check_sort_limit(&self) -> Result<(), DataError> {
        if self
            .operations
            .iter()
            .any(|operation| matches!(operation, DataQueryOperation::SortBy(_)))
        {
            let limit = DataLimits::safe().max_sort_rows;
            if self.source.retained_rows() as i64 > limit {
                return Err(DataError::simple(
                    DataErrorKind::Limit,
                    "data.query.watch",
                    format!("max_sort_rows {limit} exceeded"),
                ));
            }
        }
        Ok(())
    }

    fn apply_change(
        &self,
        entries: &mut Vec<DataWatchEntry<T>>,
        change: DataTrackedChange<T>,
    ) -> Result<(), DataError> {
        let index = entries.iter().position(|entry| entry.order == change.order);
        match change.kind {
            DataTrackedChangeKind::Insert { row } => {
                let (accepted, key) = Self::accepts(&self.operations, &row);
                if accepted {
                    Self::insert_entry(entries, DataWatchEntry { order: change.order, key, value: row });
                }
            }
            DataTrackedChangeKind::Replace { after, .. } => {
                if let Some(index) = index {
                    entries.remove(index);
                }
                let (accepted, key) = Self::accepts(&self.operations, &after);
                if accepted {
                    Self::insert_entry(entries, DataWatchEntry {
                        order: change.order,
                        key,
                        value: after,
                    });
                }
            }
            DataTrackedChangeKind::Remove { .. } => {
                if let Some(index) = index {
                    entries.remove(index);
                }
            }
        }
        Ok(())
    }
}

impl<T: Clone + 'static> DataWatchSource<T> for DataTrackedWatchSource<T> {
    fn initial(&self) -> Result<DataWatchMaterialized<T>, DataError> {
        self.check_sort_limit()?;
        let (revision, entries) = self.source.snapshot_entries_at_revision();
        let entries = self.entries_from_snapshot(entries);
        Ok(DataWatchMaterialized {
            entries,
            revision,
            mode: "incremental",
            recomputed: false,
            changed: true,
            retained_rows: self.source.retained_rows(),
        })
    }

    fn refresh(
        &self,
        revision: u64,
        prior: &[DataWatchEntry<T>],
    ) -> Result<DataWatchMaterialized<T>, DataError> {
        self.check_sort_limit()?;
        let changes = self.source.changes_since(revision)?;
        let mut entries = prior.to_vec();
        for change in changes {
            self.apply_change(&mut entries, change)?;
        }
        Ok(DataWatchMaterialized {
            entries,
            revision: self.source.revision(),
            mode: "incremental",
            recomputed: false,
            changed: self.source.revision() != revision,
            retained_rows: self.source.retained_rows(),
        })
    }
}

#[derive(Clone)]
struct DataWatchState<T> {
    entries: Vec<DataWatchEntry<T>>,
    source_revision: u64,
    mode: String,
    retained_rows: usize,
    recomputations: u64,
    lifecycle: super::JetLiveLifecycle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataWatchStatus {
    pub mode: String,
    pub revision: i64,
    pub retained_rows: i64,
    pub recomputations: i64,
    pub active: bool,
    pub generation: i64,
    pub dirty: bool,
    pub error: String,
    pub freshness_ms: i64,
    pub invalidation_cause: String,
    pub refreshing: bool,
    pub cancelled: bool,
}

impl super::JetShow for DataWatchStatus {
    fn jet_show(&self) -> String {
        format!(
            "DataWatchStatus{{mode: {}, revision: {}, retained_rows: {}, recomputations: {}, active: {}, generation: {}, dirty: {}, error: {}, freshness_ms: {}, invalidation_cause: {}, refreshing: {}, cancelled: {}}}",
            self.mode,
            self.revision,
            self.retained_rows,
            self.recomputations,
            self.active,
            self.generation,
            self.dirty,
            self.error,
            self.freshness_ms,
            self.invalidation_cause,
            self.refreshing,
            self.cancelled
        )
    }
}
impl super::JetDisplay for DataWatchStatus {
    fn jet_display(&self) -> String {
        self.jet_show()
    }
}
impl super::JetDebug for DataWatchStatus {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}
pub struct DataWatch<T> {
    pub(crate) query: DataQuery<T>,
    source: std::rc::Rc<dyn DataWatchSource<T>>,
    state: std::rc::Rc<JetShared<DataWatchState<T>>>,
}

impl<T> Clone for DataWatch<T> {
    fn clone(&self) -> Self {
        Self {
            query: self.query.clone(),
            source: self.source.clone(),
            state: self.state.clone(),
        }
    }
}


impl<T: Clone + 'static> DataWatch<T> {
    pub(crate) fn from_query(query: DataQuery<T>) -> Result<Self, DataError> {
        let source = query.maintained_watch_source().ok_or_else(|| {
            DataError::simple(
                DataErrorKind::Unsupported,
                "data.query.watch",
                "E2476: watch requires a maintained data.track source",
            )
        })?;
        let initial = source.initial()?;
        if initial.entries.len() as i64 > DataLimits::safe().max_output_rows {
            return Err(DataError::simple(
                DataErrorKind::Limit,
                "data.query.watch",
                "maintained result exceeds max_output_rows",
            ));
        }
        let state = DataWatchState {
            entries: initial.entries,
            source_revision: initial.revision,
            mode: initial.mode.to_string(),
            retained_rows: initial.retained_rows,
            recomputations: u64::from(initial.recomputed),
            lifecycle: super::JetLiveLifecycle::active_now(),
        };
        Ok(Self {
            query,
            source,
            state: std::rc::Rc::new(JetShared::new(state)),
        })
    }

    pub(crate) fn is_active(&self) -> bool {
        self.state.read(|state| state.lifecycle.active)
    }

    pub(crate) fn cancel(&self) {
        self.state.edit(|state| {
            let _ = state.lifecycle.cancel();
        });
    }

    pub(crate) fn get(&self) -> Result<Vec<T>, DataError> {
        if !self.is_active() {
            return Err(DataError::simple(
                DataErrorKind::State,
                "data.watch.get",
                "watch is cancelled",
            ));
        }
        let snapshot = self.state.capture();
        let current = snapshot.value();
        let generation = current.lifecycle.generation;
        let mut lifecycle = current.lifecycle.clone();
        if !lifecycle.begin_refresh(generation) {
            return Err(DataError::simple(
                DataErrorKind::State,
                "data.watch.get",
                "watch is cancelled",
            ));
        }
        let update = match self.source.refresh(current.source_revision, &current.entries) {
            Ok(update) => update,
            Err(error) => {
                let _ = lifecycle.fail(generation, error.to_string());
                let failed = DataWatchState {
                    entries: current.entries.clone(),
                    source_revision: current.source_revision,
                    mode: current.mode.clone(),
                    retained_rows: current.retained_rows,
                    recomputations: current.recomputations,
                    lifecycle,
                };
                let _ = self.state.try_replace(snapshot, failed);
                return Err(error);
            }
        };
        if update.entries.len() as i64 > DataLimits::safe().max_output_rows {
            let error = DataError::simple(
                DataErrorKind::Limit,
                "data.watch.get",
                "maintained result exceeds max_output_rows",
            );
            let _ = lifecycle.fail(generation, error.to_string());
            let failed = DataWatchState {
                entries: current.entries.clone(),
                source_revision: current.source_revision,
                mode: current.mode.clone(),
                retained_rows: current.retained_rows,
                recomputations: current.recomputations,
                lifecycle,
            };
            let _ = self.state.try_replace(snapshot, failed);
            return Err(error);
        }
        let _ = lifecycle.publish(generation, super::JetLiveLifecycle::active_now().fresh_at_ms);
        let next = DataWatchState {
            entries: update.entries,
            source_revision: update.revision,
            mode: update.mode.to_string(),
            retained_rows: update.retained_rows,
            recomputations: current
                .recomputations
                .saturating_add(u64::from(update.recomputed)),
            lifecycle,
        };
        if !self.is_active() {
            return Err(DataError::simple(
                DataErrorKind::State,
                "data.watch.get",
                "watch was cancelled before publication",
            ));
        }
        match self.state.try_replace(snapshot, next) {
            Ok(true) => Ok(self.state.read(|state| {
                state
                    .entries
                    .iter()
                    .map(|entry| entry.value.clone())
                    .collect()
            })),
            Ok(false) => Err(DataError::simple(
                DataErrorKind::StaleRevision,
                "data.watch.get",
                "watch state changed before publication",
            )),
            Err(JetSharedRevisionError::WrongOwner) => Err(DataError::simple(
                DataErrorKind::WrongOwner,
                "data.watch.get",
                "watch snapshot belongs to another owner",
            )),
            Err(JetSharedRevisionError::GenerationExhausted) => Err(DataError::simple(
                DataErrorKind::Overflow,
                "data.watch.get",
                "watch publication revision exhausted",
            )),
        }
    }

    /// Compatibility hook for callers that only need to advance publication.
    pub(crate) fn note_refresh(&self) {
        let _ = self.get();
    }

    pub(crate) fn status(&self) -> DataWatchStatus {
        self.state.read(|state| DataWatchStatus {
            mode: state.mode.clone(),
            revision: state.source_revision.min(i64::MAX as u64) as i64,
            retained_rows: state.retained_rows.min(i64::MAX as usize) as i64,
            recomputations: state.recomputations.min(i64::MAX as u64) as i64,
            active: state.lifecycle.active,
            generation: state.lifecycle.generation.min(i64::MAX as u64) as i64,
            dirty: state.lifecycle.dirty,
            error: state.lifecycle.error.clone(),
            freshness_ms: state.lifecycle.freshness_ms().min(i64::MAX as u64) as i64,
            invalidation_cause: state.lifecycle.invalidation_cause.clone(),
            refreshing: state.lifecycle.refreshing,
            cancelled: state.lifecycle.cancelled,
        })
    }
}

impl<T: Clone + 'static> super::JetShow for DataWatch<T> {
    fn jet_show(&self) -> String {
        self.status().jet_show()
    }
}
impl<T: Clone + 'static> super::JetDisplay for DataWatch<T> {
    fn jet_display(&self) -> String {
        self.jet_show()
    }
}
impl<T: Clone + 'static> super::JetDebug for DataWatch<T> {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}

impl<T> Clone for DataQueryOperation<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Filter(predicate) => Self::Filter(predicate.clone()),
            Self::SortBy(key) => Self::SortBy(key.clone()),
        }
    }
}


/// `Rows` is a reusable in-memory plan; `Computed` is a single-use producer
/// (a one-shot `DataStream` reader or a grouped reducer). `consumed` is
/// shared by every query derived from the same source, so a second collect
/// reports a `State` error instead of silently reopening or replaying.
enum DataQuerySource<T> {
    Rows(DataQueryRows<T>),
    Tracked(std::rc::Rc<dyn DataTrackedReader<T>>),
    Computed(
        std::rc::Rc<
            std::cell::RefCell<Option<Box<dyn FnOnce() -> Result<Vec<T>, DataError>>>>,
        >,
    ),
    Recomputed(std::rc::Rc<dyn Fn() -> Result<Vec<T>, DataError>>),
}
impl<T> Clone for DataQuerySource<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Rows(rows) => Self::Rows(rows.clone()),
            Self::Tracked(source) => Self::Tracked(source.clone()),
            Self::Computed(compute) => Self::Computed(compute.clone()),
            Self::Recomputed(compute) => Self::Recomputed(compute.clone()),
        }
    }
}


/// Logical step names shared by every tier's `Query.plan()`: the source
/// (`scan` for a reusable list, `stream` for a one-shot reader), then each
/// deferred operation by its public spelling (`filter`, `sort_by`, `sql`,
/// `group_by`, `count`, `sum`, `mean`).
pub struct DataQuery<T> {
    source: DataQuerySource<T>,
    pub(crate) operations: Vec<DataQueryOperation<T>>,
    pub(crate) steps: Vec<String>,
    pub(crate) tracking: Option<std::rc::Rc<dyn DataTrackedMeta>>,
    pub(crate) watch_source: Option<std::rc::Rc<dyn DataWatchSource<T>>>,
    consumed: std::rc::Rc<std::cell::Cell<bool>>,
}
impl<T> Clone for DataQuery<T> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            operations: self.operations.clone(),
            steps: self.steps.clone(),
            tracking: self.tracking.clone(),
            watch_source: self.watch_source.clone(),
            consumed: self.consumed.clone(),
        }
    }
}


impl<T> DataQuery<T> {
    pub fn from_rows(rows: Vec<T>, missing: i64, plan: crate::JetTablePlan) -> Self {
        Self {
            source: DataQuerySource::Rows(DataQueryRows::from_rows(rows, missing, plan)),
            operations: Vec::new(),
            steps: vec!["scan".to_string()],
            tracking: None,
            watch_source: None,
            consumed: std::rc::Rc::new(std::cell::Cell::new(false)),
        }
    }

    pub fn from_tracked<K>(tracked: &DataTracked<T, K>) -> Self
    where
        T: Clone + 'static,
        K: Clone + PartialEq + 'static,
    {
        Self {
            source: DataQuerySource::Tracked(std::rc::Rc::new(tracked.clone())),
            operations: Vec::new(),
            steps: vec!["track".to_string()],
            tracking: Some(std::rc::Rc::new(tracked.clone())),
            watch_source: None,
            consumed: std::rc::Rc::new(std::cell::Cell::new(false)),
        }
    }

    /// Append one logical step name without changing deferred operations.
    pub fn with_step(mut self, step: &str) -> Self {
        self.steps.push(step.to_string());
        self
    }

    pub fn plan(&self) -> Vec<String> {
        self.steps.clone()
    }

    pub fn filter<F>(&self, predicate: F) -> Self
    where
        T: Clone + 'static,
        F: Fn(T) -> bool + 'static,
    {
        let operation =
            DataQueryOperation::Filter(std::sync::Arc::new(predicate));
        let mut operations = self.operations.clone();
        operations.push(operation.clone());
        let mut steps = self.steps.clone();
        steps.push("filter".to_string());
        let watch_source = self.watch_source.as_ref().map(|source| {
            std::rc::Rc::new(DataWatchOperationSource {
                inner: source.clone(),
                operation,
                base: std::cell::RefCell::new(None),
            }) as std::rc::Rc<dyn DataWatchSource<T>>
        });
        Self {
            source: self.source.clone(),
            operations,
            steps,
            tracking: self.tracking.clone(),
            watch_source,
            consumed: self.consumed.clone(),
        }
    }

    pub fn sort_by<F>(&self, key: F) -> Self
    where
        T: Clone + 'static,
        F: Fn(T) -> String + 'static,
    {
        let operation =
            DataQueryOperation::SortBy(std::sync::Arc::new(key));
        let mut operations = self.operations.clone();
        operations.push(operation.clone());
        let mut steps = self.steps.clone();
        steps.push("sort_by".to_string());
        let watch_source = self.watch_source.as_ref().map(|source| {
            std::rc::Rc::new(DataWatchOperationSource {
                inner: source.clone(),
                operation,
                base: std::cell::RefCell::new(None),
            }) as std::rc::Rc<dyn DataWatchSource<T>>
        });
        Self {
            source: self.source.clone(),
            operations,
            steps,
            tracking: self.tracking.clone(),
            watch_source,
            consumed: self.consumed.clone(),
        }
    }

    pub(crate) fn rows_frame(&self) -> Option<DataQueryRows<T>>
    where
        T: Clone + 'static,
    {
        let mut rows = match &self.source {
            DataQuerySource::Rows(rows) => rows.clone(),
            DataQuerySource::Tracked(source) => {
                let values = source.snapshot_rows();
                DataQueryRows::from_rows(values.clone(), 0, source.plan(values.len()))
            }
            DataQuerySource::Computed(_) | DataQuerySource::Recomputed(_) => return None,
        };
        for operation in &self.operations {
            match operation {
                DataQueryOperation::Filter(predicate) => {
                    let predicate = predicate.clone();
                    rows = rows.filter(move |row| predicate(row));
                }
                DataQueryOperation::SortBy(key) => {
                    let key = key.clone();
                    rows = rows.sort_by(move |row| key(row));
                }
            }
        }
        Some(rows)
    }

    pub(crate) fn is_computed(&self) -> bool {
        matches!(&self.source, DataQuerySource::Computed(_))
    }

    pub(crate) fn is_recomputed(&self) -> bool {
        matches!(&self.source, DataQuerySource::Recomputed(_))
    }

    pub(crate) fn recomputation(&self) -> Option<Result<Vec<T>, DataError>> {
        let DataQuerySource::Recomputed(compute) = &self.source else {
            return None;
        };
        Some(compute())
    }

    /// A derived query whose rows are produced by one deferred computation.
    pub(crate) fn from_computation<F>(steps: Vec<String>, compute: F) -> Self
    where
        F: FnOnce() -> Result<Vec<T>, DataError> + 'static,
    {
        Self {
            source: DataQuerySource::Computed(std::rc::Rc::new(std::cell::RefCell::new(
                Some(Box::new(compute)),
            ))),
            operations: Vec::new(),
            steps,
            tracking: None,
            watch_source: None,
            consumed: std::rc::Rc::new(std::cell::Cell::new(false)),
        }
    }

    pub(crate) fn from_recomputation<F>(
        steps: Vec<String>,
        tracking: std::rc::Rc<dyn DataTrackedMeta>,
        compute: F,
    ) -> Self
    where
        F: Fn() -> Result<Vec<T>, DataError> + 'static,
    {
        Self {
            source: DataQuerySource::Recomputed(std::rc::Rc::new(compute)),
            operations: Vec::new(),
            steps,
            tracking: Some(tracking),
            watch_source: None,
            consumed: std::rc::Rc::new(std::cell::Cell::new(false)),
        }
    }
    pub(crate) fn with_watch_source(
        mut self,
        source: std::rc::Rc<dyn DataWatchSource<T>>,
    ) -> Self {
        self.watch_source = Some(source);
        self
    }

    pub(crate) fn tracked_reader(&self) -> Option<std::rc::Rc<dyn DataTrackedReader<T>>> {
        match &self.source {
            DataQuerySource::Tracked(source) => Some(source.clone()),
            _ => None,
        }
    }

    pub(crate) fn maintained_watch_source(
        &self,
    ) -> Option<std::rc::Rc<dyn DataWatchSource<T>>>
    where
        T: Clone + 'static,
    {
        if let Some(source) = &self.watch_source {
            return Some(source.clone());
        }
        self.tracked_reader().map(|source| {
            std::rc::Rc::new(DataTrackedWatchSource {
                source,
                operations: self.operations.clone(),
            }) as std::rc::Rc<dyn DataWatchSource<T>>
        })
    }

    pub(crate) fn take_computation(
        &self,
    ) -> Option<Box<dyn FnOnce() -> Result<Vec<T>, DataError>>> {
        if !self.is_computed() || self.consumed.replace(true) {
            return None;
        }
        let DataQuerySource::Computed(compute) = &self.source else {
            return None;
        };
        compute.borrow_mut().take()
    }
}

/// D-QUERY-RETAIN1=A: a grouped query keeps the key callback and source plan
/// deferred until a reducer is consumed.
#[derive(Clone)]
pub struct DataGroupedQuery<T, K> {
    pub(crate) query: DataQuery<T>,
    pub(crate) key: std::sync::Arc<dyn Fn(T) -> K>,
}


/// Pull executor for Query's private reusable list source. `SortBy` owns its
/// bounded materialization only on the first pull; source and filter nodes stay
/// streaming and retain no output buffer.
pub(crate) struct DataQueryCursor<T> {
    state: DataQueryCursorState<T>,
}

enum DataQueryCursorState<T> {
    Scan {
        rows: std::sync::Arc<Vec<T>>,
        index: usize,
    },
    Filter {
        input: Box<DataQueryCursor<T>>,
        predicate: std::sync::Arc<dyn Fn(T) -> bool>,
    },
    SortBy {
        input: Box<DataQueryCursor<T>>,
        key: std::sync::Arc<dyn Fn(T) -> String>,
        rows: Option<Vec<T>>,
        index: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DataQueryPlanLimit {
    pub(crate) operation: &'static str,
    pub(crate) limit: i64,
}

impl<T: Clone> DataQueryRows<T> {
    pub(crate) fn cursor(&self) -> DataQueryCursor<T> {
        DataQueryCursor::from_plan(self.root.clone())
    }
}

impl<T: Clone> DataQueryCursor<T> {
    fn from_plan(plan: std::sync::Arc<DataQueryPlan<T>>) -> Self {
        let state = match plan.as_ref() {
            DataQueryPlan::Scan { rows } => DataQueryCursorState::Scan {
                rows: rows.clone(),
                index: 0,
            },
            DataQueryPlan::Filter { input, predicate } => DataQueryCursorState::Filter {
                input: Box::new(Self::from_plan(input.clone())),
                predicate: predicate.clone(),
            },
            DataQueryPlan::SortBy { input, key } => DataQueryCursorState::SortBy {
                input: Box::new(Self::from_plan(input.clone())),
                key: key.clone(),
                rows: None,
                index: 0,
            },
        };
        Self { state }
    }

    pub(crate) fn next(&mut self) -> Option<T> {
        self.next_with_sort_limit(None).ok().flatten()
    }

    pub(crate) fn next_batch(&mut self, batch_size: usize) -> Vec<T> {
        self.next_batch_with_sort_limit(batch_size, None)
            .unwrap_or_default()
    }

    pub(crate) fn next_batch_with_sort_limit(
        &mut self,
        batch_size: usize,
        max_sort_rows: Option<i64>,
    ) -> Result<Vec<T>, DataQueryPlanLimit> {
        let mut batch = Vec::with_capacity(batch_size);
        for _ in 0..batch_size {
            let Some(row) = self.next_with_sort_limit(max_sort_rows)? else {
                break;
            };
            batch.push(row);
        }
        Ok(batch)
    }

    fn next_with_sort_limit(
        &mut self,
        max_sort_rows: Option<i64>,
    ) -> Result<Option<T>, DataQueryPlanLimit> {
        match &mut self.state {
            DataQueryCursorState::Scan { rows, index } => {
                if *index >= rows.len() {
                    return Ok(None);
                }
                let row = rows[*index].clone();
                *index += 1;
                Ok(Some(row))
            }
            DataQueryCursorState::Filter { input, predicate } => loop {
                let Some(row) = input.next_with_sort_limit(max_sort_rows)? else {
                    return Ok(None);
                };
                if predicate(row.clone()) {
                    return Ok(Some(row));
                }
            },
            DataQueryCursorState::SortBy {
                input,
                key,
                rows,
                index,
            } => {
                if rows.is_none() {
                    let mut keyed = Vec::new();
                    loop {
                        let Some(row) = input.next_with_sort_limit(max_sort_rows)? else {
                            break;
                        };
                        if max_sort_rows.is_some_and(|limit| {
                            limit < 1
                                || i64::try_from(keyed.len()).unwrap_or(i64::MAX) >= limit
                        }) {
                            return Err(DataQueryPlanLimit {
                                operation: "sort_by",
                                limit: max_sort_rows.unwrap_or(0),
                            });
                        }
                        keyed.push((key(row.clone()), row));
                    }
                    keyed.sort_by(|left, right| left.0.cmp(&right.0));
                    *rows = Some(keyed.into_iter().map(|(_, row)| row).collect());
                }
                let sorted = rows.as_ref().expect("sort rows initialized");
                if *index >= sorted.len() {
                    return Ok(None);
                }
                let row = sorted[*index].clone();
                *index += 1;
                Ok(Some(row))
            }
        }
    }
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
        ClockState::System { started, offset_ms } => offset_ms
            .saturating_add(i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)),
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

    #[inline]
    pub fn as_nanos(self) -> i64 {
        self.ns
    }
}
// A duration renders as whole nanoseconds with its unit, the same text the
// TIR evaluator and the JIT print, so `print("{1d}")` reads alike on every
// tier (I9). Without this, AOT emitted `.jet_display()` on a type that had
// no such method and rustc rejected the generated program (I2).
impl super::JetDisplay for Duration {
    fn jet_display(&self) -> String {
        super::jet_duration_kernel_show(self.ns)
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

fn jet_limb_len(limbs: &[u32]) -> usize {
    let mut length = limbs.len();
    while length > 1 && limbs[length - 1] == 0 {
        length -= 1;
    }
    length
}

fn jet_limb_is_zero(limbs: &[u32]) -> bool {
    jet_limb_len(limbs) == 1 && limbs.first().copied().unwrap_or(0) == 0
}

fn jet_limb_decimal_digits(limbs: &[u32]) -> usize {
    let length = jet_limb_len(limbs);
    if length == 0 || limbs[0] == 0 && length == 1 {
        return 1;
    }
    let mut top = limbs[length - 1];
    let mut digits = 1usize;
    while top >= 10 {
        top /= 10;
        digits += 1;
    }
    digits + length.saturating_sub(1) * 9
}

fn jet_limb_decimal_prefix_f64(limbs: &[u32], digits: usize) -> f64 {
    if jet_limb_is_zero(limbs) {
        return 0.0;
    }
    let total = jet_limb_decimal_digits(limbs);
    let take = digits.min(total);
    let top_index = jet_limb_len(limbs) - 1;
    let mut top_width = 1usize;
    let mut width_probe = limbs[top_index];
    while width_probe >= 10 {
        width_probe /= 10;
        top_width += 1;
    }
    let mut prefix = 0.0;
    let mut consumed = 0usize;
    for index in (0..=top_index).rev() {
        if consumed == take {
            break;
        }
        let limb = limbs[index];
        let width = if index == top_index { top_width } else { 9 };
        let need = take - consumed;
        if need >= width {
            prefix = prefix * 10f64.powi(width as i32) + limb as f64;
            consumed += width;
        } else {
            let divisor = 10u32.pow((width - need) as u32);
            prefix = prefix * 10f64.powi(need as i32) + f64::from(limb / divisor);
            break;
        }
    }
    prefix
}

fn jet_limb_cmp(left: &[u32], right: &[u32]) -> std::cmp::Ordering {
    let left_len = jet_limb_len(left);
    let right_len = jet_limb_len(right);
    match left_len.cmp(&right_len) {
        std::cmp::Ordering::Equal => {
            for index in (0..left_len).rev() {
                match left[index].cmp(&right[index]) {
                    std::cmp::Ordering::Equal => {}
                    ordering => return ordering,
                }
            }
            std::cmp::Ordering::Equal
        }
        ordering => ordering,
    }
}

fn jet_limb_shift_left(limbs: &mut Vec<u32>, bits: usize) {
    limbs.reserve(bits.saturating_div(28).saturating_add(1));
    for _ in 0..bits {
        let mut carry = 0u64;
        for limb in limbs.iter_mut() {
            let value = u64::from(*limb) * 2 + carry;
            *limb = (value % BI_BASE) as u32;
            carry = value / BI_BASE;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
    }
}

fn jet_limb_double(limbs: &mut Vec<u32>) {
    let mut carry = 0u64;
    for limb in limbs.iter_mut() {
        let value = u64::from(*limb) * 2 + carry;
        *limb = (value % BI_BASE) as u32;
        carry = value / BI_BASE;
    }
    if carry != 0 {
        limbs.push(carry as u32);
    }
}

fn jet_limb_sub_assign(left: &mut Vec<u32>, right: &[u32]) {
    debug_assert!(jet_limb_cmp(left, right) != std::cmp::Ordering::Less);
    let mut borrow = 0i64;
    for index in 0..left.len() {
        let subtrahend = i64::from(right.get(index).copied().unwrap_or(0)) + borrow;
        let difference = i64::from(left[index]) - subtrahend;
        if difference < 0 {
            left[index] = (difference + BI_BASE as i64) as u32;
            borrow = 1;
        } else {
            left[index] = difference as u32;
            borrow = 0;
        }
    }
    debug_assert_eq!(borrow, 0);
    while left.len() > 1 && left.last() == Some(&0) {
        left.pop();
    }
}

fn jet_ratio_cmp_pow2(
    numerator: &[u32],
    denominator: &[u32],
    exponent: i64,
) -> std::cmp::Ordering {
    let mut shifted = if exponent >= 0 {
        denominator.to_vec()
    } else {
        numerator.to_vec()
    };
    let bits = usize::try_from(exponent.unsigned_abs()).unwrap_or(usize::MAX);
    jet_limb_shift_left(&mut shifted, bits);
    if exponent >= 0 {
        jet_limb_cmp(numerator, &shifted)
    } else {
        jet_limb_cmp(&shifted, denominator)
    }
}

fn jet_ratio_exponent(numerator: &[u32], denominator: &[u32]) -> i64 {
    let numerator_digits = jet_limb_decimal_digits(numerator);
    let denominator_digits = jet_limb_decimal_digits(denominator);
    let numerator_take = numerator_digits.min(17);
    let denominator_take = denominator_digits.min(17);
    let log2_ten = 10f64.log2();
    let numerator_log =
        (numerator_digits - numerator_take) as f64 * log2_ten
            + jet_limb_decimal_prefix_f64(numerator, numerator_take).log2();
    let denominator_log =
        (denominator_digits - denominator_take) as f64 * log2_ten
            + jet_limb_decimal_prefix_f64(denominator, denominator_take).log2();
    (numerator_log - denominator_log).floor() as i64
}

fn jet_ratio_inf(negative: bool) -> f64 {
    if negative {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    }
}

fn jet_ratio_zero(negative: bool) -> f64 {
    if negative {
        f64::from_bits(1u64 << 63)
    } else {
        0.0
    }
}

fn jet_ratio_limbs_to_f64(
    numerator: &[u32],
    denominator: &[u32],
    negative: bool,
) -> f64 {
    if jet_limb_is_zero(numerator) {
        return 0.0;
    }
    debug_assert!(!jet_limb_is_zero(denominator));

    let numerator_digits = jet_limb_decimal_digits(numerator);
    let denominator_digits = jet_limb_decimal_digits(denominator);
    let decimal_delta = numerator_digits as i64 - denominator_digits as i64;
    if decimal_delta > 400 {
        return jet_ratio_inf(negative);
    }
    if decimal_delta < -400 {
        return jet_ratio_zero(negative);
    }

    let mut exponent = jet_ratio_exponent(numerator, denominator);
    loop {
        if jet_ratio_cmp_pow2(numerator, denominator, exponent)
            == std::cmp::Ordering::Less
        {
            exponent -= 1;
            continue;
        }
        if jet_ratio_cmp_pow2(numerator, denominator, exponent + 1)
            != std::cmp::Ordering::Less
        {
            exponent += 1;
            continue;
        }
        break;
    }

    if exponent > 1023 {
        return jet_ratio_inf(negative);
    }
    if exponent < -1075 {
        return jet_ratio_zero(negative);
    }
    if exponent == -1075 {
        let mut scaled = numerator.to_vec();
        jet_limb_shift_left(&mut scaled, 1074);
        jet_limb_double(&mut scaled);
        let magnitude = if jet_limb_cmp(&scaled, denominator) == std::cmp::Ordering::Greater {
            f64::from_bits(1)
        } else {
            0.0
        };
        return if negative { -magnitude } else { magnitude };
    }

    let mut divisor_storage = Vec::new();
    let mut remainder = if exponent >= 0 {
        if exponent == 0 {
            numerator.to_vec()
        } else {
            divisor_storage.extend_from_slice(denominator);
            jet_limb_shift_left(&mut divisor_storage, exponent as usize);
            numerator.to_vec()
        }
    } else {
        let mut scaled = numerator.to_vec();
        jet_limb_shift_left(&mut scaled, exponent.unsigned_abs() as usize);
        scaled
    };
    let divisor: &[u32] = if exponent > 0 {
        &divisor_storage
    } else {
        denominator
    };
    jet_limb_sub_assign(&mut remainder, divisor);

    let mut significand = 1u64 << 52;
    for shift in (0..52).rev() {
        jet_limb_double(&mut remainder);
        if jet_limb_cmp(&remainder, divisor) != std::cmp::Ordering::Less {
            jet_limb_sub_assign(&mut remainder, divisor);
            significand |= 1u64 << shift;
        }
    }
    jet_limb_double(&mut remainder);
    let guard = jet_limb_cmp(&remainder, divisor) != std::cmp::Ordering::Less;
    if guard {
        jet_limb_sub_assign(&mut remainder, divisor);
    }
    let sticky = !jet_limb_is_zero(&remainder);
    if guard && (sticky || (significand & 1) != 0) {
        significand += 1;
        if significand == 1u64 << 53 {
            significand >>= 1;
            exponent += 1;
        }
    }
    if exponent > 1023 {
        return jet_ratio_inf(negative);
    }
    let bits = ((exponent + 1023) as u64) << 52
        | (significand & ((1u64 << 52) - 1));
    let bits = if negative {
        bits | (1u64 << 63)
    } else {
        bits
    };
    f64::from_bits(bits)
}

fn jet_decimal_power10_limbs(scale: u32) -> Vec<u32> {
    let groups = usize::try_from(scale / 9).unwrap_or(usize::MAX);
    let remainder = scale % 9;
    let mut limbs = vec![0u32; groups.saturating_add(1)];
    limbs[groups] = 10u32.pow(remainder);
    limbs
}

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

    pub fn from_radix(text: &str, radix: u32) -> Result<Self, String> {
        if !(2..=36).contains(&radix) {
            return Err(format!(
                "integer radix must be between 2 and 36, got {radix}"
            ));
        }
        let text = text.trim();
        let (negative, digits) = if let Some(rest) = text.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix('+') {
            (false, rest)
        } else {
            (false, text)
        };
        if digits.is_empty() {
            return Err("integer radix text is empty".to_string());
        }
        let mut value = Self::from_int(0);
        for digit in digits.chars() {
            let digit = digit
                .to_digit(radix)
                .ok_or_else(|| format!("invalid base-{radix} integer `{text}`"))?;
            value = value.mul_small(radix).add_small(digit);
        }
        value.negative = negative && !value.is_zero();
        Ok(value)
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
        let significant = self.bit_width();
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
        crate::jet_numeric_checked_widen_parts(
            significant,
            trailing,
            self.to_f64(),
            target_f32,
        )
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
        i64::try_from(self.decimal_digit_count()).unwrap_or(i64::MAX)
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
            let (_, remainder) = a.div_rem(&b).expect("gcd divisor is nonzero");
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
        let quotient = left.abs().div_rem(&divisor).expect("lcm gcd is nonzero").0;
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
            result = result.mul(&numerator).div_rem(&index)?.0;
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
    /// Project the exact integer into binary64 without allocating its decimal
    /// spelling. Overflow follows the ordinary integer-to-float rule and
    /// produces the matching signed infinity.
    pub fn to_f64(&self) -> f64 {
        if self.is_zero() {
            return 0.0;
        }
        if let Some(value) = self.try_i128() {
            return value as f64;
        }
        if self.decimal_digit_count() > 309 {
            return if self.negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        }

        // A finite binary64 value has at most 309 decimal digits. Copy the
        // bounded magnitude to the stack, then extract bits from low to high.
        let mut limbs = [0u32; 35];
        let mut limb_count = self.limbs.len();
        limbs[..limb_count].copy_from_slice(&self.limbs);
        let mut top = 0u64;
        let mut bit_index = 0usize;
        let mut guard = false;
        let mut sticky = false;
        while limb_count > 1 || limbs[0] != 0 {
            let mut remainder = 0u64;
            for index in (0..limb_count).rev() {
                let current = remainder * BI_BASE + u64::from(limbs[index]);
                limbs[index] = (current / 2) as u32;
                remainder = current % 2;
            }
            while limb_count > 1 && limbs[limb_count - 1] == 0 {
                limb_count -= 1;
            }
            let bit = remainder != 0;
            if bit_index < 53 {
                top |= u64::from(bit) << bit_index;
            } else {
                sticky |= guard;
                guard = (top & 1) != 0;
                top = (top >> 1) | (u64::from(bit) << 52);
            }
            bit_index += 1;
        }

        let mut exponent = bit_index - 1;
        if bit_index > 53 && guard && (sticky || (top & 1) != 0) {
            top += 1;
            if top == 1u64 << 53 {
                top >>= 1;
                exponent += 1;
            }
        }
        if exponent > 1023 {
            return if self.negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        }
        let bits = if bit_index <= 53 {
            (top as f64).to_bits()
        } else {
            (((exponent as u64 + 1023) << 52) | (top & ((1u64 << 52) - 1)))
        };
        let bits = if self.negative {
            bits | (1u64 << 63)
        } else {
            bits
        };
        f64::from_bits(bits)
    }

    fn decimal_digit_count(&self) -> usize {
        jet_limb_decimal_digits(&self.limbs)
    }


    fn ratio_to_f64(&self, denominator: &Self) -> f64 {
        jet_ratio_limbs_to_f64(
            &self.limbs,
            &denominator.limbs,
            self.negative != denominator.negative,
        )
    }

    fn scaled_to_f64(&self, scale: u32) -> f64 {
        let denominator = jet_decimal_power10_limbs(scale);
        jet_ratio_limbs_to_f64(&self.limbs, &denominator, false)
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

    /// Euclidean quotient and remainder. The remainder is always
    /// non-negative and smaller than the divisor magnitude; the quotient
    /// is adjusted from the truncating pair when the dividend is negative.
    pub fn div_rem_euclid(&self, other: &JetBigInt) -> Option<(JetBigInt, JetBigInt)> {
        let (mut quotient, mut remainder) = self.div_rem(other)?;
        if remainder.negative {
            remainder = remainder.add(&other.abs());
            let one = JetBigInt::from_int(1);
            quotient = if other.negative {
                quotient.add(&one)
            } else {
                quotient.sub(&one)
            };
        }
        Some((quotient, remainder))
    }

    pub fn abs(&self) -> JetBigInt {
        JetBigInt {
            negative: false,
            limbs: self.limbs.clone(),
        }
    }

    pub fn to_radix(&self, radix: u32) -> Result<String, String> {
        if !(2..=36).contains(&radix) {
            return Err(format!(
                "integer radix must be between 2 and 36, got {radix}"
            ));
        }
        let mut value = self.abs();
        let mut digits = Vec::new();
        while !value.is_zero() {
            let (next, digit) = value.div_rem_small(radix);
            digits.push(
                char::from_digit(digit, radix).expect("div_rem_small remainder is below the radix"),
            );
            value = next;
        }
        if digits.is_empty() {
            digits.push('0');
        } else {
            digits.reverse();
        }
        let body = digits.into_iter().collect::<String>();
        Ok(if self.negative {
            format!("-{body}")
        } else {
            body
        })
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

// D-INTBIG1: exact `Int` is an owned one-word Foundation carrier. Inline
// payloads remain direct; spilled words retain/release an immutable numeric
// node through `Clone`/`Drop` at each managed boundary.
pub const JET_INT_SMALL_MIN: i64 = jet_foundation::Numeric::JetInt::inline_min();
pub const JET_INT_SMALL_MAX: i64 = jet_foundation::Numeric::JetInt::inline_max();

fn jet_int_owned_from_raw(value: i64) -> jet_foundation::Numeric::JetInt {
    // SAFETY: an adapter receives `value` as a borrowed carrier and clones its
    // owner before storing or returning it.
    unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(value) }
}

pub fn jet_int_clone_from_raw(value: i64) -> jet_foundation::Numeric::JetInt {
    jet_int_owned_from_raw(value)
}

/// Return whether a raw exact-Int word carries its inline payload directly.
#[inline(always)]
pub fn jet_int_is_inline(value: i64) -> bool {
    !jet_int_is_tagged(value)
}
#[inline(always)]
pub fn jet_int_is_small(value: i64) -> bool {
    !jet_int_is_tagged(value)
        && (JET_INT_SMALL_MIN..=JET_INT_SMALL_MAX).contains(&value)
}

fn jet_int_is_tagged(value: i64) -> bool {
    !jet_int_owned_from_raw(value).is_inline()
}

fn jet_int_big_value(value: i64) -> Option<JetBigInt> {
    let owner = jet_int_owned_from_raw(value);
    if owner.is_inline() {
        None
    } else {
        JetBigInt::from_str(&owner.to_string_rep()).ok()
    }
}

fn jet_int_value(value: i64) -> JetBigInt {
    jet_int_big_value(value).unwrap_or_else(|| JetBigInt::from_int(value))
}

fn jet_int_pack(value: JetBigInt) -> i64 {
    let exact = jet_foundation::Numeric::CtBigInt::from_str(&value.to_string_rep())
        .unwrap_or_else(|_| jet_foundation::Numeric::CtBigInt::from_int(0));
    jet_foundation::Numeric::JetInt::from_big(exact).into_raw()
}

pub fn jet_int_try_new(
    value: i64,
) -> JetOutcome<i64, AllocError> {
    jet_foundation::Numeric::JetInt::try_from_i64(value).map(|value| value.into_raw())
}

pub fn jet_int_try_add(
    left: i64,
    right: i64,
) -> JetOutcome<i64, AllocError> {
    let left = jet_int_owned_from_raw(left);
    let right = jet_int_owned_from_raw(right);
    left.add(&right).map(|value| value.into_raw())
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
    jet_int_from_str(value.trim()).map_err(|_| format!("cannot parse `{value}` as an integer"))
}
pub fn jet_float_parse(value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("cannot parse `{value}` as a float"))
}


pub fn jet_int_to_radix(value: i64, radix: i64) -> Result<String, String> {
    let radix = u32::try_from(radix)
        .map_err(|_| format!("integer radix must be between 2 and 36, got {radix}"))?;
    jet_int_value(value).to_radix(radix)
}

pub fn jet_int_from_radix(text: &str, radix: i64) -> Result<i64, String> {
    let radix = u32::try_from(radix)
        .map_err(|_| format!("integer radix must be between 2 and 36, got {radix}"))?;
    JetBigInt::from_radix(text, radix).map(jet_int_pack)
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
    if !jet_int_is_tagged(value) {
        return value as f64;
    }
    jet_int_big_value(value)
        .map(|value| value.to_f64())
        .unwrap_or_else(|| {
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

// Keep the small-value branch in the shared Prelude, but expose it as a
// macro so AOT can place the branch and machine operation in a hot loop.
// The slow functions remain the sole promotion/overflow implementation;
// the macro only selects that shared rail after the same representation
// checks as the resident helpers below.
#[macro_export]
macro_rules! jet_int_compare_hot {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        if $crate::jet_std::jet_int_is_small(__jet_left)
            && $crate::jet_std::jet_int_is_small(__jet_right)
        {
            match __jet_left.cmp(&__jet_right) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        } else {
            $crate::jet_std::jet_int_compare_slow(__jet_left, __jet_right)
        }
    }};
}
pub use crate::jet_int_compare_hot;

#[macro_export]
macro_rules! jet_int_add_hot {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        if $crate::jet_std::jet_int_is_small(__jet_left)
            && $crate::jet_std::jet_int_is_small(__jet_right)
        {
            let (__jet_value, __jet_overflowed) = __jet_left.overflowing_add(__jet_right);
            if !__jet_overflowed
                && ($crate::jet_std::JET_INT_SMALL_MIN..=$crate::jet_std::JET_INT_SMALL_MAX)
                    .contains(&__jet_value)
            {
                __jet_value
            } else {
                $crate::jet_std::jet_int_add_slow(__jet_left, __jet_right)
            }
        } else {
            $crate::jet_std::jet_int_add_slow(__jet_left, __jet_right)
        }
    }};
}
pub use crate::jet_int_add_hot;

#[macro_export]
macro_rules! jet_int_sub_hot {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        if $crate::jet_std::jet_int_is_small(__jet_left)
            && $crate::jet_std::jet_int_is_small(__jet_right)
        {
            let (__jet_value, __jet_overflowed) = __jet_left.overflowing_sub(__jet_right);
            if !__jet_overflowed
                && ($crate::jet_std::JET_INT_SMALL_MIN..=$crate::jet_std::JET_INT_SMALL_MAX)
                    .contains(&__jet_value)
            {
                __jet_value
            } else {
                $crate::jet_std::jet_int_sub_slow(__jet_left, __jet_right)
            }
        } else {
            $crate::jet_std::jet_int_sub_slow(__jet_left, __jet_right)
        }
    }};
}
pub use crate::jet_int_sub_hot;

#[macro_export]
macro_rules! jet_int_mul_hot {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        if $crate::jet_std::jet_int_is_small(__jet_left)
            && $crate::jet_std::jet_int_is_small(__jet_right)
        {
            let (__jet_value, __jet_overflowed) = __jet_left.overflowing_mul(__jet_right);
            if !__jet_overflowed
                && ($crate::jet_std::JET_INT_SMALL_MIN..=$crate::jet_std::JET_INT_SMALL_MAX)
                    .contains(&__jet_value)
            {
                __jet_value
            } else {
                $crate::jet_std::jet_int_mul_slow(__jet_left, __jet_right)
            }
        } else {
            $crate::jet_std::jet_int_mul_slow(__jet_left, __jet_right)
        }
    }};
}
pub use crate::jet_int_mul_hot;

// These macros have no fallback by design. The TIR emitter may select them
// only after its interval/cost fact proves both operands and the result stay
// on the signed-63-bit rail. Every unproved operation remains on the hot
// macro above, which preserves bigint promotion and checked arithmetic.
#[macro_export]
macro_rules! jet_int_add_inline {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        __jet_left + __jet_right
    }};
}
pub use crate::jet_int_add_inline;

#[macro_export]
macro_rules! jet_int_sub_inline {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        __jet_left - __jet_right
    }};
}
pub use crate::jet_int_sub_inline;

#[macro_export]
macro_rules! jet_int_mul_inline {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        __jet_left * __jet_right
    }};
}
pub use crate::jet_int_mul_inline;

#[macro_export]
macro_rules! jet_int_compare_inline {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        match __jet_left.cmp(&__jet_right) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }};
}
pub use crate::jet_int_compare_inline;

#[macro_export]
macro_rules! jet_int_neg_inline {
    ($value:expr) => {{
        let __jet_value = $value;
        -__jet_value
    }};
}
pub use crate::jet_int_neg_inline;

// Bitwise operations cannot overflow a valid packed value. Keep their
// representation check in the shared Prelude so the cold bigint rail is
// still selected for every tagged or otherwise non-packed input.
#[macro_export]
macro_rules! jet_int_bit_and_hot {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        if $crate::jet_std::jet_int_is_small(__jet_left)
            && $crate::jet_std::jet_int_is_small(__jet_right)
        {
            __jet_left & __jet_right
        } else {
            $crate::jet_std::jet_int_bit_and_slow(__jet_left, __jet_right)
        }
    }};
}
pub use crate::jet_int_bit_and_hot;

#[macro_export]
macro_rules! jet_int_bit_or_hot {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        if $crate::jet_std::jet_int_is_small(__jet_left)
            && $crate::jet_std::jet_int_is_small(__jet_right)
        {
            __jet_left | __jet_right
        } else {
            $crate::jet_std::jet_int_bit_or_slow(__jet_left, __jet_right)
        }
    }};
}
pub use crate::jet_int_bit_or_hot;

#[macro_export]
macro_rules! jet_int_bit_xor_hot {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        if $crate::jet_std::jet_int_is_small(__jet_left)
            && $crate::jet_std::jet_int_is_small(__jet_right)
        {
            __jet_left ^ __jet_right
        } else {
            $crate::jet_std::jet_int_bit_xor_slow(__jet_left, __jet_right)
        }
    }};
}
pub use crate::jet_int_bit_xor_hot;

#[inline(always)]
pub fn jet_int_compare(left: i64, right: i64) -> i64 {
    jet_int_compare_hot!(left, right)
}

#[cold]
#[inline(never)]
pub fn jet_int_compare_slow(left: i64, right: i64) -> i64 {
    match jet_int_value(left).compare(&jet_int_value(right)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Hot default-Int path. Keep promotion/overflow handling in a cold
/// helper so tight loops see two tag tests and one checked machine op.
#[inline(always)]
pub fn jet_int_add(left: i64, right: i64) -> i64 {
    jet_int_add_hot!(left, right)
}

#[cold]
#[inline(never)]
pub fn jet_int_add_slow(left: i64, right: i64) -> i64 {
    jet_int_pack(jet_int_value(left).add(&jet_int_value(right)))
}

#[inline(always)]
pub fn jet_int_sub(left: i64, right: i64) -> i64 {
    jet_int_sub_hot!(left, right)
}

#[cold]
#[inline(never)]
pub fn jet_int_sub_slow(left: i64, right: i64) -> i64 {
    jet_int_pack(jet_int_value(left).sub(&jet_int_value(right)))
}

#[inline(always)]
pub fn jet_int_mul(left: i64, right: i64) -> i64 {
    jet_int_mul_hot!(left, right)
}

#[cold]
#[inline(never)]
pub fn jet_int_mul_slow(left: i64, right: i64) -> i64 {
    jet_int_pack(jet_int_value(left).mul(&jet_int_value(right)))
}

#[inline(always)]
pub fn jet_int_bit_and(left: i64, right: i64) -> i64 {
    jet_int_bit_and_hot!(left, right)
}

#[cold]
#[inline(never)]
pub fn jet_int_bit_and_slow(left: i64, right: i64) -> i64 {
    jet_int_pack(jet_int_value(left).bit_and(&jet_int_value(right)))
}

#[inline(always)]
pub fn jet_int_bit_or(left: i64, right: i64) -> i64 {
    jet_int_bit_or_hot!(left, right)
}

#[cold]
#[inline(never)]
pub fn jet_int_bit_or_slow(left: i64, right: i64) -> i64 {
    jet_int_pack(jet_int_value(left).bit_or(&jet_int_value(right)))
}

#[inline(always)]
pub fn jet_int_bit_xor(left: i64, right: i64) -> i64 {
    jet_int_bit_xor_hot!(left, right)
}

#[cold]
#[inline(never)]
pub fn jet_int_bit_xor_slow(left: i64, right: i64) -> i64 {
    jet_int_pack(jet_int_value(left).bit_xor(&jet_int_value(right)))
}

#[inline(always)]
pub fn jet_int_neg(value: i64) -> i64 {
    if jet_int_is_small(value) {
        let (value, overflowed) = value.overflowing_neg();
        if !overflowed && (JET_INT_SMALL_MIN..=JET_INT_SMALL_MAX).contains(&value) {
            return value;
        }
    }
    jet_int_neg_slow(value)
}

#[cold]
#[inline(never)]
fn jet_int_neg_slow(value: i64) -> i64 {
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
    jet_int_to_i128(value)
        .and_then(|value| crate::jet_numeric_fixed_from_i128(value, kind))
}

pub fn jet_int_try_from_checked(value: i64, kind: i64) -> Result<i128, String> {
    jet_int_try_from(value, kind)
        .ok_or_else(|| crate::JET_NUMERIC_CONVERSION_ERROR.to_string())
}

pub fn jet_int_checked_fixed(value: i64, kind: i64, file: &str, line: u32) -> i128 {
    jet_int_try_from(value, kind).unwrap_or_else(|| {
        crate::jet_arithmetic_stop(file, line, crate::JET_NUMERIC_CONVERSION_TRAP)
    })
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
        .unwrap_or_else(|| crate::jet_arithmetic_stop(file, line, "Invalid shift count"))
}

pub fn jet_int_shr(value: i64, count: i64, file: &str, line: u32) -> i64 {
    jet_int_value(value)
        .shr(&jet_int_value(count))
        .map(jet_int_pack)
        .unwrap_or_else(|| crate::jet_arithmetic_stop(file, line, "Invalid shift count"))
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
    if jet_int_is_small(value) && jet_int_is_small(divisor) {
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

pub fn jet_int_div_euclid(value: i64, divisor: i64, file: &str, line: u32) -> i64 {
    if jet_int_is_zero(divisor) {
        crate::jet_arithmetic_stop(file, line, crate::JET_ARITHMETIC_DIVIDE_ZERO);
    }
    let (quotient, _) = jet_int_value(value)
        .div_rem_euclid(&jet_int_value(divisor))
        .expect("checked Euclidean division by zero");
    jet_int_pack(quotient)
}

pub fn jet_int_rem_euclid(value: i64, divisor: i64, file: &str, line: u32) -> i64 {
    if jet_int_is_zero(divisor) {
        crate::jet_arithmetic_stop(file, line, crate::JET_ARITHMETIC_DIVIDE_ZERO);
    }
    let (_, remainder) = jet_int_value(value)
        .div_rem_euclid(&jet_int_value(divisor))
        .expect("checked Euclidean remainder by zero");
    jet_int_pack(remainder)
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
        crate::jet_arithmetic_stop(file, line, "Negative default Int exponent");
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
    if !jet_int_is_zero(remainder) && jet_int_is_negative(value) != jet_int_is_negative(divisor) {
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

// Native calls can return a machine integer or an already-owned exact Int.
// Neither is an invitation to interpret an arbitrary word as an owned pointer.
pub trait JetNativeIntResult {
    fn into_owned_int(self) -> jet_foundation::Numeric::JetInt;
}

impl JetNativeIntResult for i64 {
    #[inline(always)]
    fn into_owned_int(self) -> jet_foundation::Numeric::JetInt {
        jet_foundation::Numeric::JetInt::from_i64(self)
    }
}

impl JetNativeIntResult for jet_foundation::Numeric::JetInt {
    #[inline(always)]
    fn into_owned_int(self) -> jet_foundation::Numeric::JetInt {
        self
    }
}

#[inline(always)]
pub fn jet_int_owned_from_native_result(
    value: impl JetNativeIntResult,
) -> jet_foundation::Numeric::JetInt {
    value.into_owned_int()
}

#[inline(always)]
fn jet_int_owned_from_raw_result(value: i64) -> jet_foundation::Numeric::JetInt {
    // SAFETY: raw arithmetic adapters below pass freshly owned carrier results.
    unsafe { jet_foundation::Numeric::JetInt::from_raw_owned(value) }
}

#[inline(always)]
fn jet_int_owned_raw(value: &jet_foundation::Numeric::JetInt) -> i64 {
    value.to_raw()
}

pub fn jet_int_owned_from_i64(value: i64) -> jet_foundation::Numeric::JetInt {
    jet_foundation::Numeric::JetInt::from_i64(value)
}

pub fn jet_int_owned_from_u64(value: u64) -> jet_foundation::Numeric::JetInt {
    jet_foundation::Numeric::JetInt::from_big(
        jet_foundation::Numeric::CtBigInt::from_u64(value),
    )
}

pub fn jet_int_owned_from_str(
    value: &str,
) -> Result<jet_foundation::Numeric::JetInt, String> {
    jet_foundation::Numeric::CtBigInt::from_str(value)
        .map(jet_foundation::Numeric::JetInt::from_big)
}

pub fn jet_int_owned_parse(
    value: &str,
) -> Result<jet_foundation::Numeric::JetInt, String> {
    jet_int_owned_from_str(value.trim())
        .map_err(|_| format!("cannot parse `{value}` as an integer"))
}

pub fn jet_int_owned_to_radix(
    value: &jet_foundation::Numeric::JetInt,
    radix: &jet_foundation::Numeric::JetInt,
) -> Result<String, String> {
    jet_int_to_radix(jet_int_owned_raw(value), jet_int_owned_to_i64(radix)?)
}

pub fn jet_int_owned_from_radix(
    value: &str,
    radix: &jet_foundation::Numeric::JetInt,
) -> Result<jet_foundation::Numeric::JetInt, String> {
    jet_int_from_radix(value, jet_int_owned_to_i64(radix)?)
        .map(jet_int_owned_from_raw_result)
}

pub fn jet_int_owned_to_i64(
    value: &jet_foundation::Numeric::JetInt,
) -> Result<i64, String> {
    jet_int_to_i64(jet_int_owned_raw(value))
        .ok_or_else(|| "exact Int does not fit the native i64 ABI".to_string())
}

pub fn jet_int_owned_to_f64(value: &jet_foundation::Numeric::JetInt) -> f64 {
    jet_int_to_f64(jet_int_owned_raw(value))
}

pub fn jet_int_owned_compare(
    left: &jet_foundation::Numeric::JetInt,
    right: &jet_foundation::Numeric::JetInt,
) -> i64 {
    jet_int_compare(jet_int_owned_raw(left), jet_int_owned_raw(right))
}

macro_rules! jet_int_owned_binary {
    ($owned:ident, $raw:ident) => {
        pub fn $owned(
            left: &jet_foundation::Numeric::JetInt,
            right: &jet_foundation::Numeric::JetInt,
        ) -> jet_foundation::Numeric::JetInt {
            jet_int_owned_from_raw_result($raw(jet_int_owned_raw(left), jet_int_owned_raw(right)))
        }
    };
}
jet_int_owned_binary!(jet_int_owned_add, jet_int_add);
jet_int_owned_binary!(jet_int_owned_sub, jet_int_sub);
jet_int_owned_binary!(jet_int_owned_mul, jet_int_mul);
jet_int_owned_binary!(jet_int_owned_bit_and, jet_int_bit_and);
jet_int_owned_binary!(jet_int_owned_bit_or, jet_int_bit_or);
jet_int_owned_binary!(jet_int_owned_bit_xor, jet_int_bit_xor);

pub fn jet_int_owned_neg(
    value: &jet_foundation::Numeric::JetInt,
) -> jet_foundation::Numeric::JetInt {
    jet_int_owned_from_raw_result(jet_int_neg(jet_int_owned_raw(value)))
}

pub fn jet_int_owned_abs(
    value: &jet_foundation::Numeric::JetInt,
) -> jet_foundation::Numeric::JetInt {
    jet_int_owned_from_raw_result(jet_int_abs(jet_int_owned_raw(value)))
}

pub fn jet_int_owned_not(
    value: &jet_foundation::Numeric::JetInt,
) -> jet_foundation::Numeric::JetInt {
    jet_int_owned_from_raw_result(jet_int_not(jet_int_owned_raw(value)))
}

macro_rules! jet_int_owned_binary_context {
    ($owned:ident, $raw:ident) => {
        pub fn $owned(
            left: &jet_foundation::Numeric::JetInt,
            right: &jet_foundation::Numeric::JetInt,
            file: &str,
            line: u32,
        ) -> jet_foundation::Numeric::JetInt {
            jet_int_owned_from_raw_result($raw(
                jet_int_owned_raw(left),
                jet_int_owned_raw(right),
                file,
                line,
            ))
        }
    };
}
jet_int_owned_binary_context!(jet_int_owned_div, jet_int_div);
jet_int_owned_binary_context!(jet_int_owned_rem, jet_int_rem);
jet_int_owned_binary_context!(jet_int_owned_div_euclid, jet_int_div_euclid);
jet_int_owned_binary_context!(jet_int_owned_rem_euclid, jet_int_rem_euclid);
jet_int_owned_binary_context!(jet_int_owned_floor_div, jet_int_floor_div);
jet_int_owned_binary_context!(jet_int_owned_mod, jet_int_mod);
jet_int_owned_binary_context!(jet_int_owned_pow, jet_int_pow);

pub fn jet_int_owned_shl(
    value: &jet_foundation::Numeric::JetInt,
    count: &jet_foundation::Numeric::JetInt,
    file: &str,
    line: u32,
) -> jet_foundation::Numeric::JetInt {
    jet_int_owned_from_raw_result(jet_int_shl(
        jet_int_owned_raw(value),
        jet_int_owned_raw(count),
        file,
        line,
    ))
}

pub fn jet_int_owned_shr(
    value: &jet_foundation::Numeric::JetInt,
    count: &jet_foundation::Numeric::JetInt,
    file: &str,
    line: u32,
) -> jet_foundation::Numeric::JetInt {
    jet_int_owned_from_raw_result(jet_int_shr(
        jet_int_owned_raw(value),
        jet_int_owned_raw(count),
        file,
        line,
    ))
}

pub fn jet_int_owned_checked_widen(
    value: &jet_foundation::Numeric::JetInt,
    target_f32: bool,
    file: &str,
    line: u32,
) -> f64 {
    jet_int_checked_widen(jet_int_owned_raw(value), target_f32, file, line)
}

pub fn jet_int_owned_bit_count(
    value: &jet_foundation::Numeric::JetInt,
    width: i64,
    method: &str,
) -> i64 {
    jet_int_bit_count(jet_int_owned_raw(value), width as u32, method)
}

pub fn jet_int_owned_try_from_checked(
    value: &jet_foundation::Numeric::JetInt,
    kind: i64,
) -> Result<i128, String> {
    jet_int_try_from_checked(jet_int_owned_raw(value), kind)
}

pub fn jet_int_owned_checked_fixed(
    value: &jet_foundation::Numeric::JetInt,
    kind: i64,
    file: &str,
    line: u32,
) -> i128 {
    jet_int_checked_fixed(jet_int_owned_raw(value), kind, file, line)
}

pub fn jet_int_owned_inline_range(
    value: &jet_foundation::Numeric::JetInt,
    lo: i64,
    hi: i64,
) -> Result<jet_foundation::Numeric::JetInt, String> {
    crate::jet_inline_range_from_int(jet_int_owned_to_i64(value)?, lo, hi)
        .map(jet_int_owned_from_i64)
}
pub fn jet_fmt_decimal_int_owned(
    value: &jet_foundation::Numeric::JetInt,
    precision: &jet_foundation::Numeric::JetInt,
) -> String {
    let text = value.to_string_rep();
    crate::jet_fmt_decimal_int(&text, jet_int_owned_to_i64(precision).unwrap_or_else(|error| panic!("{error}")))
}

pub fn jet_fmt_grouped_int_owned(
    value: &jet_foundation::Numeric::JetInt,
    precision: &jet_foundation::Numeric::JetInt,
) -> String {
    let text = value.to_string_rep();
    crate::jet_fmt_grouped_int(&text, jet_int_owned_to_i64(precision).unwrap_or_else(|error| panic!("{error}")))
}

pub fn jet_fmt_hex_owned(
    value: &jet_foundation::Numeric::JetInt,
    width: &jet_foundation::Numeric::JetInt,
) -> String {
    let text = value.to_string_rep();
    crate::jet_fmt_hex_decimal(&text, jet_int_owned_to_i64(width).unwrap_or_else(|error| panic!("{error}")))
}

pub fn jet_fmt_bin_owned(value: &jet_foundation::Numeric::JetInt) -> String {
    let text = value.to_string_rep();
    crate::jet_fmt_bin_decimal(&text)
}

pub fn jet_fmt_oct_owned(value: &jet_foundation::Numeric::JetInt) -> String {
    let text = value.to_string_rep();
    crate::jet_fmt_oct_decimal(&text)
}

// D-NUMTYPE1=A: an exact ratio of two whole numbers, always reduced, with
// the sign carried on the top. A zero bottom has no value, so building one
// answers nothing rather than a wrong number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetFraction {
    numerator: JetBigInt,
    denominator: JetBigInt,
}

impl PartialOrd for JetFraction {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JetFraction {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.numerator
            .mul(&other.denominator)
            .compare(&other.numerator.mul(&self.denominator))
    }
}

impl JetFraction {
    pub fn new(numerator: i64, denominator: i64) -> Option<Self> {
        Self::from_bigints(jet_int_value(numerator), jet_int_value(denominator))
    }

    pub fn from_bigints(mut numerator: JetBigInt, mut denominator: JetBigInt) -> Option<Self> {
        if denominator.is_zero() {
            return None;
        }
        if denominator.negative {
            numerator = numerator.neg();
            denominator = denominator.neg();
        }
        let divisor = JetBigInt::gcd(&numerator, &denominator);
        Some(Self {
            numerator: numerator.div_rem(&divisor)?.0,
            denominator: denominator.div_rem(&divisor)?.0,
        })
    }

    pub fn add(&self, other: &Self) -> Option<Self> {
        let left = self.numerator.mul(&other.denominator);
        let right = other.numerator.mul(&self.denominator);
        Self::from_bigints(left.add(&right), self.denominator.mul(&other.denominator))
    }

    pub fn sub(&self, other: &Self) -> Option<Self> {
        let left = self.numerator.mul(&other.denominator);
        let right = other.numerator.mul(&self.denominator);
        Self::from_bigints(left.sub(&right), self.denominator.mul(&other.denominator))
    }

    pub fn mul(&self, other: &Self) -> Option<Self> {
        Self::from_bigints(
            self.numerator.mul(&other.numerator),
            self.denominator.mul(&other.denominator),
        )
    }

    pub fn div(&self, other: &Self) -> Option<Self> {
        Self::from_bigints(
            self.numerator.mul(&other.denominator),
            self.denominator.mul(&other.numerator),
        )
    }

    pub fn from_int(value: i64) -> Option<Self> {
        Self::from_bigints(jet_int_value(value), JetBigInt::from_int(1))
    }

    pub fn from_float(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        if value == 0.0 {
            return Self::new(0, 1);
        }
        let bits = value.to_bits();
        let negative = (bits >> 63) != 0;
        let exponent = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1u64 << 52) - 1);
        let (significand, power) = if exponent == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1u64 << 52), exponent - 1023 - 52)
        };
        let mut numerator = JetBigInt::from_u64(significand);
        let denominator = if power >= 0 {
            for _ in 0..u32::try_from(power).ok()? {
                numerator = numerator.mul(&JetBigInt::from_int(2));
            }
            JetBigInt::from_int(1)
        } else {
            let mut denominator = JetBigInt::from_int(1);
            for _ in 0..u32::try_from(-power).ok()? {
                denominator = denominator.mul(&JetBigInt::from_int(2));
            }
            denominator
        };
        if negative {
            numerator = numerator.neg();
        }
        Self::from_bigints(numerator, denominator)
    }

    pub fn from_decimal(value: &JetDecimal) -> Option<Self> {
        value.to_fraction()
    }

    pub fn to_int_exact(&self) -> Option<i64> {
        (self.denominator == JetBigInt::from_int(1)).then(|| jet_int_pack(self.numerator.clone()))
    }

    pub fn to_decimal(&self) -> Option<JetDecimal> {
        JetDecimal::from_fraction(self)
    }

    pub fn to_float(&self) -> f64 {
        self.numerator.ratio_to_f64(&self.denominator)
    }

    pub fn is_zero(&self) -> bool {
        self.numerator.is_zero()
    }

    pub fn to_string_rep(&self) -> String {
        if let Some(decimal) = JetDecimal::from_fraction(self) {
            return decimal.to_string_rep();
        }
        format!(
            "{}/{}",
            self.numerator.to_string_rep(),
            self.denominator.to_string_rep()
        )
    }

    pub fn numerator_value(&self) -> i64 {
        jet_int_pack(self.numerator.clone())
    }

    pub fn denominator_value(&self) -> i64 {
        jet_int_pack(self.denominator.clone())
    }
}

impl super::JetShow for JetFraction {
    fn jet_show(&self) -> String {
        self.to_string_rep()
    }
}

impl super::JetDisplay for JetFraction {
    fn jet_display(&self) -> String {
        self.to_string_rep()
    }
}

// D-DECIMAL1: exact base-10 decimal (scaled integer + scale).
#[derive(Clone, Debug, PartialEq, Eq)]
enum JetDecimalMagnitude {
    Small(i128),
    Big(JetBigInt),
}

fn jet_decimal_bigint_from_i128(value: i128) -> JetBigInt {
    let negative = value < 0;
    let mut magnitude = if negative {
        value.wrapping_neg() as u128
    } else {
        value as u128
    };
    if magnitude == 0 {
        return JetBigInt::from_int(0);
    }
    let mut limbs = Vec::new();
    while magnitude != 0 {
        limbs.push((magnitude % u128::from(BI_BASE)) as u32);
        magnitude /= u128::from(BI_BASE);
    }
    JetBigInt { negative, limbs }
}

fn jet_decimal_mul_pow10(value: &JetBigInt, places: u32) -> JetBigInt {
    if places == 0 || value.is_zero() {
        return value.clone();
    }
    let chunk_shift = (places / 9) as usize;
    let remainder = places % 9;
    let mut limbs =
        Vec::with_capacity(value.limbs.len() + chunk_shift + usize::from(remainder != 0));
    limbs.resize(chunk_shift, 0);
    if remainder == 0 {
        limbs.extend_from_slice(&value.limbs);
    } else {
        let factor = 10u32.pow(remainder);
        let mut carry = 0u64;
        for &limb in &value.limbs {
            let product = u64::from(limb) * u64::from(factor) + carry;
            limbs.push((product % BI_BASE) as u32);
            carry = product / BI_BASE;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
    }
    JetBigInt {
        negative: value.negative,
        limbs,
    }
}

fn jet_decimal_pow10(scale: u32) -> JetBigInt {
    let chunk_shift = (scale / 9) as usize;
    let remainder = scale % 9;
    let mut limbs = vec![0; chunk_shift];
    limbs.push(10u32.pow(remainder));
    JetBigInt {
        negative: false,
        limbs,
    }
}

fn jet_decimal_strip_trailing_zeros(value: &mut JetBigInt, scale: &mut u32) {
    while *scale > 0 && !value.is_zero() {
        let zero_chunks = value.limbs.iter().take_while(|limb| **limb == 0).count();
        let removable_chunks = zero_chunks.min((*scale / 9) as usize);
        if removable_chunks > 0 {
            value.limbs.drain(..removable_chunks);
            *scale -= (removable_chunks as u32) * 9;
            continue;
        }

        let low = *value.limbs.first().unwrap_or(&0);
        let mut zeros = 0u32;
        let mut factor = 1u32;
        while zeros < 9 && low % (factor * 10) == 0 {
            factor *= 10;
            zeros += 1;
        }
        let removable = zeros.min(*scale);
        if removable == 0 {
            break;
        }
        let divisor = 10u32.pow(removable);
        let (next, remainder) = value.div_rem_small(divisor);
        debug_assert_eq!(remainder, 0);
        *value = next;
        *scale -= removable;
    }
}

fn jet_decimal_magnitude_from_digits(digits: &[u8]) -> JetDecimalMagnitude {
    debug_assert!(!digits.is_empty());
    if digits.len() <= 39 {
        let mut small = 0i128;
        let mut fits = true;
        for &digit in digits {
            let Some(next) = small
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(digit)))
            else {
                fits = false;
                break;
            };
            small = next;
        }
        if fits {
            return JetDecimalMagnitude::Small(small);
        }
    }

    let chunk_count = digits.len().div_ceil(9);
    let first_chunk_len = match digits.len() % 9 {
        0 => 9,
        length => length,
    };
    let mut limbs = Vec::with_capacity(chunk_count);
    let mut offset = 0usize;
    let mut chunk_len = first_chunk_len;
    while offset < digits.len() {
        let mut chunk = 0u32;
        for &digit in &digits[offset..offset + chunk_len] {
            chunk = chunk * 10 + u32::from(digit);
        }
        limbs.push(chunk);
        offset += chunk_len;
        chunk_len = 9;
    }
    limbs.reverse();
    JetDecimalMagnitude::Big(JetBigInt {
        negative: false,
        limbs,
    })
}

fn jet_decimal_parse(s: &str) -> Result<(bool, JetDecimalMagnitude, u32), String> {
    let (negative, digits, scale) = crate::jet_json_number::json_decimal_lexeme(s)?;
    Ok((
        negative,
        jet_decimal_magnitude_from_digits(&digits),
        scale,
    ))
}

fn jet_decimal_small_to_f64(value: i128, scale: u32) -> f64 {
    debug_assert!(value >= 0);
    if value == 0 {
        return 0.0;
    }
    let mut magnitude = value as u128;
    let mut limbs = [0u32; 5];
    let mut length = 0usize;
    while magnitude != 0 {
        limbs[length] = (magnitude % BI_BASE as u128) as u32;
        magnitude /= BI_BASE as u128;
        length += 1;
    }
    let denominator = jet_decimal_power10_limbs(scale);
    jet_ratio_limbs_to_f64(&limbs[..length], &denominator, false)
}

#[derive(Clone, Debug, PartialEq, Eq)]

pub struct JetDecimal {
    negative: bool,
    magnitude: JetDecimalMagnitude,
    scale: u32,
}

impl JetDecimal {
    pub fn from_str(s: &str) -> Result<Self, String> {
        let (negative, magnitude, scale) = jet_decimal_parse(s)?;
        Ok(Self {
            negative,
            magnitude,
            scale,
        })
    }

    /// Project one JSON number token directly into base-10 chunks. This
    /// preserves written scale and exponent without crossing binary64.
    pub fn from_json_number(s: &str) -> Result<Self, String> {
        Self::from_str(s)
    }

    pub fn from_int(value: i64) -> Result<Self, String> {
        let value = i128::from(value);
        let negative = value < 0;
        let magnitude = if negative {
            value.wrapping_neg()
        } else {
            value
        };
        Ok(Self {
            negative,
            magnitude: JetDecimalMagnitude::Small(magnitude),
            scale: 0,
        })
    }

    /// Preserve the exact binary64 value as a finite base-10 decimal.
    /// Every binary denominator is a power of two, so a matching power
    /// of five produces an exact decimal expansion.
    pub fn from_float(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        if value == 0.0 {
            return Some(Self::from_int(0).expect("zero is a valid Decimal"));
        }
        let bits = value.to_bits();
        let negative = (bits >> 63) != 0;
        let exponent = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1u64 << 52) - 1);
        let (significand, power) = if exponent == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1u64 << 52), exponent - 1023 - 52)
        };
        let mut numerator = JetBigInt::from_u64(significand);
        if power >= 0 {
            for _ in 0..power {
                numerator = numerator.mul(&JetBigInt::from_int(2));
            }
            Some(Self::from_bigint(numerator, 0, negative))
        } else {
            let scale = u32::try_from(-power).ok()?;
            for _ in 0..scale {
                numerator = numerator.mul(&JetBigInt::from_int(5));
            }
            Some(Self::from_bigint(numerator, scale, negative))
        }
    }

    fn is_zero(&self) -> bool {
        match &self.magnitude {
            JetDecimalMagnitude::Small(value) => *value == 0,
            JetDecimalMagnitude::Big(value) => value.is_zero(),
        }
    }

    fn normalize(mut self) -> Self {
        match &mut self.magnitude {
            JetDecimalMagnitude::Small(value) => {
                while self.scale > 0 && *value != 0 && *value % 10 == 0 {
                    *value /= 10;
                    self.scale -= 1;
                }
            }
            JetDecimalMagnitude::Big(value) => {
                jet_decimal_strip_trailing_zeros(value, &mut self.scale);
            }
        }
        if self.is_zero() {
            self.negative = false;
            self.scale = 0;
        }
        self
    }

    fn signed_small(&self) -> Option<i128> {
        let JetDecimalMagnitude::Small(value) = &self.magnitude else {
            return None;
        };
        if self.negative {
            value.checked_neg()
        } else {
            Some(*value)
        }
    }

    fn scale_small(value: i128, places: u32) -> Option<i128> {
        if value == 0 {
            return Some(0);
        }
        value.checked_mul(10i128.checked_pow(places)?)
    }

    fn from_signed_small_preserving_scale(value: i128, scale: u32) -> Option<Self> {
        let negative = value < 0;
        let magnitude = if negative {
            value.checked_neg()?
        } else {
            value
        };
        Some(Self {
            negative,
            magnitude: JetDecimalMagnitude::Small(magnitude),
            scale,
        })
    }

    fn magnitude_bigint(&self) -> JetBigInt {
        match &self.magnitude {
            JetDecimalMagnitude::Small(value) => jet_decimal_bigint_from_i128(*value),
            JetDecimalMagnitude::Big(value) => value.clone(),
        }
    }

    fn signed_bigint(&self) -> JetBigInt {
        let mut value = self.magnitude_bigint();
        if self.negative && !value.is_zero() {
            value.negative = true;
        }
        value
    }

    fn scaled_bigint(&self, scale: u32) -> JetBigInt {
        jet_decimal_mul_pow10(&self.signed_bigint(), scale - self.scale)
    }

    fn from_bigint_preserving_scale(
        value: JetBigInt,
        scale: u32,
        negative: bool,
    ) -> JetDecimal {
        let magnitude = value.abs();
        let negative = negative && !magnitude.is_zero();
        let magnitude = if let Some(value) = magnitude.try_i128() {
            JetDecimalMagnitude::Small(value)
        } else {
            JetDecimalMagnitude::Big(magnitude)
        };
        JetDecimal {
            negative,
            magnitude,
            scale,
        }
    }

    fn from_bigint(value: JetBigInt, scale: u32, negative: bool) -> JetDecimal {
        Self::from_bigint_preserving_scale(value, scale, negative).normalize()
    }

    fn from_signed_bigint_preserving_scale(value: JetBigInt, scale: u32) -> JetDecimal {
        let negative = value.negative;
        Self::from_bigint_preserving_scale(value, scale, negative)
    }

    fn try_add(&self, other: &JetDecimal, negate_other: bool) -> Option<JetDecimal> {
        let scale = self.scale.max(other.scale);
        let left = Self::scale_small(self.signed_small()?, scale - self.scale)?;
        let mut right = Self::scale_small(other.signed_small()?, scale - other.scale)?;
        if negate_other {
            right = right.checked_neg()?;
        }
        Self::from_signed_small_preserving_scale(left.checked_add(right)?, scale)
    }

    fn add_with_sign(&self, other: &JetDecimal, negate_other: bool) -> JetDecimal {
        if let Some(value) = self.try_add(other, negate_other) {
            return value;
        }
        let scale = self.scale.max(other.scale);
        let left = self.scaled_bigint(scale);
        let right = other.scaled_bigint(scale);
        let right = if negate_other {
            right.neg()
        } else {
            right
        };
        Self::from_signed_bigint_preserving_scale(left.add(&right), scale)
    }

    pub fn add(&self, other: &JetDecimal) -> JetDecimal {
        self.add_with_sign(other, false)
    }

    pub fn sub(&self, other: &JetDecimal) -> JetDecimal {
        self.add_with_sign(other, true)
    }

    pub fn mul(&self, other: &JetDecimal) -> JetDecimal {
        let mut scale = self.scale + other.scale;
        let minimum_scale = self.scale.max(other.scale);
        if let (Some(left), Some(right)) = (self.signed_small(), other.signed_small()) {
            if let Some(mut product) = left.checked_mul(right) {
                if product == 0 {
                    scale = minimum_scale;
                } else {
                    while scale > minimum_scale && product % 10 == 0 {
                        product /= 10;
                        scale -= 1;
                    }
                }
                if let Some(value) = Self::from_signed_small_preserving_scale(product, scale) {
                    return value;
                }
            }
        }

        let product = self.signed_bigint().mul(&other.signed_bigint());
        let negative = product.negative;
        let mut magnitude = product.abs();
        let mut removable_scale = scale - minimum_scale;
        if magnitude.is_zero() {
            removable_scale = 0;
        } else {
            jet_decimal_strip_trailing_zeros(&mut magnitude, &mut removable_scale);
        }
        Self::from_signed_bigint_preserving_scale(
            magnitude.with_sign(negative),
            minimum_scale + removable_scale,
        )
    }

    fn scale_factor(scale: u32) -> JetBigInt {
        jet_decimal_pow10(scale)
    }

    fn from_signed_bigint(value: JetBigInt, scale: u32) -> JetDecimal {
        let negative = value.negative;
        JetDecimal::from_bigint(value.abs(), scale, negative)
    }

    pub fn to_fraction(&self) -> Option<JetFraction> {
        JetFraction::from_bigints(self.signed_bigint(), Self::scale_factor(self.scale))
    }

    pub fn to_float(&self) -> f64 {
        let magnitude = match &self.magnitude {
            JetDecimalMagnitude::Small(value) => jet_decimal_small_to_f64(*value, self.scale),
            JetDecimalMagnitude::Big(value) => value.scaled_to_f64(self.scale),
        };
        if self.negative {
            -magnitude
        } else {
            magnitude
        }
    }

    pub fn div(&self, other: &JetDecimal) -> Option<JetFraction> {
        self.to_fraction()?.div(&other.to_fraction()?)
    }

    fn quotient_remainder(&self) -> (JetBigInt, JetBigInt, JetBigInt) {
        let denominator = Self::scale_factor(self.scale);
        let (quotient, remainder) = self
            .signed_bigint()
            .div_rem(&denominator)
            .expect("Decimal scale denominator is nonzero");
        (quotient, remainder, denominator)
    }

    pub fn to_int_exact(&self) -> Option<i64> {
        let (quotient, remainder, _) = self.quotient_remainder();
        remainder.is_zero().then(|| jet_int_pack(quotient))
    }

    fn floor_int(&self) -> JetBigInt {
        let (quotient, remainder, _) = self.quotient_remainder();
        if self.negative && !remainder.is_zero() {
            quotient.sub(&JetBigInt::from_int(1))
        } else {
            quotient
        }
    }

    fn ceil_int(&self) -> JetBigInt {
        let (quotient, remainder, _) = self.quotient_remainder();
        if !self.negative && !remainder.is_zero() {
            quotient.add(&JetBigInt::from_int(1))
        } else {
            quotient
        }
    }

    fn round_int(&self) -> JetBigInt {
        let (quotient, remainder, denominator) = self.quotient_remainder();
        let doubled = remainder.abs().mul(&JetBigInt::from_int(2));
        let mut magnitude = quotient.abs();
        if doubled.compare(&denominator) != std::cmp::Ordering::Less {
            magnitude = magnitude.add(&JetBigInt::from_int(1));
        }
        if self.negative {
            magnitude.neg()
        } else {
            magnitude
        }
    }

    pub fn floor(&self) -> JetDecimal {
        Self::from_signed_bigint(self.floor_int(), 0)
    }

    pub fn ceil(&self) -> JetDecimal {
        Self::from_signed_bigint(self.ceil_int(), 0)
    }

    pub fn round(&self) -> JetDecimal {
        Self::from_signed_bigint(self.round_int(), 0)
    }

    /// Build a Decimal only when the reduced ratio has a finite base-10
    /// expansion. Repeating ratios remain Fractions by design.
    pub fn from_fraction(fraction: &JetFraction) -> Option<Self> {
        let mut factors = fraction.denominator.clone();
        let mut twos = 0u32;
        loop {
            let (quotient, remainder) = factors.div_rem_small(2);
            if remainder != 0 {
                break;
            }
            factors = quotient;
            twos += 1;
        }
        let mut fives = 0u32;
        loop {
            let (quotient, remainder) = factors.div_rem_small(5);
            if remainder != 0 {
                break;
            }
            factors = quotient;
            fives += 1;
        }
        if factors != JetBigInt::from_int(1) {
            return None;
        }
        let scale = twos.max(fives);
        let scaled = fraction.numerator.mul(&Self::scale_factor(scale));
        let (digits, remainder) = scaled.div_rem(&fraction.denominator)?;
        if !remainder.is_zero() {
            return None;
        }
        Some(Self::from_signed_bigint(digits, scale))
    }

    fn magnitude_len(&self) -> usize {
        match &self.magnitude {
            JetDecimalMagnitude::Small(value) => {
                let mut value = *value;
                let mut digits = 1usize;
                while value >= 10 {
                    value /= 10;
                    digits += 1;
                }
                digits
            }
            JetDecimalMagnitude::Big(value) => value.decimal_digit_count(),
        }
    }

    fn write_magnitude(&self, out: &mut String) {
        use std::fmt::Write as _;
        match &self.magnitude {
            JetDecimalMagnitude::Small(value) => {
                let _ = write!(out, "{value}");
            }
            JetDecimalMagnitude::Big(value) => {
                let top = *value.limbs.last().unwrap_or(&0);
                let _ = write!(out, "{top}");
                for &limb in value.limbs.iter().rev().skip(1) {
                    let _ = write!(out, "{limb:09}");
                }
            }
        }
    }

    pub fn to_string_rep(&self) -> String {
        if self.is_zero() {
            let fraction_len = self.scale as usize;
            if fraction_len == 0 {
                return "0".to_string();
            }
            let mut out = String::with_capacity(2 + fraction_len);
            out.push_str("0.");
            out.extend(std::iter::repeat('0').take(fraction_len));
            return out;
        }
        let fraction_len = self.scale as usize;
        let digit_len = self.magnitude_len();
        let capacity = digit_len
            .max(fraction_len.saturating_add(1))
            .saturating_add(usize::from(self.negative))
            .saturating_add(usize::from(fraction_len > 0));
        let mut out = String::with_capacity(capacity);
        if self.negative {
            out.push('-');
        }
        if fraction_len == 0 {
            self.write_magnitude(&mut out);
            return out;
        }
        if digit_len <= fraction_len {
            out.push('0');
            out.push('.');
            out.extend(std::iter::repeat('0').take(fraction_len - digit_len));
            self.write_magnitude(&mut out);
        } else {
            self.write_magnitude(&mut out);
            let split = out.len() - fraction_len;
            out.insert(split, '.');
        }
        out
    }
}

impl super::JetShow for JetDecimal {
    fn jet_show(&self) -> String {
        self.to_string_rep()
    }
}

impl super::JetDisplay for JetDecimal {
    fn jet_display(&self) -> String {
        self.to_string_rep()
    }
}

impl super::JetDebug for JetDecimal {
    fn jet_debug(&self) -> String {
        self.to_string_rep()
    }
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
                ("cause".to_string(), super::JetDebug::jet_debug(&self.cause)),
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
            IOError::ResourceLimit(limit) => {
                return format!("process resource limit exceeded: {}", limit.jet_show());
            }
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
            IOError::ResourceLimit(limit) => {
                return crate::jet_debug_variant("ResourceLimit", Some(limit.jet_debug()));
            }
        };
        crate::jet_debug_variant(variant, Some(super::JetDebug::jet_debug(context)))
    }
}
impl super::JetShow for ProcessResourceLimit {
    fn jet_show(&self) -> String {
        let variant = match self {
            ProcessResourceLimit::WallTime => 0,
            ProcessResourceLimit::CpuTime => 1,
            ProcessResourceLimit::Memory => 2,
            ProcessResourceLimit::OpenFiles => 3,
            ProcessResourceLimit::Output => 4,
        };
        crate::jet_show_process_resource_limit(variant)
    }
}
impl super::JetDisplay for ProcessResourceLimit {
    fn jet_display(&self) -> String {
        <Self as super::JetShow>::jet_show(self)
    }
}
impl super::JetDebug for ProcessResourceLimit {
    fn jet_debug(&self) -> String {
        match self {
            ProcessResourceLimit::WallTime => "WallTime",
            ProcessResourceLimit::CpuTime => "CpuTime",
            ProcessResourceLimit::Memory => "Memory",
            ProcessResourceLimit::OpenFiles => "OpenFiles",
            ProcessResourceLimit::Output => "Output",
        }
        .to_string()
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
impl super::JetShow for ProcessPlan {
    fn jet_show(&self) -> String {
        format!(
            "ProcessPlan {{ executable: {}, backend: {}, policy_digest: {} }}",
            self.executable_identity, self.backend, self.policy_digest
        )
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
        format!(
            "Fake {{ locale: {} }}",
            if self.locale == 1 { "de" } else { "en" }
        )
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
