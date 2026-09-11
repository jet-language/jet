//! D-DBG1 / D-DBG3 — `jet debug <file>`: the source-level step debugger.
//!
//! The debugger steps your program at the **Jet source level** (I2): every
//! breakpoint, step, frame, and value is in Jet terms. It never surfaces the
//! generated Rust — values are shown through the interpreter's own
//! `CtValue::jet_show()` path, the same bytes a `print` would produce.
//!
//! Backend (D-DBG3 step 1, the shipped vertical slice): the debugger drives the
//! existing tree-walking interpreter (`crate::Comptime`) — the same engine
//! behind `jet dev` and `jet repl`. It registers a [`DebugHook`] that the
//! interpreter calls before every statement; the hook decides whether to pause
//! and run the interactive `(jet)` prompt. Because it reuses the dev
//! interpreter, it covers the same deterministic subset and declines the same
//! boundary (FFI / tasks / `#Unsafe` / native std) with **E2203**, pointing at
//! the native lldb-backed adapter shipped by D-DBG3 step 2.
//!
//! Command surface (D-DBG3, ratified A): the prompt is `(jet)`; the step verbs
//! are lldb-familiar `step` / `next` / `continue` / `finish` with single-letter
//! aliases `s` / `n` / `c` / `f`. Recorded sessions add `back` and
//! `reverse-continue`; live sessions add bounded `watch` and checked `fix`.
//! A paused line shows a `<- here` caret and a one-line `locals:` dump;
//! `help` lists every verb. Only Jet frames/lines and safe locals are shown by
//! default (D-DBG2 — `--raw-frames` is the clearly marked native expert view).
//!
//! I6: std-only — no DAP/JSON crate, no debugger library. The interactive loop
//! reads stdin with `std::io`; tests drive it with a scripted-input transcript
//! (`run_session`), the same shape as the REPL transcript tests.
//!
//! D-DBG3 step 2 (dap-debugger): the native backend lives in the sibling
//! submodules below. [`LineMap`] and [`Inferior`] are the shared building
//! blocks; [`Native`] is the `(jet)`-prompt terminal session; [`Dap`] is the
//! Debug Adapter Protocol server editors (VS Code/Zed) launch instead.

#![allow(non_snake_case)]
#![deny(warnings)]

// D-ARCH-SOURCE1=A: full debugger ownership lives here. Compiler semantics
// enter through inward path-only seams; no root host dependency exists.
pub use jet_driver::{Comptime, Diagnostics, Loader, Sema, Syntax, AST};
pub use jet_foundation::ExitCodes;

mod Dap;
mod EventObservation;
mod Inferior;
mod LineMap;
mod Native;
pub use Native::{NativeLiveResult, NativeLiveSession};

pub use EventObservation::render as render_event_observations;

use std::collections::{HashMap, HashSet};

use crate::Comptime::{CtValue, DebugHook, DevSink};
use crate::Diagnostics::{span_line_col, Diagnostic, Span};

/// One paused frame for the `backtrace` view: the executing function and the
/// Jet line it is stopped on. Newest (innermost) frame last.
#[derive(Clone)]
struct Frame {
    func: String,
    line: usize,
}

/// One value captured at a real paused source statement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueSnapshot {
    pub name: String,
    pub type_name: String,
    pub value: String,
}

/// One Jet frame captured at a real paused source statement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameSnapshot {
    pub function: String,
    pub line: usize,
}

/// Runtime-owned debugger state for the current paused interpreter frame.
/// Canvas projects this instead of guessing from terminal text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugSnapshot {
    pub function: String,
    pub line: usize,
    pub locals: Vec<ValueSnapshot>,
    pub call_stack: Vec<FrameSnapshot>,
}

/// Aggregate shape retained by the safe debugger evaluator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugAggregateKind {
    Record,
    List,
    Map,
    Set,
    Tuple,
    Variant,
}

/// A debugger value after the backend has removed unsafe runtime layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebugValue {
    Unit,
    Bool(bool),
    Int(i64),
    FloatBits(u64),
    Text(String),
    Bytes { length: u64, sha256: String },
    Aggregate {
        kind: DebugAggregateKind,
        length: u64,
    },
}

/// Bounded accounting for one safe debugger evaluation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DebugEvaluationUsage {
    pub depth: u32,
    pub nodes: u64,
    pub bytes: u64,
    pub steps: u64,
}

/// Result of resolving a read-only Jet local path from a real paused frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugEvaluation {
    pub expression: String,
    pub name: String,
    pub type_name: String,
    pub value: DebugValue,
    pub usage: DebugEvaluationUsage,
}

impl DebugSnapshot {
    /// Stable identity for this actual paused fact set.
    pub fn pause_id(&self) -> String {
        format!("pause-{}", &self.identity_digest()[..24])
    }

    /// Stable identity for the current frame in this paused fact set.
    pub fn frame_id(&self) -> String {
        let mut key = format!("{}:{}", self.function, self.line);
        for frame in &self.call_stack {
            key.push('|');
            key.push_str(&frame.function);
            key.push(':');
            key.push_str(&frame.line.to_string());
        }
        format!(
            "frame-{}",
            &jet_foundation::SHA256::sha256_hex(key.as_bytes())[..24]
        )
    }

    fn identity_digest(&self) -> String {
        let mut key = format!("{}:{}", self.function, self.line);
        for frame in &self.call_stack {
            key.push('|');
            key.push_str(&frame.function);
            key.push(':');
            key.push_str(&frame.line.to_string());
        }
        for local in &self.locals {
            key.push('|');
            key.push_str(&local.name);
            key.push(':');
            key.push_str(&local.type_name);
            key.push('=');
            key.push_str(&local.value);
        }
        jet_foundation::SHA256::sha256_hex(key.as_bytes())
    }
}

/// Resolve one bounded read-only Jet local path from a paused snapshot.
///
/// This function consumes debugger facts only. It never invokes an
/// interpreter, native debugger, process, or host callback.
pub fn evaluate_snapshot(
    snapshot: &DebugSnapshot,
    expression: &str,
) -> Result<DebugEvaluation, String> {
    let expression = expression.trim();
    let Some((root, suffix)) = split_debug_expression(expression) else {
        return Err("only bounded read-only Jet local paths can be evaluated".to_string());
    };
    if !suffix.is_empty() {
        return Err(
            "nested debugger evaluation is unavailable until the paused backend publishes child facts"
                .to_string(),
        );
    }
    let Some(value) = snapshot.locals.iter().find(|value| value.name == root) else {
        return Err(format!("no readable local named `{root}` at this stop"));
    };
    let typed = debug_value(value)?;
    let bytes = match &typed {
        DebugValue::Text(text) => text.len() as u64,
        DebugValue::Bytes { .. }
        | DebugValue::Aggregate { .. }
        | DebugValue::Unit
        | DebugValue::Bool(_)
        | DebugValue::Int(_)
        | DebugValue::FloatBits(_) => 8,
    };
    Ok(DebugEvaluation {
        expression: expression.to_string(),
        name: value.name.clone(),
        type_name: value.type_name.clone(),
        value: typed,
        usage: DebugEvaluationUsage {
            depth: 1,
            nodes: 1,
            bytes,
            steps: 1,
        },
    })
}

fn split_debug_expression(value: &str) -> Option<(&str, &str)> {
    if value.is_empty() || value.len() > 16 * 1024 || value.chars().any(char::is_control) {
        return None;
    }
    let root_end = value
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_')
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    let root = &value[..root_end];
    if root.is_empty()
        || root.starts_with('_')
        || !root.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        || !root[1..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let mut suffix = &value[root_end..];
    while !suffix.is_empty() {
        if let Some(field) = suffix.strip_prefix('.') {
            let end = field
                .char_indices()
                .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_')
                .map(|(index, _)| index)
                .unwrap_or(field.len());
            if end == 0
                || !field[..end]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            {
                return None;
            }
            suffix = &field[end..];
        } else if let Some(index) = suffix.strip_prefix('[') {
            let end = index.find(']')?;
            if end == 0 || !index[..end].chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            suffix = &index[end + 1..];
        } else {
            return None;
        }
    }
    Some((root, &value[root_end..]))
}

fn debug_value(value: &ValueSnapshot) -> Result<DebugValue, String> {
    let raw = value.value.trim();
    if matches!(raw, "<optimized out>" | "<unavailable>") {
        return Err(format!("local `{}` has no readable value at this stop", value.name));
    }
    match value.type_name.trim() {
        "Bool" | "bool" => raw
            .parse::<bool>()
            .map(DebugValue::Bool)
            .map_err(|_| format!("local `{}` has an invalid Bool value", value.name)),
        "Int"
        | "int"
        | "i8"
        | "i16"
        | "i32"
        | "i64"
        | "isize"
        | "i128"
        | "u8"
        | "u16"
        | "u32"
        | "u64"
        | "usize"
        | "u128"
        | "unsigned"
        | "unsigned long"
        | "unsigned long long"
        | "long"
        | "long long" => raw
            .parse::<i64>()
            .map(DebugValue::Int)
            .map_err(|_| format!("local `{}` has an unavailable Int value", value.name)),
        "Float" | "f32" | "f64" => raw
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| DebugValue::FloatBits(value.to_bits()))
            .ok_or_else(|| format!("local `{}` has an unavailable Float value", value.name)),
        "Unit" | "()" if raw.is_empty() || raw == "()" => Ok(DebugValue::Unit),
        "String" | "str" | "&str" | "Char" | "char" => {
            Ok(DebugValue::Text(unquote_debug_text(raw)))
        }
        _ if raw.starts_with('[') && raw.ends_with(']') => Ok(DebugValue::Aggregate {
            kind: DebugAggregateKind::List,
            length: aggregate_length(raw),
        }),
        _ if raw.starts_with('{') && raw.ends_with('}') => Ok(DebugValue::Aggregate {
            kind: DebugAggregateKind::Record,
            length: aggregate_length(raw),
        }),
        _ if raw.contains("Map") || value.type_name.contains("Map") => {
            Ok(DebugValue::Aggregate {
                kind: DebugAggregateKind::Map,
                length: aggregate_length(raw),
            })
        }
        _ if raw.contains('(') && raw.ends_with(')') => Ok(DebugValue::Aggregate {
            kind: DebugAggregateKind::Variant,
            length: aggregate_length(raw),
        }),
        _ => Err(format!(
            "local `{}` has a value shape the safe debugger cannot publish",
            value.name
        )),
    }
}

fn aggregate_length(raw: &str) -> u64 {
    let inner = if let Some(value) = raw.strip_prefix('[') {
        value.strip_suffix(']').unwrap_or(raw)
    } else if let Some(value) = raw.strip_prefix('{') {
        value.strip_suffix('}').unwrap_or(raw)
    } else if let Some(value) = raw.strip_prefix('(') {
        value.strip_suffix(')').unwrap_or(raw)
    } else {
        raw
    }
    .trim();
    if inner.is_empty() {
        return 0;
    }
    let mut depth = 0u32;
    let mut count = 1u64;
    for ch in inner.chars() {
        match ch {
            '[' | '{' | '(' => depth = depth.saturating_add(1),
            ']' | '}' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => count = count.saturating_add(1),
            _ => {}
        }
    }
    count
}

