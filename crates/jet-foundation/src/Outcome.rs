// D-FAIL-CARRIER1=A / D-FAIL-MODEL1=A (ratified 2026-08-06) — the one outcome
// carrier under `T?` and `T ? E`.
//
// An outcome has three independent facts:
//
//   * payload — a value, part of one, or none;
//   * verdict — clean, succeeded with notes, or failed;
//   * reports — the failure report, its cause, and the notes it collected.
//
// `JetOutcome<T, E>` holds the payload on the value side and the report on the
// stop side. The two ratified surface spellings are two views of this one
// carrier, and nothing converts between them because there is nothing to
// convert:
//
//   * `T?`     is `JetOutcome<T, JetAbsent>` — absence is clean, so the report
//              is `JetAbsent`, which carries nothing.
//   * `T ? E`  is `JetOutcome<T, E>` — the report matters, so `E` is the report.
//
// Happy-path erasure: `JetAbsent` is zero-sized, so `JetOutcome<T, JetAbsent>`
// has the same layout, the same niche and the same two branches as a bare
// payload. A verdict nobody reads costs no allocation and no branch.
pub type JetOutcome<T, E> = Result<T, E>;

/// D-ALLOCFAIL1=A (ratified 2026-08-12): the one report returned by every
/// fallible allocation surface. The requested byte count and allocator name
/// are source facts; execution tiers only marshal this value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllocError {
    pub requested_bytes: i64,
    pub allocator: String,
}

pub fn jet_alloc_error(requested_bytes: usize, allocator: &str) -> AllocError {
    AllocError {
        requested_bytes: requested_bytes.min(i64::MAX as usize) as i64,
        allocator: allocator.to_string(),
    }
}

/// The shared fallible-allocation value seam. `used`, `capacity`, and the
/// metadata overhead are allocator facts supplied by the execution adapter;
/// fit testing, charge accounting, and error construction stay in the
/// embedded Prelude rather than in an execution engine.
pub fn jet_try_alloc_value<T>(
    value: T,
    used: usize,
    capacity: usize,
    requested_bytes: usize,
    allocator: &str,
    overhead_bytes: usize,
) -> JetOutcome<(T, usize), AllocError> {
    let requested = requested_bytes.max(1);
    let charge = requested.saturating_add(overhead_bytes);
    if charge > capacity.saturating_sub(used) {
        return Err(jet_alloc_error(requested, allocator));
    }
    Ok((value, used.saturating_add(charge)))
}

// D-FAIL-ERROR1=A (ratified 2026-08-06) — one default error value on every tier.
// Engines marshal the three source fields. Construction, projection, and report
// rendering stay here so no engine owns error meaning.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErr {
    message: String,
    code: JetOutcome<String, JetAbsent>,
    cause: JetOutcome<Box<JetErr>, JetAbsent>,
}

pub fn jet_err(
    message: String,
    code: JetOutcome<String, JetAbsent>,
    cause: JetOutcome<JetErr, JetAbsent>,
) -> JetErr {
    JetErr {
        message,
        code,
        cause: cause.map(Box::new),
    }
}

/// D-FAIL-ERROR1=A: the only String-to-default-error conversion.
pub fn jet_err_from_message(message: String) -> JetErr {
    jet_err(message, Err(JetAbsent), Err(JetAbsent))
}

pub fn jet_err_message(error: &JetErr) -> String {
    error.message.clone()
}

pub fn jet_err_code(error: &JetErr) -> JetOutcome<String, JetAbsent> {
    error.code.clone()
}

pub fn jet_err_cause(error: &JetErr) -> JetOutcome<JetErr, JetAbsent> {
    error.cause.as_ref().map(|cause| (**cause).clone()).map_err(|_| JetAbsent)
}

