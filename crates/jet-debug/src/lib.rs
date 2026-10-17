//! D-DBG1 / D-DBG3 — `jet debug <file>`: the source-level step debugger.
//!
//! The debugger steps your program at the **Jet source level** (I2): every
//! breakpoint, step, frame, and value is in Jet terms. It never surfaces the
//! generated Rust by default.
//!
//! Backend (D-EXEC1/D-EXEC2): live sessions step the native debug build under
//! LLDB — the `(jet)` terminal session ([`Native`]) or the Debug Adapter
//! Protocol server editors launch ([`Dap`]); [`LineMap`] and [`Inferior`] are
//! their shared building blocks. D-DX-DEBUGBACK1: `jet debug --record` records
//! one complete native run, and the recorded-history browser in this module
//! navigates that receipt (`--replay`, Canvas) without executing anything.
//!
//! Command surface (D-DBG3, ratified A): the prompt is `(jet)`; the step verbs
//! are lldb-familiar `step` / `next` / `continue` / `finish` with single-letter
//! aliases `s` / `n` / `c` / `f`. Recorded sessions add `back` and
//! `reverse-continue`, `why`, and `when`. A paused line shows a `<- here`
//! caret and a one-line `locals:` dump; `help` lists every verb. Only Jet
//! frames/lines and safe locals are shown by default (D-DBG2 — `--raw-frames`
//! is the clearly marked native expert view).
//!
//! I6: std-only — no DAP/JSON crate, no debugger library.

#![allow(non_snake_case)]

// D-ARCH-SOURCE1=A: full debugger ownership lives here. Compiler semantics
// enter through inward path-only seams; no root host dependency exists.
pub use jet_driver::{Diagnostics, Loader, Sema, Syntax, AST};
pub use jet_foundation::ExitCodes;

mod Dap;
mod EventObservation;
mod Inferior;
mod LineMap;
mod Native;
pub use Native::{NativeLiveResult, NativeLiveSession};

pub use EventObservation::render as render_event_observations;

use std::collections::{HashMap, HashSet};

use crate::Diagnostics::Diagnostic;

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

/// Runtime-owned debugger state for the currently selected paused frame.
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

/// One post-statement source event shared by the native adapter,
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
        /// Keep recorded history at the current stop when scripted input ends.
        /// Canvas uses this to expose one command boundary; transcript
        /// tests continue through the remaining recorded events.
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

/// The recorded-history browser (D-DX-DEBUGBACK1). Holds the breakpoint and
/// watch sets, the source text (for the `<- here` caret and `list`), and the
/// authoritative recording it navigates; it never executes the program.
struct Debugger {
    /// Jet line numbers the user set a breakpoint on (`break <line>`).
    breakpoints: HashSet<usize>,
    /// The entry file's source, for the `<- here` caret and the breakpoint banner.
    src: String,
    /// The file name shown in banners (e.g. `loops.jet`).
    file: String,
    /// The call stack shown before the first recorded stop, innermost last.
    stack: Vec<Frame>,
    io: IO,
    /// Set when the user typed `quit`.
    quit: bool,
    /// Set when Canvas input ends at a recorded stop. The paused transcript is
    /// not a diagnostic.
    paused: bool,
    /// Structured state captured at the last recorded stop. This is separate
    /// from the human transcript so program output cannot fabricate liveness.
    snapshot: Option<DebugSnapshot>,
    /// Canonical event/stack/checkpoint/value history shared with native, DAP,
    /// Canvas, and receipt replay consumers.
    history: DebugHistory,
    /// Legacy receipt projection retained for the stable artifact API.
    recording: RecordedRun,
    /// Session-scoped places whose writes stop `continue`.
    watches: HashSet<String>,
    /// `Some(index)` means the prompt is viewing that recorded statement.
    history_cursor: Option<usize>,
    replay_finished: bool,
}