fn unquote_debug_text(raw: &str) -> String {
    let Some(inner) = raw.strip_prefix('"').and_then(|value| value.strip_suffix('"')) else {
        return raw.to_string();
    };
    let mut text = String::with_capacity(inner.len());
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            text.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '"' => '"',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            text.push(ch);
        }
    }
    if escaped {
        text.push('\\');
    }
    text
}

/// One post-statement source event shared by the interpreter, native adapter,
/// DAP server, Canvas, and replay receipts. Values are already projected into
/// safe Jet terms; no generated-Rust addresses or host objects cross this seam.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugEvent {
    pub sequence: u64,
    pub function: String,
    pub line: usize,
    pub depth: usize,
    pub stack: Vec<FrameSnapshot>,
    pub locals: Vec<ValueSnapshot>,
    pub writes: Vec<DebugWrite>,
}

/// One source-level write observed at a statement boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugWrite {
    pub line: usize,
    pub place: String,
    pub old_value: String,
    pub new_value: String,
}

/// Stable authority captured before a live session starts. A repair can only
/// rebind when this checkpoint remains identical.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugCheckpoint {
    pub source_identity: String,
    pub shape: String,
    pub sequence: u64,
}

/// Bounded, indexed debugger history. Frontends navigate this same model;
/// execution engines only append events and never decide replay semantics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugHistory {
    pub events: Vec<DebugEvent>,
    pub checkpoint: Option<DebugCheckpoint>,
    pub cursor: Option<usize>,
    pub capped: bool,
    /// Canonical compiler decision ledger carried through replay history.
    pub decision_ledger: Option<String>,
}

const MAX_DEBUG_HISTORY: usize = 100_000;
const MAX_DEBUG_WATCHES: usize = 32;

impl DebugHistory {
    pub fn push(&mut self, event: DebugEvent) -> bool {
        if self.events.len() >= MAX_DEBUG_HISTORY {
            self.capped = true;
            return false;
        }
        self.events.push(event);
        true
    }

    pub fn from_recorded_run(
        run: &RecordedRun,
        source_identity: impl Into<String>,
        shape: impl Into<String>,
    ) -> Self {
        let writes_by_sequence = debug_writes_by_sequence(run);
        let events = run
            .acts
            .iter()
            .map(|act| DebugEvent {
                sequence: act.sequence,
                function: act.function.clone(),
                line: act.line,
                depth: 0,
                stack: vec![FrameSnapshot {
                    function: act.function.clone(),
                    line: act.line,
                }],
                locals: act.locals.clone(),
                writes: writes_by_sequence
                    .get(&act.sequence)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        Self {
            events,
            checkpoint: Some(DebugCheckpoint {
                source_identity: source_identity.into(),
                shape: shape.into(),
                sequence: run.acts.last().map_or(0, |act| act.sequence),
            }),
            cursor: None,
            capped: run.acts.len() >= MAX_DEBUG_HISTORY,
            decision_ledger: run.decision_ledger.clone(),
        }
    }

    pub fn to_recorded_run(&self) -> RecordedRun {
        RecordedRun {
            acts: self
                .events
                .iter()
                .map(|event| RecordedAct {
                    sequence: event.sequence,
                    function: event.function.clone(),
                    line: event.line,
                    locals: event.locals.clone(),
                })
                .collect(),
            decision_ledger: self.decision_ledger.clone(),
        }
    }
}

/// One post-statement state in a recorded run receipt. This act snapshot is
/// retained for compatibility with existing safe replay artifacts; richer
/// debugger consumers use [`DebugEvent`] through [`DebugHistory`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecordedAct {
    pub sequence: u64,
    pub function: String,
    pub line: usize,
    pub locals: Vec<ValueSnapshot>,
}

/// Bounded causal evidence attached to a `.jetproof-replay` receipt.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecordedRun {
    pub acts: Vec<RecordedAct>,
    /// Canonical checked MIR decision ledger captured with the run, when the
    /// execution adapter had one.  The payload is validated by ProveReplay
    /// before it enters a receipt or inspect view.
    pub decision_ledger: Option<String>,
}

#[derive(Clone, Debug)]
struct IndexedWrite {
    sequence: u64,
    line: usize,
    place: String,
    old_value: String,
    new_value: String,
}

#[derive(Clone, Debug)]
struct PendingStatement {
    function: String,
    line: usize,
    before: HashMap<String, ValueSnapshot>,
}

#[derive(Clone, Debug)]
struct FixRequest {
    source: String,
    target: usize,
}

/// What the user asked the debugger to do next, set by a `(jet)` command and
/// read by the hook to decide where to stop.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Resume {
    /// `step`/`s`: stop at the very next statement, at any call depth.
    Step,
    /// `next`/`n`: stop at the next statement at this depth or shallower (step
    /// over a call without descending into it).
    Next { depth: usize },
    /// `finish`/`f`: run until control returns to a shallower frame.
    Finish { depth: usize },
    /// `continue`/`c`: run until the next breakpoint (or the program ends).
    Continue,
}

/// How the debugger reads its `(jet)` commands and where its output goes.
/// Interactive mode reads stdin and writes the real terminal; scripted mode
/// pops from a fixed input queue and appends output to an owned buffer so tests
/// can assert on the exact session (the same model as `REPL::run_transcript`).
enum IO {
    /// Live session: prompt + read a line from stdin, print to stdout.
    Interactive,
    /// Scripted session: a queue of typed inputs and an owned output buffer.
    Scripted {
        inputs: std::collections::VecDeque<String>,
        out: String,
        /// Keep the interpreter at the current stop when scripted input ends.
        /// Canvas uses this to expose one live command boundary; transcript
        /// tests retain the historical run-to-completion behavior.
        pause_on_input_end: bool,
    },
}

impl IO {
    /// True for the scripted (test/transcript) path.
    fn is_scripted(&self) -> bool {
        matches!(self, IO::Scripted { .. })
    }

    fn pause_on_input_end(&self) -> bool {
        matches!(
            self,
            IO::Scripted {
                pause_on_input_end: true,
                ..
            }
        )
    }
    /// Append a line of debugger/program output (scripted buffer only; the
    /// interactive path writes the terminal directly at the call site).
    fn push_line(&mut self, s: &str) {
        if let IO::Scripted { out, .. } = self {
            out.push_str(s);
            out.push('\n');
        }
    }
    /// Take the captured transcript (empty for the interactive path).
    fn into_output(&mut self) -> String {
        match self {
            IO::Scripted { out, .. } => std::mem::take(out),
            IO::Interactive => String::new(),
        }
    }
}

/// The interactive driver. Holds the breakpoint set, the pending resume mode,
/// the source text (for the `<- here` caret and `list`), and the live call
/// stack. Implements [`DebugHook`] so the interpreter can call it per statement.
struct Debugger {
    /// Jet line numbers the user set a breakpoint on (`break <line>`).
    breakpoints: HashSet<usize>,
    /// What to do next; consumed/recomputed each time we pause.
    resume: Resume,
    /// The entry file's source, for the `<- here` caret and the breakpoint banner.
    src: String,
    /// The file name shown in banners (e.g. `loops.jet`).
    file: String,
    /// The path used to reload and semantically check a proposed source fix.
    source_path: String,
    /// Stable declaration/type/layout shape captured at the checkpoint.
    checkpoint_shape: String,
    /// The live call stack, innermost last. Updated each time the hook fires.
    stack: Vec<Frame>,
    io: IO,
    /// True once the program has stopped at least once, so the first pause
    /// prints the full banner and later ones print just the moved line.
    started: bool,
    /// Set when the user typed `quit`: the hook returns E2204 to abort the run.
    quit: bool,
    /// Set when Canvas input ends at a live stop. This is an internal unwind
    /// signal; the paused transcript is not a diagnostic.
    paused: bool,
    /// Structured state captured at the last real stop. This is separate from
    /// the human transcript so program output cannot fabricate liveness.
    snapshot: Option<DebugSnapshot>,
    /// Canonical event/stack/checkpoint/value history shared with native, DAP,
    /// Canvas, and receipt replay consumers.
    history: DebugHistory,
    /// Legacy receipt projection retained for the stable artifact API.
    recording: RecordedRun,
    /// Replay receipts are authoritative; do not append live observations to
    /// their captured act list.
    recording_locked: bool,
    /// Writes derived at statement boundaries. This is deliberately session
    /// owned; no process-global watchpoint state exists.
    writes: Vec<IndexedWrite>,
    /// Scope captured before the currently executing statement.
    pending: Option<PendingStatement>,
    /// Session-scoped places whose writes should stop the next statement.
    watches: HashSet<String>,
    /// Monotonic statement sequence, including statements beyond the retention
    /// cap. It prevents a capped receipt from repeating sequence numbers.
    statement_sequence: u64,
    /// True after the bounded history cap rejects another observation.
    history_limit_reached: bool,
    /// `Some(index)` means the prompt is viewing a retained post-statement
    /// state; `None` means the interpreter is at its live boundary.
    history_cursor: Option<usize>,
    /// True when this debugger is browsing an authoritative replay receipt
    /// instead of driving an interpreter.
    replay_mode: bool,
    replay_finished: bool,
    /// A checked source edit requested a restart at this checkpoint.
    fix_request: Option<FixRequest>,
    /// Remaining statements to replay silently after a compatible fix.
    rebind_remaining: Option<usize>,
    rebind_prefix: Vec<RecordedAct>,
    rebind_reached: bool,
}

impl Debugger {
    fn new(
        src: String,
        file: String,
        source_path: String,
        io: IO,
        recording: Option<RecordedRun>,
        checkpoint_shape: String,
    ) -> Self {
        let (recording, recording_locked) = match recording {
            Some(recording) => (recording, true),
            None => (RecordedRun::default(), false),
        };
        let replay_mode = recording_locked;
        let statement_sequence = recording.acts.last().map_or(0, |act| act.sequence);
        let writes = derive_recorded_writes(&recording, &src);
        let source_identity = jet_foundation::SHA256::sha256_hex(
            format!("{}:{}", source_path, src).as_bytes(),
        );
        let history =
            DebugHistory::from_recorded_run(&recording, source_identity, checkpoint_shape.clone());
        Debugger {
            breakpoints: HashSet::new(),
            // Stop on the very first statement so the user lands inside `main`
            // (lldb/gdb behavior: `run` halts at the entry breakpoint).
            resume: Resume::Step,
            src,
            file,
            source_path,
            checkpoint_shape,
            stack: Vec::new(),
            io,
            started: false,
            quit: false,
            paused: false,
            snapshot: None,
            recording,
            history,
            recording_locked,
            writes,
            pending: None,
            watches: HashSet::new(),
            statement_sequence,
            history_limit_reached: false,
            history_cursor: None,
            replay_mode,
            replay_finished: false,
            fix_request: None,
            rebind_remaining: None,
            rebind_prefix: Vec::new(),
            rebind_reached: !replay_mode && statement_sequence == 0,
        }
    }