// D-FAIL-CTX1: shared development journey state and rendering. Each `?`
// adapter claims its source site here, so AOT, JIT, and TIR-eval print one
// journey vocabulary and collapse only consecutive duplicate hops.
thread_local! {
    static JET_JOURNEY_LAST: std::cell::RefCell<Option<(String, String, u32)>> =
        const { std::cell::RefCell::new(None) };
    static JET_JOURNEY_FRAMES: std::cell::RefCell<String> = const {
        std::cell::RefCell::new(String::new())
    };
    static JET_STREAM_FAILURE_REPORT: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

pub fn jet_stream_record_failure_report(report: String) {
    JET_STREAM_FAILURE_REPORT.with(|slot| *slot.borrow_mut() = Some(report));
}

pub fn jet_stream_take_failure_report() -> Option<String> {
    JET_STREAM_FAILURE_REPORT.with(|slot| slot.borrow_mut().take())
}

pub fn jet_journey_reset() {
    JET_JOURNEY_LAST.with(|last| *last.borrow_mut() = None);
    JET_JOURNEY_FRAMES.with(|frames| frames.borrow_mut().clear());
}

pub fn jet_journey_take() -> String {
    JET_JOURNEY_LAST.with(|last| *last.borrow_mut() = None);
    JET_JOURNEY_FRAMES.with(|frames| std::mem::take(&mut *frames.borrow_mut()))
}

pub fn jet_journey_frame<F: FnOnce() -> String>(
    file: &str,
    line: u32,
    fn_name: &str,
    note: F,
) -> Option<String> {
    let site = (fn_name.to_string(), file.to_string(), line);
    let fresh = JET_JOURNEY_LAST.with(|last| {
        let mut slot = last.borrow_mut();
        if slot.as_ref() == Some(&site) {
            false
        } else {
            *slot = Some(site);
            true
        }
    });
    if !fresh {
        return None;
    }
    let note = note();
    let suffix = if note.is_empty() {
        String::new()
    } else {
        format!(": {note}")
    };
    let frame = format!("error propagated from: {fn_name} ({file}:{line}) via ?{suffix}\n");
    JET_JOURNEY_FRAMES.with(|frames| frames.borrow_mut().push_str(&frame));
    Some(frame)
}

pub fn jet_render_err(error: &JetErr) -> String {
    fn render(error: &JetErr, out: &mut String, depth: usize) {
        if depth == 0 {
            match &error.code {
                Ok(code) => out.push_str(&format!("Error [{code}]: {}", error.message)),
                Err(JetAbsent) => out.push_str(&format!("Error: {}", error.message)),
            }
        } else {
            out.push_str(&"  ".repeat(depth));
            out.push_str("cause: ");
            out.push_str(&error.message);
        }
        if let Ok(cause) = &error.cause {
            out.push('\n');
            render(cause, out, depth + 1);
        }
    }

    let mut out = String::new();
    render(error, &mut out, 0);
    out
}

impl std::fmt::Display for JetErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&jet_render_err(self))
    }
}

#[cfg(test)]
mod err_tests {
    use super::*;

    #[test]
    fn default_error_has_ratified_fields_and_report() {
        let cause = jet_err_from_message("unexpected token at line 3".to_string());
        let error = jet_err(
            "parse failed".to_string(),
            Ok("CFG404".to_string()),
            Ok(cause),
        );

        assert_eq!(jet_err_message(&error), "parse failed");
        assert_eq!(jet_err_code(&error), Ok("CFG404".to_string()));
        assert_eq!(
            jet_err_message(&jet_err_cause(&error).expect("cause")),
            "unexpected token at line 3"
        );
        assert_eq!(
            jet_render_err(&error),
            "Error [CFG404]: parse failed\n  cause: unexpected token at line 3"
        );

    }
}

/// The clean report: no payload, nothing to say. This is what `T?` puts on the
/// stop side, which is why an absence is not a failure.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct JetAbsent;

/// D-CONC-FAIL1=A: the one typed failure report for a joined task.  The
/// scheduler produces this value; AOT, JIT, and TIR only marshal it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JetTaskFailure {
    Cancelled,
    DeadlineBlown,
    Panicked(String),
}

impl JetTaskFailure {
    /// One failure message for every runtime adapter. AOT, JIT, and the
    /// interpreter may marshal this value, but they do not invent separate
    /// wording or policy for it.
    pub fn message(&self) -> String {
        match self {
            Self::Cancelled => "task cancelled".to_string(),
            Self::DeadlineBlown => "task deadline exceeded".to_string(),
            Self::Panicked(reason) => reason.clone(),
        }
    }
}

