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

/// D-ERRCTX1=D over D-FAIL-ERROR1=A: add a human boundary without flattening
/// the original error. The message is evaluated by the caller only on failure.
pub fn jet_err_context(error: JetErr, message: String) -> JetErr {
    jet_err(message, Err(JetAbsent), Ok(error))
}

pub fn jet_context<T, F: FnOnce() -> String>(
    outcome: JetOutcome<T, JetErr>,
    message: F,
) -> JetOutcome<T, JetErr> {
    outcome.map_err(|error| jet_err_context(error, message()))
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

        let contextual = jet_context::<(), _>(Err(error), || "loading config".to_string())
            .expect_err("context preserves failure");
        assert_eq!(
            jet_render_err(&contextual),
            "Error: loading config\n  cause: parse failed\n    cause: unexpected token at line 3"
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

pub fn jet_render_registered_diagnostic(
    code: &'static str,
    source: &'static str,
    what: String,
    why: String,
    fix: String,
    exit_code: i32,
) -> JetRuntimeDiagnostic {
    JetRuntimeDiagnostic {
        code,
        source,
        rendered: format!("Error [{code}]: {what}\n Why: {why}\n Fix: {fix}\n"),
        what,
        why,
        fix,
        exit_code,
    }
}

/// E3001's registered runtime row. Keep this projection dependency-free: this
/// file is embedded verbatim in AOT programs and re-exported by JIT hosts.
const E3001_SOURCE: &str = "runtime";
const E3001_FIX: &str = "handle the CryptoError in fn run";

/// Marshal an unhandled `CryptoError` into the one E3001 runtime report.
pub fn jet_render_e3001_crypto(message: &str, internal: bool) -> JetRuntimeDiagnostic {
    jet_render_registered_diagnostic(
        "E3001",
        E3001_SOURCE,
        "unhandled cryptographic error".to_string(),
        message.to_string(),
        E3001_FIX.to_string(),
        if internal { 101 } else { 70 },
    )
}

/// The interpreter carries the stable redacted Display text rather than the
/// Rust enum variant. Preserve the same E3001 exit rule at that boundary.
pub fn jet_crypto_error_is_internal(message: &str) -> bool {
    message.starts_with("Jet could not preserve a cryptographic invariant; incident ")
}

/// A native runtime adapter's only termination door for a rendered report.
pub fn jet_abort_diagnostic(report: JetRuntimeDiagnostic) -> ! {
    eprint!("{}", report.rendered);
    std::process::exit(report.exit_code)
}