    /// 1-based Jet line for a span's start.
    fn line_of(&self, span: Span) -> usize {
        span_line_col(&self.src, span.start).0
    }

    /// The text of a 1-based source line, without the trailing newline.
    fn source_line(&self, line: usize) -> &str {
        self.src.lines().nth(line.saturating_sub(1)).unwrap_or("")
    }

    /// Emit a line of debugger output (to stdout or the transcript buffer).
    fn emit(&mut self, s: &str) {
        match &mut self.io {
            IO::Interactive => println!("{}", s),
            IO::Scripted { out, .. } => {
                out.push_str(s);
                out.push('\n');
            }
        }
    }

    /// Read the next `(jet)` command line. `None` means end-of-input: in a live
    /// session that is EOF (treat as `continue`); in a script it means the
    /// transcript ran out (also `continue` to let the program finish).
    fn read_command(&mut self) -> Option<String> {
        match &mut self.io {
            IO::Interactive => {
                use std::io::Write;
                print!("{} ", Syntax::DBG_PROMPT);
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) => None, // EOF (Ctrl-D)
                    Ok(_) => Some(line.trim().to_string()),
                    Err(_) => None,
                }
            }
            IO::Scripted { inputs, out, .. } => {
                let next = inputs.pop_front()?;
                // Echo the typed command after the prompt so the transcript
                // reads like a real session.
                out.push_str(&format!("{} {}\n", Syntax::DBG_PROMPT, next.trim()));
                Some(next.trim().to_string())
            }
        }
    }

    /// Print the stop banner: the file:line, the function, a small source
    /// window with a `<- here` caret on the current line, and the one-line
    /// `locals:` dump (D-DBG3 layout).
    fn show_stop(&mut self, line: usize, scope: &HashMap<String, CtValue>) {
        if !self.started {
            self.started = true;
            let func = self
                .stack
                .last()
                .map(|f| f.func.clone())
                .unwrap_or_else(|| "run".to_string());
            self.emit(&format!(
                "breakpoint hit  {}:{}  in {}()",
                self.file, line, func
            ));
        }
        // A two-line window (prev + current) mirrors the plan's worked example.
        for l in line.saturating_sub(1)..=line {
            if l == 0 {
                continue;
            }
            let text = self.source_line(l);
            if text.is_empty() && l != line {
                continue;
            }
            let caret = if l == line { "        <- here" } else { "" };
            self.emit(&format!("   {} | {}{}", l, text.trim_end(), caret));
        }
        self.emit(&render_locals(scope));
    }

    /// Run the `(jet)` prompt until the user issues a resume verb (step/next/
    /// continue/finish) or quits. `line`/`scope`/`depth` describe the current
    /// stop. Returns once `self.resume` is set for the run to proceed.
    fn prompt_loop(&mut self, line: usize, depth: usize, scope: &HashMap<String, CtValue>) {
        loop {
            let cmd = match self.read_command() {
                Some(c) => c,
                None => {
                    if self.io.pause_on_input_end() {
                        // Canvas owns the next command. Unwind the current
                        // interpreter invocation while retaining this stop.
                        self.paused = true;
                    } else {
                        // Transcript tests and terminal EOF run to completion.
                        self.resume = Resume::Continue;
                    }
                    return;
                }
            };
            if cmd.is_empty() {
                // Bare Enter repeats the last step kind (lldb behavior): re-issue
                // a single step.
                if self.history_cursor.is_some() {
                    if self.advance_history(Resume::Step) {
                        return;
                    }
                    continue;
                }
                self.resume = Resume::Step;
                return;
            }
            let mut parts = cmd.split_whitespace();
            let verb = parts.next().unwrap_or("");
            let arg = parts.next();
            let tail = parts.collect::<Vec<_>>().join(" ");
            let query = match (arg, tail.is_empty()) {
                (Some(arg), true) => arg.to_string(),
                (Some(arg), false) => format!("{arg} {tail}"),
                (None, _) => tail,
            };

            if self.history_cursor.is_some() {
                let resumed = match verb {
                    v if v == Syntax::DBG_STEP || v == "s" => self.advance_history(Resume::Step),
                    v if v == Syntax::DBG_NEXT || v == "n" => {
                        self.advance_history(Resume::Next { depth })
                    }
                    v if v == Syntax::DBG_FINISH || v == "f" => {
                        self.advance_history(Resume::Finish { depth })
                    }
                    v if v == Syntax::DBG_CONTINUE || v == "c" => {
                        self.advance_history(Resume::Continue)
                    }
                    v if v == Syntax::DBG_BACK
                        || v == "reverse"
                        || v == "reverse-step"
                        || v == "rstep" =>
                    {
                        let steps = arg
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(1);
                        self.reverse_step(steps);
                        false
                    }
                    v if v == Syntax::DBG_REVERSE_CONTINUE
                        || v == "rcontinue"
                        || v == "back-continue" =>
                    {
                        self.reverse_continue();
                        false
                    }
                    v if v == Syntax::DBG_BREAK || v == "b" => {
                        self.cmd_break(arg);
                        false
                    }
                    v if v == Syntax::DBG_LIST || v == "l" => {
                        self.cmd_list_history();
                        false
                    }
                    v if v == Syntax::DBG_PRINT || v == "p" => {
                        self.cmd_print_history(arg);
                        false
                    }
                    v if v == Syntax::DBG_LOCALS => {
                        self.cmd_locals_history();
                        false
                    }
                    v if v == Syntax::DBG_BACKTRACE || v == "bt" => {
                        self.cmd_backtrace_history();
                        false
                    }
                    v if v == Syntax::DBG_WHY => {
                        self.cmd_why(&query);
                        false
                    }
                    v if v == Syntax::DBG_WHEN => {
                        self.cmd_when(&query);
                        false
                    }
                    "watch" => {
                        self.cmd_watch(&query);
                        false
                    }
                    "unwatch" => {
                        self.cmd_unwatch(&query);
                        false
                    }
                    "fix" => {
                        self.cmd_fix();
                        self.fix_request.is_some()
                    }
                    v if v == Syntax::DBG_HELP || v == "h" => {
                        self.cmd_help();
                        false
                    }
                    v if v == Syntax::DBG_QUIT || v == "q" => {
                        self.quit = true;
                        return;
                    }
                    other => {
                        self.emit(&format!(
                            "unknown command `{}` — type `help` for the verbs",
                            other
                        ));
                        false
                    }
                };
                if resumed {
                    return;
                }
                if self.replay_finished {
                    return;
                }
                continue;
            }

            match verb {
                v if v == Syntax::DBG_STEP || v == "s" => {
                    self.resume = Resume::Step;
                    return;
                }
                v if v == Syntax::DBG_NEXT || v == "n" => {
                    self.resume = Resume::Next { depth };
                    return;
                }
                v if v == Syntax::DBG_FINISH || v == "f" => {
                    self.resume = Resume::Finish { depth };
                    return;
                }
                v if v == Syntax::DBG_CONTINUE || v == "c" => {
                    self.resume = Resume::Continue;
                    return;
                }
                v if v == Syntax::DBG_BACK
                    || v == "reverse"
                    || v == "reverse-step"
                    || v == "rstep" =>
                {
                    let steps = arg
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    self.reverse_step(steps);
                }
                v if v == Syntax::DBG_REVERSE_CONTINUE
                    || v == "rcontinue"
                    || v == "back-continue" =>
                {
                    self.reverse_continue();
                }
                v if v == Syntax::DBG_BREAK || v == "b" => {
                    self.cmd_break(arg);
                }
                v if v == Syntax::DBG_LIST || v == "l" => {
                    self.cmd_list(line);
                }
                v if v == Syntax::DBG_PRINT || v == "p" => {
                    self.cmd_print(arg, scope);
                }
                v if v == Syntax::DBG_LOCALS => {
                    let rendered = render_locals(scope);
                    self.emit(&rendered);
                }
                v if v == Syntax::DBG_BACKTRACE || v == "bt" => {
                    self.cmd_backtrace();
                }
                v if v == Syntax::DBG_WHY => {
                    self.cmd_why(&query);
                }
                v if v == Syntax::DBG_WHEN => {
                    self.cmd_when(&query);
                }
                v if v == Syntax::DBG_WATCH => {
                    self.cmd_watch(&query);
                    self.refresh_watch_baseline(scope);
                }
                v if v == Syntax::DBG_UNWATCH => {
                    self.cmd_unwatch(&query);
                    self.refresh_watch_baseline(scope);
                }
                v if v == Syntax::DBG_FIX => {
                    self.cmd_fix();
                    if self.fix_request.is_some() {
                        return;
                    }
                }
                v if v == Syntax::DBG_HELP || v == "h" => {
                    self.cmd_help();
                }
                v if v == Syntax::DBG_QUIT || v == "q" => {
                    self.quit = true;
                    return;
                }
                other => {
                    self.emit(&format!(
                        "unknown command `{}` — type `help` for the verbs",
                        other
                    ));
                }
            }
        }
    }

    fn history_count(&self) -> usize {
        self.history
            .cursor
            .map_or(self.history.events.len(), |index| index.saturating_add(1))
    }

    fn reverse_unavailable(&mut self, detail: &str) {
        let suffix = if self.history.capped {
            " (the bounded history limit has been reached)"
        } else {
            ""
        };
        self.emit(&format!("reverse unavailable: {detail}{suffix}"));
    }

    fn reverse_step(&mut self, steps: usize) {
        let steps = steps.max(1);
        let boundary = self.history_count();
        if boundary < steps {
            self.reverse_unavailable("no earlier recorded statement exists");
            return;
        }
        let index = boundary - steps;
        self.history.cursor = Some(index);
        self.history_cursor = Some(index);
        self.show_history_stop(index);
    }

    fn reverse_continue(&mut self) {
        let boundary = self.history_count();
        if boundary == 0 {
            self.reverse_unavailable(
                "no earlier recorded statement is available; continue once to create history",
            );
            return;
        }
        let start = self.history.cursor.map_or(boundary, |index| index);
        let mut target = None;
        for index in (0..start).rev() {
            let Some(event) = self.history.events.get(index) else {
                continue;
            };
            let breakpoint = self.breakpoints.contains(&event.line);
            let watched_write = event.writes.iter().any(|write| {
                self.watches.iter().any(|watch| {
                    watch == &write.place
                        || write.place.starts_with(&format!("{watch}."))
                        || watch.starts_with(&format!("{}.", write.place))
                })
            });
            if breakpoint || watched_write {
                target = Some(index);
                break;
            }
        }
        let Some(index) = target else {
            self.reverse_unavailable(
                "no earlier breakpoint or watched write exists in the retained history",
            );
            return;
        };
        self.history.cursor = Some(index);
        self.history_cursor = Some(index);
        self.show_history_stop(index);
    }

    /// Move one virtual history step. Returns true when the prompt should
    /// return to the interpreter (or complete a replay receipt).
    fn advance_history(&mut self, resume: Resume) -> bool {
        let Some(current) = self.history.cursor.or(self.history_cursor) else {
            return false;
        };
        let next = current.saturating_add(1);
        let target = match resume {
            Resume::Continue => (next..self.history.events.len()).find(|index| {
                self.history.events.get(*index).is_some_and(|event| {
                    self.breakpoints.contains(&event.line)
                        || event.writes.iter().any(|write| {
                            self.watches.iter().any(|watch| {
                                watch == &write.place
                                    || write.place.starts_with(&format!("{watch}."))
                                    || watch.starts_with(&format!("{}.", write.place))
                            })
                        })
                })
            }),
            Resume::Finish { .. } => {
                let depth = self
                    .history
                    .events
                    .get(current)
                    .map_or(0, |event| event.depth);
                (next..self.history.events.len()).find(|index| {
                    self.history
                        .events
                        .get(*index)
                        .is_some_and(|event| event.depth < depth)
                })
            }
            Resume::Next { depth } => (next..self.history.events.len()).find(|index| {
                self.history
                    .events
                    .get(*index)
                    .is_some_and(|event| event.depth <= depth)
            }),
            Resume::Step => (next < self.history.events.len()).then_some(next),
        };
        if let Some(index) = target {
            self.history.cursor = Some(index);
            self.history_cursor = Some(index);
            self.show_history_stop(index);
            return false;
        }
        if self.replay_mode {
            self.replay_finished = true;
            return true;
        }
        self.history.cursor = None;
        self.history_cursor = None;
        self.resume = resume;
        true
    }

    fn show_history_stop(&mut self, index: usize) {
        let Some(event) = self.history.events.get(index).cloned() else {
            self.reverse_unavailable("the requested recorded statement was evicted");
            return;
        };
        let stack = if event.stack.is_empty() {
            vec![FrameSnapshot {
                function: event.function.clone(),
                line: event.line,
            }]
        } else {
            event.stack.clone()
        };
        self.stack = stack
            .iter()
            .map(|frame| Frame {
                func: frame.function.clone(),
                line: frame.line,
            })
            .collect();
        self.snapshot = Some(DebugSnapshot {
            function: event.function.clone(),
            line: event.line,
            locals: event.locals.clone(),
            call_stack: stack,
        });
        self.emit(&format!(
            "breakpoint hit  {}:{}  in {}()  (recorded act {})",
            self.file, event.line, event.function, event.sequence
        ));
        for line in event.line.saturating_sub(1)..=event.line {
            if line == 0 {
                continue;
            }
            let text = self.source_line(line);
            if text.is_empty() && line != event.line {
                continue;
            }
            let caret = if line == event.line {
                "        <- here"
            } else {
                ""
            };
            self.emit(&format!("   {} | {}{}", line, text.trim_end(), caret));
        }
        self.emit(&render_snapshot_locals(&event.locals));
        for write in event.writes {
            self.emit(&format!(
                "write: {}:{}  {} -> {}",
                self.file, write.line, write.old_value, write.new_value
            ));
        }
    }

    fn render_write(&self, write: &IndexedWrite) -> String {
        format!(
            "write: {}:{}  {} -> {}",
            self.file, write.line, write.old_value, write.new_value
        )
    }

    fn cmd_list_history(&mut self) {
        let Some(index) = self.history_cursor else {
            self.cmd_list(self.stack.last().map_or(1, |frame| frame.line));
            return;
        };
        let line = self.history.events.get(index).map_or(1, |event| event.line);
        self.cmd_list(line);
    }

    fn cmd_print_history(&mut self, arg: Option<&str>) {
        let Some(index) = self.history_cursor else {
            self.emit("print is unavailable before a recorded history stop");
            return;
        };
        let Some(name) = arg else {
            self.emit("print needs a name, e.g. `print total`");
            return;
        };
        let Some(event) = self.history.events.get(index) else {
            self.reverse_unavailable("the requested recorded statement was evicted");
            return;
        };
        match event.locals.iter().find(|local| local.name == name) {
            Some(value) => self.emit(&format!("{} = {}", value.name, value.value)),
            None => self.emit(&format!("no local named `{name}` in this recorded frame")),
        }
    }

    fn cmd_locals_history(&mut self) {
        let Some(index) = self.history_cursor else {
            self.emit("locals are unavailable before a recorded history stop");
            return;
        };
        let Some(event) = self.history.events.get(index) else {
            self.reverse_unavailable("the requested recorded statement was evicted");
            return;
        };
        self.emit(&render_snapshot_locals(&event.locals));
    }

    fn cmd_backtrace_history(&mut self) {
        let Some(index) = self.history_cursor else {
            self.cmd_backtrace();
            return;
        };
        let Some(event) = self.history.events.get(index) else {
            self.reverse_unavailable("the requested recorded statement was evicted");
            return;
        };
        let stack = if event.stack.is_empty() {
            vec![FrameSnapshot {
                function: event.function.clone(),
                line: event.line,
            }]
        } else {
            event.stack.clone()
        };
        let frames = stack
            .iter()
            .rev()
            .enumerate()
            .map(|(frame, value)| {
                format!(
                    "#{}  {}()  at {}:{} (recorded act {})",
                    frame, value.function, self.file, value.line, event.sequence
                )
            })
            .collect::<Vec<_>>();
        self.emit(&frames.join("\n"));
    }

    fn cmd_watch(&mut self, query: &str) {
        let place = query.trim();
        if split_debug_expression(place).is_none() {
            self.emit("watch needs a bounded Jet local path, e.g. `watch account.balance`");
            return;
        }
        if self.watches.contains(place) {
            self.emit(&format!("watchpoint already set  {place}"));
            return;
        }
        if self.watches.len() >= MAX_DEBUG_WATCHES {
            self.emit("watchpoint limit reached (32 session watchpoints)");
            return;
        }
        self.watches.insert(place.to_string());
        self.emit(&format!("watchpoint set  {place} (session scope)"));
    }

    fn cmd_unwatch(&mut self, query: &str) {
        let place = query.trim();
        if place.is_empty() {
            self.emit("unwatch needs a place, e.g. `unwatch total`");
            return;
        }
        if self.watches.remove(place) {
            self.emit(&format!("watchpoint cleared  {place}"));
        } else {
            self.emit(&format!("no watchpoint named `{place}`"));
        }
    }
    fn refresh_watch_baseline(&mut self, scope: &HashMap<String, CtValue>) {
        if self.pending.is_some() {
            let baseline = snapshot_places(scope, &self.watches);
            if let Some(pending) = self.pending.as_mut() {
                pending.before = baseline;
            }
        }
    }

    fn cmd_fix(&mut self) {
        if self.replay_mode {
            self.emit(
                "fix unavailable: replay receipts are read-only; start a live debug session to repair source",
            );
            return;
        }
        let Ok(candidate_source) = std::fs::read_to_string(&self.source_path) else {
            self.emit("fix rejected: the debug source could not be read");
            return;
        };
        if candidate_source == self.src {
            self.emit("fix unavailable: source is unchanged; edit the file and retry");
            return;
        }
        let (_, _, candidate_shape) = match checked_bundle_for_fix(&self.source_path) {
            Ok(value) => value,
            Err(reason) => {
                self.emit(&format!("fix rejected: {reason}"));
                return;
            }
        };
        if candidate_shape != self.checkpoint_shape {
            self.emit(
                "fix rejected: checkpoint incompatible (source type, layout, or authority changed); restart required",
            );
            return;
        }
        let target = self.history_count();
        self.emit("checkpoint compatible");
        self.fix_request = Some(FixRequest {
            source: candidate_source,
            target,
        });
    }

    fn run_replay(
        &mut self,
    ) -> (
        i32,
        String,
        bool,
        Option<DebugSnapshot>,
        RecordedRun,
        DebugHistory,
    ) {
        if self.recording.acts.is_empty() {
            self.emit("reverse unavailable: replay receipt contains no statement history");
            let paused = self.io.pause_on_input_end();
            if paused {
                self.paused = true;
            } else {
                self.emit("program finished");
            }
            let snapshot = self.snapshot.clone();
            let history = self.history.clone();
            let recording = self.recording.clone();
            return (
                ExitCodes::OK,
                self.io.into_output(),
                paused,
                snapshot,
                recording,
                history,
            );
        }
        self.history.cursor = Some(0);
        self.history_cursor = Some(0);
        self.show_history_stop(0);
        self.prompt_loop(0, 0, &HashMap::new());
        if self.paused {
            let snapshot = self.snapshot.clone();
            let recording = self.recording.clone();
            let history = self.history.clone();
            return (
                ExitCodes::OK,
                self.io.into_output(),
                true,
                snapshot,
                recording,
                history,
            );
        }
        if self.quit {
            let snapshot = self.snapshot.clone();
            let recording = self.recording.clone();
            let history = self.history.clone();
            return (
                ExitCodes::USER_ERROR,
                self.io.into_output(),
                false,
                snapshot,
                recording,
                history,
            );
        }
        self.emit("program finished");
        let snapshot = self.snapshot.clone();
        let recording = self.recording.clone();
        let history = self.history.clone();
        (
            ExitCodes::OK,
            self.io.into_output(),
            false,
            snapshot,
            recording,
            history,
        )
    }

    fn prepare_rebind(&mut self, source: String, target: usize) {
        let target = target.min(self.recording.acts.len());
        self.recording.acts.truncate(target);
        self.writes
            .retain(|write| write.sequence <= target as u64);
        self.src = source;
        self.history = DebugHistory::from_recorded_run(
            &RecordedRun {
                acts: self.recording.acts[..target].to_vec(),
                decision_ledger: self.recording.decision_ledger.clone(),
            },
            jet_foundation::SHA256::sha256_hex(format!("{}:{}", self.source_path, self.src).as_bytes()),
            self.checkpoint_shape.clone(),
        );
        self.history_cursor = None;
        self.pending = None;
        self.stack.clear();
        self.snapshot = None;
        self.started = false;
        self.resume = Resume::Continue;
        self.fix_request = None;
        self.rebind_remaining = (target > 0).then_some(target);
        self.rebind_prefix.clear();
        self.rebind_reached = target == 0;
        self.statement_sequence = target as u64;
    }

    fn cmd_break(&mut self, arg: Option<&str>) {
        match arg.and_then(|a| a.parse::<usize>().ok()) {
            Some(n) if n >= 1 => {
                self.breakpoints.insert(n);
                self.emit(&format!("breakpoint set  {}:{}", self.file, n));
            }
            _ => self.emit("break needs a line number, e.g. `break 7`"),
        }
    }

    fn cmd_list(&mut self, line: usize) {
        let lo = line.saturating_sub(2).max(1);
        let hi = line + 2;
        for l in lo..=hi {
            let text = self.source_line(l);
            if text.is_empty() && l > line {
                break;
            }
            let marker = if l == line { "->" } else { "  " };
            self.emit(&format!("{} {} | {}", marker, l, text.trim_end()));
        }
    }

    fn cmd_print(&mut self, arg: Option<&str>, scope: &HashMap<String, CtValue>) {
        match arg {
            Some(name) => match scope.get(name) {
                Some(v) => {
                    let line = format!("{} = {}", name, v.jet_show());
                    self.emit(&line);
                }
                None => self.emit(&format!("no local named `{}` in this frame", name)),
            },
            None => self.emit("print needs a name, e.g. `print total`"),
        }
    }

    fn cmd_backtrace(&mut self) {
        // Innermost frame first (lldb `bt` order): index 0 = where we stopped.
        let frames: Vec<String> = self
            .stack
            .iter()
            .rev()
            .enumerate()
            .map(|(i, f)| format!("#{}  {}()  at {}:{}", i, f.func, self.file, f.line))
            .collect();
        let joined = frames.join("\n");
        self.emit(&joined);
    }

    fn event_location(&self, event: &DebugEvent) -> String {
        let source = self.source_line(event.line).trim();
        format!(
            "act {}: {}() at {}:{} — {}",
            event.sequence, event.function, self.file, event.line, source
        )
    }

    fn cmd_why(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            self.emit("why needs a query, e.g. `why total == 0`");
            return;
        }
        let (name, expected) = query
            .split_once("==")
            .map(|(name, value)| (name.trim(), Some(value.trim())))
            .unwrap_or((query, None));
        if name.is_empty() {
            self.emit("why needs a place name, e.g. `why total == 0`");
            return;
        }
        let mut answer = Vec::new();
        let mut previous: Option<&str> = None;
        for event in &self.history.events {
            let Some(value) = event.locals.iter().find(|local| local.name == name) else {
                continue;
            };
            let changed = previous != Some(value.value.as_str());
            if changed && expected.map_or(true, |expected| expected == value.value) {
                answer.push(format!(
                    "{} = {} because {}",
                    value.name,
                    value.value,
                    self.event_location(event)
                ));
            }
            previous = Some(value.value.as_str());
        }
        if answer.is_empty() {
            self.emit(&format!("no recorded act explains `{query}`"));
        } else {
            self.emit(&answer.join("\n"));
        }
    }

    fn cmd_when(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.emit("when needs a place name, e.g. `when total`");
            return;
        }
        let mut previous: Option<&str> = None;
        let mut changes = Vec::new();
        for event in &self.history.events {
            let Some(value) = event.locals.iter().find(|local| local.name == name) else {
                continue;
            };
            if previous != Some(value.value.as_str()) {
                changes.push(format!(
                    "{} = {} at {}",
                    value.name,
                    value.value,
                    self.event_location(event)
                ));
                previous = Some(value.value.as_str());
            }
        }
        match changes.last() {
            Some(last) => self.emit(&format!("{}\nlast change: {}", changes.join("\n"), last)),
            None => self.emit(&format!("no recorded changes for `{name}`")),
        }
    }

    fn cmd_help(&mut self) {
        let text = "\
commands:
  step, s        run the next line (descend into calls)
  next, n        run the next line (step over calls)
  finish, f      run to the end of this function
  continue, c    run to the next breakpoint
  back           reverse one recorded statement
  reverse-continue
                 reverse to the previous breakpoint or watchpoint
  watch X        stop when session place X changes
  unwatch X      clear a session watchpoint
  fix            check an edited source file and continue compatibly
  break N, b N   set a breakpoint on line N
  list, l        show the source around the current line
  print X, p X   show the value of local X
  locals         show every local in this frame
  backtrace, bt  show the Jet call stack
  why X == V     name the recorded act(s) that produced a value
  when X         show recorded changes and the last change for a place
  help, h        show this list
  quit, q        end the debug session";
        self.emit(text);
    }
    fn stop_at(
        &mut self,
        func: &str,
        depth: usize,
        line: usize,
        span: Span,
        scope: &HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        self.snapshot = Some(DebugSnapshot {
            function: func.to_string(),
            line,
            locals: snapshot_values(scope),
            call_stack: self
                .stack
                .iter()
                .map(|frame| FrameSnapshot {
                    function: frame.func.clone(),
                    line: frame.line,
                })
                .collect(),
        });
        self.show_stop(line, scope);
        self.prompt_loop(line, depth, scope);
        if self.fix_request.is_some() {
            return Err(Diagnostic::error(
                "E2205",
                "debug checkpoint is ready for a checked source repair".to_string(),
                "the debugger will replay the compatible prefix before continuing".to_string(),
                "edit the source only after a stopped statement and use `fix` again if it changes shape"
                    .to_string(),
                Some(span),
            ));
        }
        if self.paused {
            return Err(Diagnostic::error(
                "E2204",
                "debug session paused at the Canvas command boundary".to_string(),
                "Canvas keeps the source-level debugger stopped until the next command arrives"
                    .to_string(),
                "send another Canvas debug command or stop the session".to_string(),
                None,
            ));
        }
        if self.quit {
            return Err(Diagnostic::error(
                "E2204",
                "debug session ended before the program finished".to_string(),
                "you typed `quit` at the `(jet)` prompt, which stops the interpreted run".to_string(),
                "run `jet debug <file>` again and use `continue` to run to the end, or `jet run <file>` to run without the debugger".to_string(),
                Some(span),
            ));
        }
        Ok(())
    }

}