/// The optional view of the carrier: `T?`.
///
/// Every method here reads the same carrier the fallible view reads. They exist
/// because the clean report answers different questions than a failure report,
/// not because the optional is a different type.
pub trait JetOptionalView<T>: Sized {
    /// Is the payload present?
    fn is_some(&self) -> bool;
    /// Is the payload absent?
    fn is_none(&self) -> bool;
    /// Take the payload out, leaving a clean absence behind.
    fn take(&mut self) -> Self;
    /// Put a payload in, handing back whatever was there.
    fn replace(&mut self, value: T) -> Self;
    /// Keep the payload only when it answers the question.
    fn filter(self, keep: impl FnOnce(&T) -> bool) -> Self;
    /// Is the payload present and does it answer the question?
    fn is_some_and(self, ask: impl FnOnce(T) -> bool) -> bool;
    /// D-FAIL-CARRIER1: lift a clean absence into a failure with a report.
    /// The payload rides through untouched; only the verdict changes.
    fn or_err(self, why: String) -> JetOutcome<T, JetErr>;
    /// Read the payload without taking it; the clean report stays clean.
    fn map_ref<U>(&self, f: impl FnOnce(&T) -> U) -> JetOutcome<U, JetAbsent>;
    /// Pair two payloads; absent on either side is absent for the pair.
    fn zip<U>(self, other: JetOutcome<U, JetAbsent>) -> JetOutcome<(T, U), JetAbsent>;
}

impl<T> JetOptionalView<T> for JetOutcome<T, JetAbsent> {
    fn is_some(&self) -> bool {
        self.is_ok()
    }
    fn is_none(&self) -> bool {
        self.is_err()
    }
    fn take(&mut self) -> Self {
        std::mem::replace(self, Err(JetAbsent))
    }
    fn replace(&mut self, value: T) -> Self {
        std::mem::replace(self, Ok(value))
    }
    fn filter(self, keep: impl FnOnce(&T) -> bool) -> Self {
        match self {
            Ok(value) if keep(&value) => Ok(value),
            _ => Err(JetAbsent),
        }
    }
    fn is_some_and(self, ask: impl FnOnce(T) -> bool) -> bool {
        match self {
            Ok(value) => ask(value),
            Err(JetAbsent) => false,
        }
    }
    fn or_err(self, why: String) -> JetOutcome<T, JetErr> {
        self.map_err(|JetAbsent| jet_err_from_message(why))
    }
    fn map_ref<U>(&self, f: impl FnOnce(&T) -> U) -> JetOutcome<U, JetAbsent> {
        match self {
            Ok(value) => Ok(f(value)),
            Err(JetAbsent) => Err(JetAbsent),
        }
    }
    fn zip<U>(self, other: JetOutcome<U, JetAbsent>) -> JetOutcome<(T, U), JetAbsent> {
        match (self, other) {
            (Ok(a), Ok(b)) => Ok((a, b)),
            _ => Err(JetAbsent),
        }
    }
}

/// Build a present payload. `T?` and `T ? E` build the same carrier.
pub fn jet_present<T, E>(value: T) -> JetOutcome<T, E> {
    Ok(value)
}

/// Build a clean absence.
pub fn jet_absent<T>() -> JetOutcome<T, JetAbsent> {
    Err(JetAbsent)
}

// D-FAIL-CARRIER1=A — the carrier's middle states.
//
// Success-with-notes and failure-with-partial-results are the same carrier
// seen at a different corner of its grid, not a third type. Both facts live on
// the outcome value itself: an error type opts into them by carrying them on
// its report, exactly as the ratified text says ("partials are opt-in per
// error type and notes erase when unread"). Nothing is stored beside the
// value, so two outcomes never share a fact, reading one twice answers the
// same both times, and an outcome that crosses a thread takes its facts with
// it. An error type that opts into neither pays for neither.
//
// Both readers take the projection onto the report and decide here — in one
// place — what a success answers and what a failure answers. Every engine
// calls these; none of them repeats the rule.