impl Debugger {
    fn new(
        src: String,
        file: String,
        source_path: String,
        io: IO,
        recording: RecordedRun,
        checkpoint_shape: String,
    ) -> Self {
        let source_identity = jet_foundation::SHA256::sha256_hex(
            format!("{}:{}", source_path, src).as_bytes(),
        );
        let history = DebugHistory::from_recorded_run(&recording, source_identity, checkpoint_shape);
        Debugger {
            breakpoints: HashSet::new(),
            src,
            file,
            stack: Vec::new(),
            io,
            quit: false,
            paused: false,
            snapshot: None,
            recording,
            history,
            watches: HashSet::new(),
            history_cursor: None,
            replay_finished: false,
        }
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

    /// Run the `(jet)` prompt over the recording until navigation runs past
    /// its last act, the input ends, or the user quits. `depth` is the call
    /// depth `next`/`finish` start from.
    fn prompt_loop(&mut self, depth: usize) {
        loop {
            let cmd = match self.read_command() {
                Some(c) => c,
                None => {
                    // Canvas owns the next command; transcripts and terminal
                    // EOF finish the browse.
                    if self.io.pause_on_input_end() {
                        self.paused = true;
                    }
                    return;
                }
            };
            if cmd.is_empty() {
                // Bare Enter repeats a single step (lldb behavior).
                if self.advance_history(Resume::Step) {
                    return;
                }
                continue;
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
                        false
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
            if resumed || self.replay_finished {
                return;
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

    /// Move one virtual history step. Returns true when the browse is
    /// complete (navigation ran past the last recorded act).
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
        self.replay_finished = true;
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
    /// A recording is authoritative history; repairing source needs a new
    /// native session (`jet debug <file>`), which rebuilds and relaunches.
    fn cmd_fix(&mut self) {
        self.emit(
            "fix unavailable: replay receipts are read-only; start a live debug session to repair source",
        );
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
        self.prompt_loop(0);
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

fn derive_recorded_writes(run: &RecordedRun) -> Vec<IndexedWrite> {
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
    for write in derive_recorded_writes(run) {
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

/// Output and causal receipt produced by the source-level recording adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedExecution {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub run: RecordedRun,
    pub history: DebugHistory,
}

/// Load + check `file`, then browse its recorded history driving `io`. The
/// receipt's acts are authoritative; the checked source supplies only the
/// line text and the checkpoint shape — nothing is executed. Returns the
/// exit code and the captured transcript (empty in interactive mode).
fn run_with_io(
    file: &str,
    mut io: IO,
    recording: RecordedRun,
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

/// Browse a checked recording at the `(jet)` prompt (D-DX-DEBUGBACK1).
fn run_checked(
    bundle: &crate::AST::ProgramBundle,
    file: &str,
    mut io: IO,
    recording: RecordedRun,
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
    let mut dbg = Debugger::new(src, short, file.to_string(), io, recording, shape);
    dbg.run_replay()
}

/// Collect top-level functions into a flat name→func map (the `run` entry check).
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

/// `jet debug --replay` query surface. The receipt's act snapshots are
/// authoritative history: navigation never re-executes the recorded program.
pub fn run_debug_with_recording(file: &str, recording: RecordedRun) -> i32 {
    let (code, _captured, _paused, _snapshot, _recording, _history) =
        run_with_io(file, IO::Interactive, recording);
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
            run_with_io(file, io, history.to_recorded_run());
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

/// D-DBG3 / D-EXEC2: the native lldb-backed `(jet)` terminal session, the
/// only live `jet debug` backend.
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

/// D-DX-DEBUGBACK1=A: record one complete native run of the already-built
/// debug binary (see [`run_native`] for the arguments) as the replay receipt
/// `jet debug --replay` browses.
pub fn record_native(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
) -> Result<RecordedExecution, String> {
    Native::record(binary, rust_file, rust_src, jet_file, jet_src)
}

/// Scripted native session for tests: feeds `inputs` to the `(jet)` prompt
/// and returns the transcript.
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

/// Scripted native session that reports [`SessionStatus`] alongside the
/// transcript, snapshot, and history.
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