impl DebugHook for Debugger {
    fn at_stmt(
        &mut self,
        func: &str,
        depth: usize,
        span: Span,
        scope: &HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let line = self.line_of(span);
        self.pending = Some(PendingStatement {
            function: func.to_string(),
            line,
            before: snapshot_places(scope, &self.watches),
        });
        // Keep the live call stack in sync: frame `depth` is this function at
        // this line. Drop any deeper frames left by a returned callee, then
        // update the current frame (or push it if we just descended).
        self.stack.truncate(depth + 1);
        if self.stack.len() <= depth {
            self.stack.push(Frame {
                func: func.to_string(),
                line,
            });
        } else if let Some(top) = self.stack.last_mut() {
            top.func = func.to_string();
            top.line = line;
        }

        // A compatible source repair replays the prefix silently. It must not
        // stop on old breakpoints or expose a candidate's duplicate output.
        if self.rebind_remaining.is_some() {
            return Ok(());
        }

        let hit_breakpoint = self.breakpoints.contains(&line);
        let should_stop = match self.resume {
            Resume::Step => true,
            Resume::Next { depth: d } => depth <= d,
            Resume::Finish { depth: d } => depth < d,
            Resume::Continue => false,
        } || hit_breakpoint;

        if !should_stop {
            return Ok(());
        }
        self.stop_at(func, depth, line, span, scope)
    }