/// Read the part of the payload a failure kept.
///
/// A success held nothing back, so the carrier answers a clean absence and the
/// caller reads the whole thing as `T?`.
pub fn jet_partial<T, E, X: Clone>(
    outcome: &JetOutcome<T, E>,
    kept: impl FnOnce(&E) -> X,
) -> JetOutcome<X, JetAbsent> {
    match outcome {
        Ok(_) => Err(JetAbsent),
        Err(report) => Ok(kept(report)),
    }
}

/// Read the notes a failure's report collected on its way up.
///
/// A success has no report, so it collected nothing and the carrier answers an
/// empty list.
pub fn jet_notes<T, E, N>(outcome: &JetOutcome<T, E>, told: impl FnOnce(&E) -> Vec<N>) -> Vec<N> {
    match outcome {
        Ok(_) => Vec::new(),
        Err(report) => told(report),
    }
}

/// Marshal a Rust plumbing `Option` into the carrier at a Core boundary.
///
/// Rust's own collections answer with `Option`, the same way they hold their
/// elements in `Vec`. This is the one place that shape becomes a Jet outcome;
/// past it, `T?` is the carrier and nothing else.
pub fn jet_outcome_of<T>(value: Option<T>) -> JetOutcome<T, JetAbsent> {
    value.ok_or(JetAbsent)
}

/// D-REPORT-RUNTIME1=A: the dependency-free runtime projection of one
/// registered diagnostic. AOT, JIT, and the interpreter marshal into this
/// value; none of those engines owns report text or exit policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetRuntimeDiagnostic {
    pub code: &'static str,
    pub source: &'static str,
    pub what: String,
    pub why: String,
    pub fix: String,
    pub rendered: String,
    pub exit_code: i32,
}

/// Shared wording for a checked list position. Collection adapters marshal
/// the length and index here; they do not own the user-facing text.
pub fn jet_list_bounds_message(len: impl std::fmt::Display, index: i64) -> String {
    format!(
        "the list has {} items, so position {} doesn't exist",
        len, index
    )
}

/// Shared wording for a missing map key. `None` is used when the adapter cannot
/// marshal a displayable key value.
pub fn jet_missing_map_key_message(key: Option<&str>) -> String {
    key.map(|key| format!("the map has no entry for key {:?}", key))
        .unwrap_or_else(|| "the map has no entry for this key".to_string())
}

/// Shared missing-key wording for adapters that can display a key but do not
/// carry the full JetShow trait into their boundary.
pub fn jet_missing_map_key_value(key: impl std::fmt::Display) -> String {
    jet_missing_map_key_message(Some(&key.to_string()))
}

/// Shared wording for a reached typed hole.
pub fn jet_todo_message(file: &str, line: u32, expected_type: &str) -> String {
    format!("#Todo at {file}:{line} — expected {expected_type}")
}

/// Shared wording for the guarded recursion stop.
pub fn jet_stack_overflow_message(fn_name: &str) -> String {
    format!("stack overflow in `{fn_name}`")
}

/// One recursion budget for every execution tier. The evaluator, AOT Prelude,
/// and JIT host all stop at the same Jet-defined depth before a native stack
/// fault can escape as a process crash.
// The evaluator keeps a larger Rust frame than generated Jet code. Keep the
// shared limit below the smallest native worker stack so it can report the
// stop before the host stack aborts.
pub const JET_RUNTIME_STACK_LIMIT: usize = 6;