    fn after_stmt(
        &mut self,
        func: &str,
        depth: usize,
        span: Span,
        scope: &HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let line = self.line_of(span);
        let pending = self.pending.take();
        let before = pending
            .as_ref()
            .map(|statement| &statement.before)
            .cloned()
            .unwrap_or_default();
        let statement_function = pending
            .as_ref()
            .map_or(func, |statement| statement.function.as_str());
        let statement_line = pending.as_ref().map_or(line, |statement| statement.line);
        let after = snapshot_places(scope, &self.watches);

        if let Some(remaining) = self.rebind_remaining {
            let sequence = u64::try_from(self.rebind_prefix.len() + 1).unwrap_or(u64::MAX);
            self.rebind_prefix.push(RecordedAct {
                sequence,
                function: statement_function.to_string(),
                line: statement_line,
                locals: snapshot_values(scope),
            });
            if remaining == 1 {
                self.recording.acts = std::mem::take(&mut self.rebind_prefix);
                self.writes = derive_recorded_writes(&self.recording, &self.src);
                self.statement_sequence = sequence;
                self.history = DebugHistory::from_recorded_run(
                    &self.recording,
                    jet_foundation::SHA256::sha256_hex(
                        format!("{}:{}", self.source_path, self.src).as_bytes(),
                    ),
                    self.checkpoint_shape.clone(),
                );
                self.rebind_remaining = None;
                self.rebind_reached = true;
                self.resume = Resume::Step;
            } else {
                self.rebind_remaining = Some(remaining - 1);
            }
            return Ok(());
        }
        if self.recording_locked {
            return Ok(());
        }

        self.statement_sequence = self.statement_sequence.saturating_add(1);
        let sequence = self.statement_sequence;
        let writes = diff_snapshots(
            sequence,
            statement_line,
            &before,
            &after,
        );
        let locals = snapshot_values(scope);
        let event_writes = writes
            .iter()
            .map(|write| DebugWrite {
                line: write.line,
                place: write.place.clone(),
                old_value: write.old_value.clone(),
                new_value: write.new_value.clone(),
            })
            .collect::<Vec<_>>();
        let event = DebugEvent {
            sequence,
            function: statement_function.to_string(),
            line: statement_line,
            depth,
            stack: self
                .stack
                .iter()
                .map(|frame| FrameSnapshot {
                    function: frame.func.clone(),
                    line: frame.line,
                })
                .collect(),
            locals: locals.clone(),
            writes: event_writes,
        };
        if !self.history.push(event) {
            self.history_limit_reached = true;
        } else {
            self.recording.acts.push(RecordedAct {
                sequence,
                function: statement_function.to_string(),
                line: statement_line,
                locals,
            });
            self.writes.extend(writes.iter().cloned());
        }
        let watched: Vec<String> = writes
            .iter()
            .filter(|write| {
                self.watches.iter().any(|watch| {
                    watch == &write.place
                        || write.place.starts_with(&format!("{watch}."))
                        || watch.starts_with(&format!("{}.", write.place))
                })
            })
            .map(|write| self.render_write(write))
            .collect();
        if !watched.is_empty() {
            self.emit(&format!(
                "watchpoint hit  {}:{}  ({} watched place change)",
                self.file,
                statement_line,
                watched.len()
            ));
            for write in watched {
                self.emit(&write);
            }
            self.stop_at(func, depth, line, span, scope)?;
        }
        Ok(())
    }
}

/// Render the one-line `locals:` dump (D-DBG3): `locals:  a = 1   b = "hi"`.
/// Names are shown in a stable (sorted) order so the transcript is
/// deterministic. Values come through `jet_show` (I2 — Jet display, not Rust).
fn render_locals(scope: &HashMap<String, CtValue>) -> String {
    if scope.is_empty() {
        return "locals:  (none)".to_string();
    }
    let mut names: Vec<&String> = scope.keys().collect();
    names.sort();
    let body: Vec<String> = names
        .iter()
        .map(|n| format!("{} = {}", n, scope[*n].jet_show()))
        .collect();
    format!("locals:  {}", body.join("   "))
}

fn snapshot_values(scope: &HashMap<String, CtValue>) -> Vec<ValueSnapshot> {
    let mut names: Vec<&String> = scope.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let value = &scope[name];
            ValueSnapshot {
                name: name.clone(),
                type_name: value.jet_type().name(),
                value: value.jet_show(),
            }
        })
        .collect()
}

fn render_snapshot_locals(values: &[ValueSnapshot]) -> String {
    if values.is_empty() {
        return "locals:  (none)".to_string();
    }
    let mut sorted = values.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| left.name.cmp(&right.name));
    let body = sorted
        .into_iter()
        .map(|value| format!("{} = {}", value.name, value.value))
        .collect::<Vec<_>>()
        .join("   ");
    format!("locals:  {body}")
}

fn debug_path_segments(value: &str) -> Option<Vec<String>> {
    let (root, mut suffix) = split_debug_expression(value)?;
    let mut segments = vec![root.to_string()];
    while !suffix.is_empty() {
        if let Some(field) = suffix.strip_prefix('.') {
            let end = field
                .char_indices()
                .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_')
                .map(|(index, _)| index)
                .unwrap_or(field.len());
            if end == 0 {
                return None;
            }
            segments.push(field[..end].to_string());
            suffix = &field[end..];
        } else if let Some(index) = suffix.strip_prefix('[') {
            let end = index.find(']')?;
            if end == 0 || !index[..end].chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            segments.push(format!("[{}]", &index[..end]));
            suffix = &index[end + 1..];
        } else {
            return None;
        }
    }
    Some(segments)
}

fn value_at_debug_path<'a>(
    scope: &'a HashMap<String, CtValue>,
    place: &str,
) -> Option<&'a CtValue> {
    let segments = debug_path_segments(place)?;
    let mut value = scope.get(segments.first()?.as_str())?;
    for segment in segments.iter().skip(1) {
        if let Some(index) = segment
            .strip_prefix('[')
            .and_then(|index| index.strip_suffix(']'))
            .and_then(|index| index.parse::<usize>().ok())
        {
            value = match value {
                CtValue::List(values) => values.get(index)?,
                _ => return None,
            };
            continue;
        }
        value = match value {
            CtValue::Struct { fields, .. } => fields
                .iter()
                .find(|(name, _)| name == segment)
                .map(|(_, value)| value)?,
            CtValue::Enum { args, .. } => args
                .iter()
                .find(|(name, _)| name.as_deref() == Some(segment.as_str()))
                .map(|(_, value)| value)?,
            _ => return None,
        };
    }
    Some(value)
}

fn snapshot_places(
    scope: &HashMap<String, CtValue>,
    watches: &HashSet<String>,
) -> HashMap<String, ValueSnapshot> {
    let mut values = snapshot_values(scope)
        .into_iter()
        .map(|value| (value.name.clone(), value))
        .collect::<HashMap<_, _>>();
    for place in watches {
        if let Some(value) = value_at_debug_path(scope, place) {
            values.insert(
                place.clone(),
                ValueSnapshot {
                    name: place.clone(),
                    type_name: value.jet_type().name(),
                    value: value.jet_show(),
                },
            );
        }
    }
    values
}

fn snapshot_map(values: &[ValueSnapshot]) -> HashMap<String, ValueSnapshot> {
    values
        .iter()
        .cloned()
        .map(|value| (value.name.clone(), value))
        .collect()
}

fn diff_snapshots(
    sequence: u64,
    line: usize,
    before: &HashMap<String, ValueSnapshot>,
    after: &HashMap<String, ValueSnapshot>,
) -> Vec<IndexedWrite> {
    let mut names = before
        .keys()
        .cloned()
        .chain(after.keys().cloned())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter_map(|place| {
            let old = before.get(&place);
            let new = after.get(&place);
            let changed = match (old, new) {
                (Some(old), Some(new)) => {
                    old.type_name != new.type_name || old.value != new.value
                }
                (None, None) => false,
                _ => true,
            };
            changed.then(|| IndexedWrite {
                sequence,
                line,
                place,
                old_value: old.map_or_else(|| "<unbound>".to_string(), |value| value.value.clone()),
                new_value: new.map_or_else(|| "<unbound>".to_string(), |value| value.value.clone()),
            })
        })
        .collect()
}

fn derive_recorded_writes(run: &RecordedRun, _src: &str) -> Vec<IndexedWrite> {
    let mut writes = Vec::new();
    for (index, act) in run.acts.iter().enumerate() {
        let before = if index > 0
            && run
                .acts
                .get(index - 1)
                .is_some_and(|previous| previous.function == act.function)
        {
            run.acts
                .get(index - 1)
                .map_or_else(HashMap::new, |previous| snapshot_map(&previous.locals))
        } else {
            HashMap::new()
        };
        let after = snapshot_map(&act.locals);
        writes.extend(diff_snapshots(act.sequence, act.line, &before, &after));
    }
    writes
}
fn debug_writes_by_sequence(run: &RecordedRun) -> HashMap<u64, Vec<DebugWrite>> {
    let mut grouped = HashMap::new();
    for write in derive_recorded_writes(run, "") {
        grouped
            .entry(write.sequence)
            .or_insert_with(Vec::new)
            .push(DebugWrite {
                line: write.line,
                place: write.place,
                old_value: write.old_value,
                new_value: write.new_value,
            });
    }
    grouped
}


struct RecordingHook {
    src: String,
    history: DebugHistory,
}

impl DebugHook for RecordingHook {
    fn at_stmt(
        &mut self,
        _func: &str,
        _depth: usize,
        _span: Span,
        _scope: &HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        Ok(())
    }

    fn after_stmt(
        &mut self,
        func: &str,
        depth: usize,
        span: Span,
        scope: &HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let sequence = u64::try_from(self.history.events.len() + 1).unwrap_or(u64::MAX);
        let line = span_line_col(&self.src, span.start).0;
        let event = DebugEvent {
            sequence,
            function: func.to_string(),
            line,
            depth,
            stack: vec![FrameSnapshot {
                function: func.to_string(),
                line,
            }],
            locals: snapshot_values(scope),
            writes: Vec::new(),
        };
        self.history.push(event);
        Ok(())
    }
}

/// Output and causal receipt produced by the source-level recording adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedExecution {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub run: RecordedRun,
    pub history: DebugHistory,
}

/// Run the interpreter once to produce the act snapshots attached to a named
/// `--record` receipt. Unsupported programs return `Err` so the normal run
/// path remains responsible for their existing execution adapter.
pub fn record_run(file: &str, program_args: &[&str]) -> Result<RecordedExecution, String> {
    jet_driver::run_compiler_work(|| {
        let mut bundle = crate::Loader::load_entry(file).map_err(|_| "load failed".to_string())?;
        let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
        if diags
            .iter()
            .any(|diag| matches!(diag.severity, crate::Diagnostics::Severity::Error))
        {
            return Err("semantic check failed".to_string());
        }
        if jet_driver::InterpreterBoundary::debug_boundary_scan(&bundle).is_some() {
            return Err("program is outside the source recording boundary".to_string());
        }
        let funcs = collect_funcs(&bundle);
        if !program_args.is_empty() && funcs.values().any(|function| function.is_job) {
            return Err("job dispatch is outside the source recording boundary".to_string());
        }
        let main = funcs
            .get("run")
            .copied()
            .ok_or_else(|| "program has no `run` function".to_string())?;
        let core_imports: HashMap<String, String> =
            jet_driver::Codegen::core_imports_for_bundle(&bundle)
                .into_iter()
                .collect();
        let src = bundle.modules[bundle.entry].source.clone();
        let mut sink = DevSink::new();
        let mut hook = RecordingHook {
            src: src.clone(),
            history: DebugHistory {
                events: Vec::new(),
                checkpoint: Some(DebugCheckpoint {
                    source_identity: jet_foundation::SHA256::sha256_hex(
                        format!("{}:{}", file, src).as_bytes(),
                    ),
                    shape: checkpoint_shape(&bundle),
                    sequence: 0,
                }),
                cursor: None,
                capped: false,
                decision_ledger: None,
            },
        };
        let argv = std::iter::once(file.to_string())
            .chain(program_args.iter().map(|arg| (*arg).to_string()))
            .collect::<Vec<_>>();
        let result = crate::Comptime::with_runtime_argv(&argv, || {
            crate::Comptime::run_main_debug(
                main,
                &funcs,
                &bundle.project_root,
                &core_imports,
                &mut sink,
                &mut hook,
            )
        });
        let exit_code = sink.exit_code.unwrap_or_else(|| {
            if result.is_ok() {
                ExitCodes::OK
            } else {
                ExitCodes::USER_ERROR
            }
        });
        let mut stderr = sink.stderr;
        if let Err(diag) = result {
            stderr.push_str(&format!("[{}] {}\n", diag.code, diag.what));
        }
        let mut history = hook.history;
        let writes_by_sequence = debug_writes_by_sequence(&history.to_recorded_run());
        for event in &mut history.events {
            event.writes = writes_by_sequence
                .get(&event.sequence)
                .cloned()
                .unwrap_or_default();
        }
        let run = history.to_recorded_run();
        Ok(RecordedExecution {
            stdout: sink.stdout,
            stderr,
            exit_code,
            run,
            history,
        })
    })
}

/// Load + check `file`, then run it under the debugger driving `io`. Returns
/// the process exit code and the captured transcript (empty in interactive
/// mode, where output already went to the terminal).
fn run_with_io(
    file: &str,
    mut io: IO,
    recording: Option<RecordedRun>,
) -> (i32, String, bool, Option<DebugSnapshot>, RecordedRun, DebugHistory) {
    let scripted = io.is_scripted();
    let checked = jet_driver::run_compiler_work(|| {
        let mut bundle = crate::Loader::load_entry(file).map_err(|diags| (None, diags))?;
        let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
        let errors: Vec<Diagnostic> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
            .collect();
        if errors.is_empty() {
            Ok(bundle)
        } else {
            let src = bundle.modules[bundle.entry].source.clone();
            Err((Some(src), errors))
        }
    });
    let bundle = match checked {
        Ok(bundle) => bundle,
        Err((None, diags)) => {
            for d in &diags {
                let line = format!("error [{}]: {}", d.code, d.what);
                if scripted {
                    io.push_line(&line);
                } else {
                    eprintln!("{}", line);
                }
            }
            return (
                ExitCodes::USER_ERROR,
                io.into_output(),
                false,
                None,
                RecordedRun::default(),
                DebugHistory::default(),
            );
        }
        Err((Some(src), errors)) => {
            emit_diags(&mut io, file, &src, &errors);
            return (
                ExitCodes::USER_ERROR,
                io.into_output(),
                false,
                None,
                RecordedRun::default(),
                DebugHistory::default(),
            );
        }
    };
    // The debugger steps the dev interpreter, so live sessions decline the
    // same features `jet dev` does — but with E2203 (debug-specific): names
    // `jet debug` and points at the real build (D-DBG3 step 2, the shipped
    // native backend). A replay receipt is different: it is an authoritative
    // source history, and the checked source is loaded only for identity and
    // line/shape validation. Do not reject (or execute) a replay merely
    // because the current source has a native-only live boundary.
    if recording.is_none() {
        if let Some(b) = jet_driver::InterpreterBoundary::debug_boundary_scan(&bundle) {
            emit_diags(&mut io, file, &bundle.modules[bundle.entry].source, &[b]);
            return (
                ExitCodes::USER_ERROR,
                io.into_output(),
                false,
                None,
                RecordedRun::default(),
                DebugHistory::default(),
            );
        }
    }
    run_checked(&bundle, file, io, recording)
}