/// D-FAIL-BREACH1=A: the one renderer for a running program's breach stop.
///
/// The source location is data supplied by an execution tier. The code-to-copy
/// mapping, report shape, and breach exit code live here so AOT, JIT, and TIR
/// cannot drift into separate panic printers.
pub fn jet_render_runtime_stop(
    code: &'static str,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    message: &str,
    locals: &str,
) -> JetRuntimeDiagnostic {
    let (what, why, fix) = match code {
        "E3001" => (
            format!("panic: {message}"),
            "the program reached a panic stop and cannot continue".to_string(),
            "check the source location and handle the failing condition".to_string(),
        ),
        "E3005" => (
            message.to_string(),
            "a runtime contract condition evaluated false".to_string(),
            "satisfy the contract or update it".to_string(),
        ),
        "E3010" => (
            message.to_string(),
            "the operation has no valid result for these operands".to_string(),
            "check the operands before the operation, or use a checked operation".to_string(),
        ),
        "E3011" => (
            message.to_string(),
            "a #Todo hole was reached at runtime".to_string(),
            "implement this code before running it".to_string(),
        ),
        "E3012" => (
            message.to_string(),
            "the call stack exceeded Jet's safe runtime limit".to_string(),
            "end the recursion or make progress toward a base case".to_string(),
        ),
        _ => (
            message.to_string(),
            "the program reached a registered runtime stop".to_string(),
            "handle the condition at the reported source location".to_string(),
        ),
    };

    let mut rendered = format!("Stop [{code}]: {what}\n");
    if !file.is_empty() {
        rendered.push_str(&format!(
            "  --> {}:{}{}\n",
            file,
            line,
            if fn_name.is_empty() {
                String::new()
            } else {
                format!(" in {fn_name}")
            }
        ));
    }
    if !src_line.is_empty() {
        let line_s = line.to_string();
        let margin = line_s.len();
        let pad = " ".repeat(margin);
        rendered.push_str(&format!("   {pad}|\n"));
        rendered.push_str(&format!("{line_s} | {src_line}\n"));
        let col_offset = col.saturating_sub(1) as usize;
        let caret = "^".repeat(caret_len.max(1) as usize);
        rendered.push_str(&format!(
            "   {pad}| {}{}\n",
            " ".repeat(col_offset),
            caret
        ));
    }
    if !locals.is_empty() {
        rendered.push_str(&format!("locals: {locals}\n"));
    }
    rendered.push_str(&format!(" Why: {why}\n Fix: {fix}\n"));

    JetRuntimeDiagnostic {
        code,
        source: "runtime",
        what,
        why,
        fix,
        rendered,
        exit_code: 70,
    }
}

/// D-MEM-SENTRY1: shared wording for a runtime memory witness. The source
/// location and gate facts come from the engine; report copy and exit policy
/// stay in the Foundation Prelude for AOT, JIT, and TIR.
pub fn jet_render_runtime_sentry(
    code: &'static str,
    file: &str,
    line: u32,
    gate: &str,
    operation: &str,
    obligation: &str,
    detail: &str,
) -> JetRuntimeDiagnostic {
    let gate = if gate.is_empty() { "this unsafe gate" } else { gate };
    let (what, why, fix) = match code {
        "R0801" => (
            format!("raw {operation} outside `{gate}`'s storage"),
            format!("the pointer is outside allocation provenance tracked for `{gate}` ({detail})"),
            format!("bound the raw {operation} before it reaches storage — obligation `{obligation}` was not met on this run"),
        ),
        "R0802" => (
            format!("use of freed storage in `{gate}`"),
            format!("the allocation was quarantined and poisoned before this {operation} ({detail})"),
            format!("do not use the pointer after release — obligation `{obligation}` was not met on this run"),
        ),
        "R0803" => (
            format!("misaligned raw {operation} in `{gate}`"),
            format!("the pointer alignment does not satisfy the allocation provenance ({detail})"),
            format!("align the raw {operation} before access — obligation `{obligation}` was not met on this run"),
        ),
        _ => (
            format!("raw {operation} violated `{gate}`'s sentry"),
            detail.to_string(),
            format!("satisfy obligation `{obligation}` before the raw access"),
        ),
    };
    let mut rendered = format!("Runtime fault [{code}]: {what}\n");
    if !file.is_empty() {
        rendered.push_str(&format!("  --> {file}:{line}, in #Unsafe gate {file}:{line}\n"));
    }
    rendered.push_str(&format!(" Why: {why}\n Fix: {fix}\n"));
    JetRuntimeDiagnostic {
        code,
        source: "runtime",
        what,
        why,
        fix,
        rendered,
        exit_code: 70,
    }
}