fn emit_diags(io: &mut IO, file: &str, src: &str, diags: &[Diagnostic]) {
    let rendered = crate::Diagnostics::render_all(file, src, diags);
    match io {
        IO::Scripted { out, .. } => out.push_str(&rendered),
        IO::Interactive => eprint!("{}", rendered),
    }
}

fn function_checkpoint_shape(function: &crate::AST::Func) -> String {
    let type_params = function
        .type_params
        .iter()
        .map(|param| format!("{}:{:?}", param.name, param.bounds))
        .collect::<Vec<_>>()
        .join(",");
    let params = function
        .params
        .iter()
        .map(|param| {
            format!(
                "{}:{}:{}:{:?}:{}:{}",
                param.name,
                param.ty.identity_key(),
                param.convention.sigil(),
                param.zone,
                param.root,
                param.variadic
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let effects = function
        .declared_effects
        .as_ref()
        .map(|effects| {
            effects
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let return_type = function
        .return_type
        .as_ref()
        .map_or_else(|| "()".to_string(), |ty| ty.identity_key());
    format!(
        "fn:{}<{}>({})->{};unsafe={};pure={};reactive={};effects={}",
        function.name,
        type_params,
        params,
        return_type,
        function.is_unsafe,
        function.is_pure,
        function.is_reactive,
        effects
    )
}

fn variant_checkpoint_shape(variant: &crate::AST::Variant) -> String {
    let payload = match &variant.payload {
        crate::AST::VariantPayload::Unit => "unit".to_string(),
        crate::AST::VariantPayload::Single(ty, _) => format!("single:{}", ty.identity_key()),
        crate::AST::VariantPayload::Named(fields) => fields
            .iter()
            .map(|field| format!("{}:{}", field.name, field.ty.identity_key()))
            .collect::<Vec<_>>()
            .join(","),
    };
    format!(
        "{}:{}:{}",
        variant.name,
        payload,
        variant
            .discriminant
            .map_or_else(|| "_".to_string(), |value| value.to_string())
    )
}

fn checkpoint_shape(bundle: &crate::AST::ProgramBundle) -> String {
    let mut rows = Vec::new();
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let module_identity = if module_index == bundle.entry {
            "entry".to_string()
        } else {
            "imported".to_string()
        };
        rows.push(format!("module:{}:{}", module.display, module_identity));
        for item in &module.items {
            match item {
                crate::AST::Item::Func(function) => rows.push(format!(
                    "{}:{}",
                    module.display,
                    function_checkpoint_shape(function)
                )),
                crate::AST::Item::Struct(structure) => {
                    let fields = structure
                        .fields
                        .iter()
                        .map(|field| {
                            format!(
                                "{}:{}:{}:{}",
                                field.name,
                                field.ty.identity_key(),
                                field.computed.is_some(),
                                field.default.is_some()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    rows.push(format!(
                        "{}:struct:{}<{}>:{}:layout={:?}",
                        module.display,
                        structure.name,
                        structure
                            .type_params
                            .iter()
                            .map(|param| format!("{}:{:?}", param.name, param.bounds))
                            .collect::<Vec<_>>()
                            .join(","),
                        fields,
                        structure.layout
                    ));
                }
                crate::AST::Item::Enum(enumeration) => rows.push(format!(
                    "{}:enum:{}<{}>:{}",
                    module.display,
                    enumeration.name,
                    enumeration
                        .type_params
                        .iter()
                        .map(|param| format!("{}:{:?}", param.name, param.bounds))
                        .collect::<Vec<_>>()
                        .join(","),
                    enumeration
                        .variants
                        .iter()
                        .map(variant_checkpoint_shape)
                        .collect::<Vec<_>>()
                        .join(",")
                )),
                crate::AST::Item::Distinct(distinct) => rows.push(format!(
                    "{}:distinct:{}:{}",
                    module.display,
                    distinct.name,
                    distinct.base.identity_key()
                )),
                crate::AST::Item::TypeAlias(alias) => rows.push(format!(
                    "{}:alias:{}:{}",
                    module.display,
                    alias.name,
                    alias.target.identity_key()
                )),
                crate::AST::Item::Impl(implementation) => rows.push(format!(
                    "{}:impl:{}:{}:{}:{}",
                    module.display,
                    implementation.type_name,
                    implementation.trait_name.as_deref().unwrap_or(""),
                    implementation
                        .operator_rhs
                        .as_ref()
                        .map_or_else(|| "_".to_string(), |ty| ty.identity_key()),
                    implementation
                        .methods
                        .iter()
                        .map(function_checkpoint_shape)
                        .collect::<Vec<_>>()
                        .join(",")
                )),
                other => rows.push(format!(
                    "{}:item:{:?}",
                    module.display,
                    std::mem::discriminant(other)
                )),
            }
        }
    }
    rows.sort();
    rows.join("|")
}

fn diagnostic_reason(diags: &[Diagnostic]) -> String {
    diags.first().map_or_else(
        || "source check failed without a diagnostic".to_string(),
        |diag| format!("[{}] {}", diag.code, diag.what),
    )
}

fn checked_bundle_for_fix(
    file: &str,
) -> Result<(crate::AST::ProgramBundle, String, String), String> {
    let mut bundle = crate::Loader::load_entry(file).map_err(|diags| diagnostic_reason(&diags))?;
    let diagnostics = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.severity,
                crate::Diagnostics::Severity::Error
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(diagnostic_reason(&errors));
    }
    if let Some(boundary) = jet_driver::InterpreterBoundary::debug_boundary_scan(&bundle) {
        return Err(format!("[{}] {}", boundary.code, boundary.what));
    }
    let source = bundle.modules[bundle.entry].source.clone();
    let shape = checkpoint_shape(&bundle);
    Ok((bundle, source, shape))
}

pub(crate) fn native_checkpoint_shape(file: &str, source: &str) -> String {
    checked_bundle_for_fix(file)
        .ok()
        .filter(|(_, candidate_source, _)| candidate_source == source)
        .map(|(_, _, shape)| shape)
        .unwrap_or_else(|| "native-debug-artifact".to_string())
}

/// Drive the interpreter with the debugger hook attached, then flush the
/// program's own stdout/stderr around the debugger's `(jet)` session.
fn run_checked(
    bundle: &crate::AST::ProgramBundle,
    file: &str,
    mut io: IO,
    recording: Option<RecordedRun>,
) -> (i32, String, bool, Option<DebugSnapshot>, RecordedRun, DebugHistory) {
    let funcs = collect_funcs(bundle);
    if !funcs.contains_key("run") {
        // I4 (#2029): a registered diagnostic, not a bare line.
        let diag = crate::Sema::Diagnostics::render_registered(
            "E0101",
            "this program has no `run` function".to_string(),
            "running a program starts at `fn run`, and the entry file doesn't define one"
                .to_string(),
            "add `fn run() { … }`, or use `jet check <file>`".to_string(),
            None,
        );
        let src = bundle.modules[bundle.entry].source.clone();
        emit_diags(&mut io, file, &src, &[diag]);
        return (
            ExitCodes::USER_ERROR,
            io.into_output(),
            false,
            None,
            RecordedRun::default(),
            DebugHistory::default(),
        );
    }

    let src = bundle.modules[bundle.entry].source.clone();
    let shape = checkpoint_shape(bundle);
    let short = std::path::Path::new(file)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string());
    let scripted = io.is_scripted();
    let mut dbg = Debugger::new(
        src,
        short,
        file.to_string(),
        io,
        recording,
        shape.clone(),
    );
    if dbg.replay_mode {
        return dbg.run_replay();
    }

    let mut active_bundle: Option<crate::AST::ProgramBundle> = None;
    let mut sink = DevSink::new();
    let result: Result<(), Diagnostic> = loop {
        let current_result = {
            let current_bundle = active_bundle.as_ref().unwrap_or(bundle);
            let funcs = collect_funcs(current_bundle);
            let Some(main) = funcs.get("run").copied() else {
                break Err(Diagnostic::error(
                    "E2205",
                    "fix removed the `run` entry function".to_string(),
                    "a source repair must preserve the paused function and its callable shape"
                        .to_string(),
                    "restore `fn run` or restart the debug session".to_string(),
                    None,
                ));
            };
            let base_dir = &current_bundle.project_root;
            let core_imports: HashMap<String, String> =
                jet_driver::Codegen::core_imports_for_bundle(current_bundle)
                    .into_iter()
                    .collect();
            sink = DevSink::new();
            crate::Comptime::run_main_debug(
                main,
                &funcs,
                base_dir,
                &core_imports,
                &mut sink,
                &mut dbg,
            )
        };
        if active_bundle.is_some() && !dbg.rebind_reached {
            dbg.emit("fix rejected: compatible checkpoint was not reached by the edited source");
            break Err(Diagnostic::error(
                "E2205",
                "debug source repair could not reach its checkpoint".to_string(),
                "the edited source stopped before the recorded prefix completed".to_string(),
                "restore the paused control-flow prefix or restart the debug session".to_string(),
                None,
            ));
        }
        let Some(request) = dbg.fix_request.take() else {
            break current_result;
        };
        let (candidate_bundle, candidate_source, candidate_shape) =
            match checked_bundle_for_fix(file) {
                Ok(value) => value,
                Err(reason) => {
                    dbg.emit(&format!("fix rejected: {reason}"));
                    break Err(Diagnostic::error(
                        "E2205",
                        "debug source repair could not be rebound".to_string(),
                        reason,
                        "leave the debugger paused and correct the source shape before retrying"
                            .to_string(),
                        None,
                    ));
                }
            };
        if candidate_source != request.source {
            let reason = "the source changed again while the repair was being checked";
            dbg.emit(&format!("fix rejected: {reason}"));
            break Err(Diagnostic::error(
                "E2205",
                "debug source repair could not be rebound".to_string(),
                reason.to_string(),
                "retry `fix` from the same stopped checkpoint".to_string(),
                None,
            ));
        }
        if candidate_shape != shape {
            let reason = "checkpoint incompatible (source type, layout, or authority changed)";
            dbg.emit(&format!("fix rejected: {reason}"));
            break Err(Diagnostic::error(
                "E2205",
                "debug source repair changed the checkpoint shape".to_string(),
                reason.to_string(),
                "restart the debug session after changing a declaration or authority boundary"
                    .to_string(),
                None,
            ));
        }
        dbg.prepare_rebind(candidate_source, request.target);
        active_bundle = Some(candidate_bundle);
    };

    // Flush the program's buffered output, then a completion line, then return
    // the captured transcript (scripted) or write the terminal (interactive).
    let code = match &result {
        Ok(()) => ExitCodes::OK,
        Err(_) => ExitCodes::USER_ERROR,
    };
    if scripted {
        if !sink.stdout.is_empty() {
            dbg.io.push_line(sink.stdout.trim_end_matches('\n'));
        }
        if !sink.stderr.is_empty() {
            dbg.io.push_line(sink.stderr.trim_end_matches('\n'));
        }
        if dbg.paused {
            let snapshot = dbg.snapshot.clone();
            let recording = dbg.recording.clone();
            let history = dbg.history.clone();
            return (
                ExitCodes::OK,
                dbg.io.into_output(),
                true,
                snapshot,
                recording,
                history,
            );
        }
        match &result {
            Ok(()) => dbg.io.push_line("program finished"),
            Err(d) => dbg.io.push_line(&format!("[{}] {}", d.code, d.what)),
        }
        let snapshot = dbg.snapshot.clone();
        let recording = dbg.recording.clone();
        let history = dbg.history.clone();
        (
            code,
            dbg.io.into_output(),
            false,
            snapshot,
            recording,
            history,
        )
    } else {
        print!("{}", sink.stdout);
        eprint!("{}", sink.stderr);
        match &result {
            Ok(()) => println!("program finished"),
            Err(d) => eprintln!("[{}] {}", d.code, d.what),
        }
        let snapshot = dbg.snapshot.clone();
        let recording = dbg.recording.clone();
        let history = dbg.history.clone();
        (code, String::new(), false, snapshot, recording, history)
    }
}

/// Collect top-level functions into the flat name→func map the interpreter
/// expects (mirrors `Interpreter::collect_funcs`).
fn collect_funcs(bundle: &crate::AST::ProgramBundle) -> HashMap<String, &crate::AST::Func> {
    let mut funcs = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Func(f) = item {
                funcs.entry(f.name.clone()).or_insert(f);
            }
        }
    }
    funcs
}

/// `jet debug <file>` — the interactive entry point (D-DBG1). Loads, checks,
/// and steps the program with a live `(jet)` prompt on stdin/stdout. Returns
/// the process exit code.
pub fn run_debug(file: &str) -> i32 {
    let (code, _captured, _paused, _snapshot, _recording, _history) =
        run_with_io(file, IO::Interactive, None);
    code
}

/// Run one interactive debugger session and return the bounded act history
/// produced by that same execution for a named replay capture.
pub fn run_debug_recorded(file: &str) -> RecordedExecution {
    let (exit_code, _captured, _paused, _snapshot, run, history) =
        run_with_io(file, IO::Interactive, None);
    RecordedExecution {
        stdout: String::new(),
        stderr: String::new(),
        exit_code,
        run,
        history,
    }
}

/// `jet debug --replay` query surface. The receipt's act snapshots are
/// authoritative history: navigation never re-executes the recorded program.
pub fn run_debug_with_recording(file: &str, recording: RecordedRun) -> i32 {
    let (code, _captured, _paused, _snapshot, _recording, _history) =
        run_with_io(file, IO::Interactive, Some(recording));
    code
}

/// Machine-readable state for a scripted debug session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    Running,
    Finished,
    Failed,
}

/// Structured result for embedders that must not infer state from program output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionResult {
    pub status: SessionStatus,
    pub transcript: String,
    pub snapshot: Option<DebugSnapshot>,
    pub history: DebugHistory,
}

/// Scripted debug session with a machine-readable outcome.
pub fn run_session_result(file: &str, inputs: &[&str]) -> SessionResult {
    jet_driver::run_compiler_work(|| {
        let queue: std::collections::VecDeque<String> =
            inputs.iter().map(|s| s.to_string()).collect();
        let io = IO::Scripted {
            inputs: queue,
            out: String::new(),
            pause_on_input_end: false,
        };
        let (code, transcript, _paused, snapshot, _recording, history) =
            run_with_io(file, io, None);
        let status = if code == ExitCodes::OK {
            SessionStatus::Finished
        } else {
            SessionStatus::Failed
        };
        SessionResult {
            status,
            transcript,
            snapshot,
            history,
        }
    })
}

/// One live Canvas command boundary. The debugger pauses at the next stopped
/// Jet source statement when `inputs` end, instead of treating EOF as
/// `continue`. The Canvas server replays the bounded command history to resume
/// the same deterministic source session on the next request.
pub fn run_session_result_paused(file: &str, inputs: &[&str]) -> SessionResult {
    jet_driver::run_compiler_work(|| {
        let queue: std::collections::VecDeque<String> =
            inputs.iter().map(|s| s.to_string()).collect();
        let io = IO::Scripted {
            inputs: queue,
            out: String::new(),
            pause_on_input_end: true,
        };
        let (code, transcript, paused, snapshot, _recording, history) =
            run_with_io(file, io, None);
        let status = if paused {
            SessionStatus::Running
        } else if code == ExitCodes::OK {
            SessionStatus::Finished
        } else {
            SessionStatus::Failed
        };
        SessionResult {
            status,
            transcript,
            snapshot,
            history,
        }
    })
}
/// Record one complete source session. The returned history is the same event
/// stream produced by that execution; replay consumers must not execute the
/// source a second time to build their index.
pub fn run_session_recorded(file: &str) -> SessionResult {
    run_session_result(file, &[Syntax::DBG_CONTINUE])
}

/// Replay an existing source history using only debugger commands. The source
/// is loaded for line/shape validation, but no program execution occurs.
pub fn run_session_replay(
    file: &str,
    history: &DebugHistory,
    inputs: &[&str],
) -> SessionResult {
    jet_driver::run_compiler_work(|| {
        let queue: std::collections::VecDeque<String> =
            inputs.iter().map(|s| s.to_string()).collect();
        let io = IO::Scripted {
            inputs: queue,
            out: String::new(),
            pause_on_input_end: true,
        };
        let (code, transcript, paused, snapshot, _recording, replay_history) =
            run_with_io(file, io, Some(history.to_recorded_run()));
        let status = if paused {
            SessionStatus::Running
        } else if code == ExitCodes::OK {
            SessionStatus::Finished
        } else {
            SessionStatus::Failed
        };
        SessionResult {
            status,
            transcript,
            snapshot,
            history: if replay_history.events.is_empty() {
                history.clone()
            } else {
                replay_history
            },
        }
    })
}

/// Scripted debug session for tests and golden transcripts. Feeds `inputs` to
/// the `(jet)` prompt in order and returns the captured transcript (banners,
/// `locals:` dumps, command echoes, program output, and the final marker).
pub fn run_session(file: &str, inputs: &[&str]) -> String {
    run_session_result(file, inputs).transcript
}

/// D-DBG3 step 2 (dap-debugger): whether this program needs the native backend
/// — the interpreter's boundary scan (E2203) found an FFI/task/`#Unsafe`/
/// native-std construct step-1 can't step through. `None` means the file
/// couldn't even be loaded; the caller should fall through to [`run_debug`],
/// which reports that error the normal way (no duplicated error path here).
pub fn needs_native(file: &str) -> Option<bool> {
    let bundle = crate::Loader::load_entry(file).ok()?;
    Some(jet_driver::InterpreterBoundary::debug_boundary_scan(&bundle).is_some())
}

/// D-DBG3 step 2: the native lldb-backed `(jet)` terminal session — steps the
/// FULL feature set (FFI/tasks/`#Unsafe`/native std) the interpreter declines.
/// `binary` is the already-built debug binary (full debuginfo); `rust_file`/
/// `rust_src` are the generated Rust this build produced (with `// jet:line N`
/// markers from `emit_bundle_dbg`); `jet_file`/`jet_src` are the original
/// source. `raw_frames` is the D-DBG2 expert opt-in (`--raw-frames`).
pub fn run_native(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
) -> i32 {
    Native::run(binary, rust_file, rust_src, jet_file, jet_src, raw_frames)
}

/// Scripted native session for tests (mirrors [`run_session`] for the native
/// backend): feeds `inputs` to the `(jet)` prompt and returns the transcript.
/// Callers should gate on lldb availability themselves before building a debug
/// binary to hand in — this only reports "lldb missing" into the transcript.
#[allow(clippy::too_many_arguments)]
pub fn run_native_scripted(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
) -> String {
    Native::run_scripted(
        binary, rust_file, rust_src, jet_file, jet_src, raw_frames, inputs,
    )
}

/// Scripted native session with one live command boundary. It uses the same
/// structured status contract as [`run_session_result_paused`], but the
/// execution is delegated to the compiled binary through the native lldb
/// adapter.
#[allow(clippy::too_many_arguments)]
pub fn run_native_session_result_paused(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
) -> SessionResult {
    run_native_session_result_with_mode(
        binary, rust_file, rust_src, jet_file, jet_src, raw_frames, inputs, true,
    )
}

/// Scripted native session with completion semantics matching
/// [`run_session_result`].
#[allow(clippy::too_many_arguments)]
pub fn run_native_session_result(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
) -> SessionResult {
    run_native_session_result_with_mode(
        binary, rust_file, rust_src, jet_file, jet_src, raw_frames, inputs, false,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_native_session_result_with_mode(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
    pause_on_input_end: bool,
) -> SessionResult {
    jet_driver::run_compiler_work(|| {
        let (code, transcript, paused, snapshot, history) =
            Native::run_scripted_mode_with_history(
                binary,
                rust_file,
                rust_src,
                jet_file,
                jet_src,
                raw_frames,
                inputs,
                pause_on_input_end,
            );
        let status = if paused {
            SessionStatus::Running
        } else if code == ExitCodes::OK {
            SessionStatus::Finished
        } else {
            SessionStatus::Failed
        };
        SessionResult {
            status,
            transcript,
            snapshot,
            history,
        }
    })
}
/// Replay a native source history without relaunching the inferior. This is
/// used by Canvas requests after the first live native capture.
pub fn run_native_session_replay(
    jet_file: &str,
    jet_src: &str,
    history: &DebugHistory,
    inputs: &[&str],
    pause_on_input_end: bool,
) -> SessionResult {
    let (code, transcript, paused, snapshot, history) =
        Native::replay_history(jet_file, jet_src, history, inputs, pause_on_input_end);
    let status = if paused {
        SessionStatus::Running
    } else if code == ExitCodes::OK {
        SessionStatus::Finished
    } else {
        SessionStatus::Failed
    };
    SessionResult {
        status,
        transcript,
        snapshot,
        history,
    }
}


/// D-DBG3 step 2: the DAP JSON-over-stdio server (editor wiring) — same native
/// backend as [`run_native`], speaking the Debug Adapter Protocol on stdin/
/// stdout instead of the `(jet)` terminal prompt.
pub fn run_dap(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
) -> i32 {
    Dap::run(binary, rust_file, rust_src, jet_file, jet_src)
}
