// D-FAIL-CARRIER1=A / D-FAIL-MODEL1=A (ratified 2026-08-06) — the one outcome
// carrier under `?T` and `T !E`.
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
//   * `?T`     is `JetOutcome<T, JetAbsent>` — absence is clean, so the report
//              is `JetAbsent`, which carries nothing.
//   * `T !E`  is `JetOutcome<T, E>` — the report matters, so `E` is the report.
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
// Engines marshal the source fields. Construction, projection, and report
// rendering stay here so no engine owns error meaning.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErrorContextFrame {
    pub text: String,
    pub file: String,
    pub line: u32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErrorConversion {
    pub source: String,
    pub target: String,
}

/// Structured identity and source facts carried by a typed error while it is
/// projected through the default `JetErr` route. `value` is one canonical JSON
/// value, not rendered prose, so a report edge can inspect fields without
/// reconstructing them from `Display` output.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErrorField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErrorSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErrorDetails {
    pub variant: String,
    pub fields: Vec<JetErrorField>,
    pub source_span: Option<JetErrorSpan>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErr {
    message: String,
    code: JetOutcome<String, JetAbsent>,
    cause: JetOutcome<Box<JetErr>, JetAbsent>,
    /// D-FAIL-REPORT1: preserve typed identity independently of display text.
    typed_identity: Option<String>,
    /// D-FAIL-REPORT1: source-linked local context frames.
    context: Vec<JetErrorContextFrame>,
    /// D-FAIL-REPORT1: every declared conversion crossed on the way out.
    conversions: Vec<JetErrorConversion>,
    /// Typed source data that survives projection into the default carrier.
    details: Option<JetErrorDetails>,
}

/// One source-linked site in the report's structured journey: the origin, or
/// one hop the failure passed on its way out. This is the report view of the
/// private journey accumulator below; callers never need to recover source
/// facts from rendered text. `column` is 1-based and counts characters.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErrorJourneyFrame {
    pub fn_name: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub note: String,
    pub hops: u32,
}

/// D-FAIL-REPORT1: the complete default error report before terminal or wire
/// rendering. Keeping every part typed is what prevents a report edge from
/// parsing `Display` output to recover identity, causes, or source history.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErrorReport {
    pub code: Option<String>,
    pub message: String,
    pub typed_identity: Option<String>,
    pub causes: Vec<JetErrorReport>,
    pub context_frames: Vec<JetErrorContextFrame>,
    /// #3713: where the failure started — the `Err(...)` that a function
    /// returned, or the first call that failed on its way out.
    pub origin: Option<JetErrorJourneyFrame>,
    /// Every later site the failure passed through, origin side first.
    pub source_journey: Vec<JetErrorJourneyFrame>,
    pub conversion_history: Vec<JetErrorConversion>,
    pub details: Option<JetErrorDetails>,
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
        typed_identity: None,
        context: Vec::new(),
        conversions: Vec::new(),
        details: None,
    }
}

/// Construct the shared default error while retaining the source type's
/// identity. The identity is a fact, not a prefix parsed back out of the
/// rendered message.
pub fn jet_err_with_identity(
    message: String,
    code: JetOutcome<String, JetAbsent>,
    cause: JetOutcome<JetErr, JetAbsent>,
    typed_identity: String,
) -> JetErr {
    let mut error = jet_err(message, code, cause);
    error.typed_identity = Some(typed_identity);
    error
}

/// Construct the default report at a declared conversion boundary.  Keeping
/// this operation structured means the report never has to recover either the
/// source identity or the conversion history from display text.
pub fn jet_err_from_conversion(
    message: String,
    code: JetOutcome<String, JetAbsent>,
    cause: JetOutcome<JetErr, JetAbsent>,
    source: String,
    target: String,
) -> JetErr {
    jet_err_apply_conversion(jet_err(message, code, cause), source, target)
}

pub fn jet_err_typed_identity(error: &JetErr) -> Option<String> {
    error.typed_identity.clone()
}

pub fn jet_err_context(error: &JetErr) -> Vec<JetErrorContextFrame> {
    error.context.clone()
}

pub fn jet_err_conversions(error: &JetErr) -> Vec<JetErrorConversion> {
    error.conversions.clone()
}

pub fn jet_err_details(error: &JetErr) -> Option<JetErrorDetails> {
    error.details.clone()
}

pub fn jet_err_set_details(error: &mut JetErr, details: JetErrorDetails) {
    error.details = Some(details);
}

pub fn jet_err_add_context(error: &mut JetErr, text: String, file: String, line: u32) {
    error.context.push(JetErrorContextFrame { text, file, line });
}

pub fn jet_err_record_conversion(error: &mut JetErr, source: String, target: String) {
    error.conversions.push(JetErrorConversion { source, target });
}

/// Apply one declared conversion to an existing structured error. The
/// conversion boundary owns the source identity and history; the converted
/// error keeps its message, code, cause, context, and any earlier metadata.
pub fn jet_err_apply_conversion(mut error: JetErr, source: String, target: String) -> JetErr {
    if error.typed_identity.is_none() {
        error.typed_identity = Some(source.clone());
    }
    jet_err_record_conversion(&mut error, source, target);
    error
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
    jet_err_cause_projection(&error.cause)
}

/// Read a physical cause slot without exposing its recursive box in Jet.
pub fn jet_err_cause_projection(
    cause: &JetOutcome<Box<JetErr>, JetAbsent>,
) -> JetOutcome<JetErr, JetAbsent> {
    cause
        .as_ref()
        .map(|cause| (**cause).clone())
        .map_err(|_| JetAbsent)
}

// D-FAIL-CTX1: shared development journey state and rendering. Each `?`
// adapter claims its source site here, so AOT, JIT, and TIR-eval print one
// journey vocabulary and collapse only consecutive duplicate hops.
const JET_JOURNEY_DIAGNOSTIC_CODE: &str = "E3002";

/// One source site on the failure's way out: where it started, or a `?`
/// (written or automatic) it passed. `column` is 1-based, in characters.
struct JourneyFrame {
    fn_name: String,
    file: String,
    line: u32,
    column: u32,
}

impl JourneyFrame {
    /// The one site-equality rule behind hop collapse: same function, same
    /// file, same line. A fresh `?` and a hop carried in from another thread
    /// both ask this, so `×N` cannot mean two things.
    fn same_site(&self, file: &str, line: u32, fn_name: &str) -> bool {
        self.line == line && self.fn_name == fn_name && self.file == file
    }
}

/// One trail line: a `?` site, the note that hop carried, and how many
/// consecutive hops it stands for. A site that re-propagates keeps one line
/// and counts the repeats (`×N`) instead of printing the same line again — the
/// Go wrap-noise lesson — while each distinct site keeps its identity.
struct JourneyHop {
    site: JourneyFrame,
    note: String,
    hops: u32,
}

/// #3713: one failure's journey — where it started and every site it passed
/// since. The origin is claimed by the `Err(...)` a function returns
/// ([`jet_journey_origin`]); a failure that began inside a call (a failing
/// Core call, a conversion) has no such site, so the first hop it reaches
/// becomes its origin instead.
struct Journey {
    origin: Option<JourneyHop>,
    hops: Vec<JourneyHop>,
}

impl Journey {
    const fn new() -> Self {
        Self {
            origin: None,
            hops: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.origin.is_none() && self.hops.is_empty()
    }

    /// Record one hop. A journey with nothing yet starts here; otherwise the
    /// hop joins the trail, collapsing into the previous hop at the same site.
    /// The note is evaluated only when the hop opens a new line.
    fn push<F: FnOnce() -> String>(&mut self, site: JourneyFrame, hops: u32, note: F) {
        if self.is_empty() {
            self.origin = Some(JourneyHop {
                site,
                note: note(),
                hops,
            });
            return;
        }
        if let Some(last) = self.hops.last_mut() {
            if last.site.same_site(&site.file, site.line, &site.fn_name) {
                last.hops += hops;
                return;
            }
        }
        self.hops.push(JourneyHop {
            site,
            note: note(),
            hops,
        });
    }
}

/// A producer stop travels with its raw identity and its already-rendered frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetStreamFailure {
    pub code: String,
    pub message: String,
    pub rendered: String,
}

thread_local! {
    static JET_JOURNEY: std::cell::RefCell<Journey> = const {
        std::cell::RefCell::new(Journey::new())
    };
    static JET_STREAM_FAILURE_REPORT: std::cell::RefCell<Option<JetStreamFailure>> =
        const { std::cell::RefCell::new(None) };
}

pub fn jet_stream_record_failure_report(code: &str, message: &str, rendered: String) {
    JET_STREAM_FAILURE_REPORT.with(|slot| *slot.borrow_mut() = Some(JetStreamFailure {
        code: code.to_string(), message: message.to_string(), rendered,
    }));
}

pub fn jet_stream_take_failure_report() -> Option<JetStreamFailure> {
    JET_STREAM_FAILURE_REPORT.with(|slot| slot.borrow_mut().take())
}

pub fn jet_journey_reset() {
    JET_JOURNEY.with(|journey| {
        let mut journey = journey.borrow_mut();
        journey.origin = None;
        journey.hops.clear();
    });
}

/// Drain the accumulated journey as its rendered block (the `failed here`
/// line and E3002's trail), or an empty string when nothing was recorded.
///
/// Plain on purpose. The generated wasm store puts this string into JSON, where
/// ANSI and a column budget are both wrong. The terminal form of the same block
/// is [`jet_journey_report`].
pub fn jet_journey_take() -> String {
    jet_journey_take_styled(JetReportStyle::PLAIN)
}

fn jet_journey_take_styled(style: JetReportStyle) -> String {
    let journey = jet_journey_take_hops().0;
    let mut rendered = String::new();
    jet_write_journey_block(
        &mut rendered,
        journey.origin.as_ref().map(JetJourneySite::from),
        journey.hops.iter().map(JetJourneySite::from),
        style, false, 0,
    );
    rendered
}

/// The journey a run has accumulated so far, moved off this thread.
///
/// The journey belongs to the running program, but it lives in a thread-local
/// because each `task` owns its own. An engine that moves one program across
/// threads for its own reasons therefore has to move the journey with it: the
/// TIR evaluator runs every program on a 64 MiB worker thread and joins before
/// its report edge, so `?` pushed hops the worker owned and
/// `jet_journey_report` drained the caller's empty list — the evaluator
/// printed the failure with no trail while AOT and the resident tier printed
/// the hops (I9).
///
/// The journey never leaves this module. An engine can only move the opaque
/// carrier and hand it back to [`jet_journey_adopt`], so collapse, order, and
/// rendering stay owned here. The structured default-error edge uses
/// [`jet_error_report`] so the journey is projected beside the error facts;
/// string-only edges use this renderer directly.
pub struct JetJourneyHops(Journey);

/// Move this thread's accumulated journey out, for a caller that is about to
/// carry it across a thread boundary it created.
pub fn jet_journey_take_hops() -> JetJourneyHops {
    JET_JOURNEY.with(|journey| {
        JetJourneyHops(std::mem::replace(&mut *journey.borrow_mut(), Journey::new()))
    })
}

/// `jet run --json` (#3713): the report edge writes the `jet.err/v1` wire
/// instead of the terminal report. Process-wide because every tier's edge
/// renders through this module and the choice belongs to the invocation.
static JET_REPORT_WIRE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn jet_report_use_wire() {
    JET_REPORT_WIRE.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn jet_report_wire() -> bool {
    JET_REPORT_WIRE.load(std::sync::atomic::Ordering::Relaxed)
}

impl JetErrorDetails {
    pub fn to_json(&self) -> String {
        let fields = self
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{}:{}",
                    jet_error_json_quote(&field.name),
                    field.value
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let source_span = self.source_span.as_ref().map_or_else(
            || "null".to_string(),
            |span| format!("{{\"start\":{},\"end\":{}}}", span.start, span.end),
        );
        format!(
            "{{\"variant\":{},\"fields\":{{{fields}}},\"source_span\":{source_span}}}",
            jet_error_json_quote(&self.variant),
        )
    }
}

fn jet_error_json_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

const JET_ENTRY_ERROR_CODE: &str = "E3015";

struct JetErrorTerminalText<'a>(&'a str);

impl std::fmt::Display for JetErrorTerminalText<'_> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        crate::RuntimeDiagnosticCore::jet_write_terminal_text(out, self.0)
    }
}

impl JetErrorReport {
    fn from_error(
        error: &JetErr,
        origin: Option<JetErrorJourneyFrame>,
        source_journey: Vec<JetErrorJourneyFrame>,
    ) -> Self {
        let causes = match &error.cause {
            Ok(cause) => vec![Self::from_error(cause, None, Vec::new())],
            Err(_) => Vec::new(),
        };
        Self {
            code: match &error.code {
                Ok(code) => Some(code.clone()),
                Err(_) => None,
            },
            message: error.message.clone(),
            typed_identity: error.typed_identity.clone(),
            causes,
            context_frames: error.context.clone(),
            origin,
            source_journey,
            conversion_history: error.conversions.clone(),
            details: error.details.clone(),
        }
    }

    fn render_root(&self, style: JetReportStyle) -> String {
        use std::fmt::Write as _;

        fn indent(out: &mut String, depth: usize) {
            for _ in 0..depth {
                out.push_str("  ");
            }
        }

        fn render(report: &JetErrorReport, out: &mut String, depth: usize, style: JetReportStyle) {
            if depth == 0 {
                if let Some(code) = &report.code {
                    writeln!(out, "error code: {}", JetErrorTerminalText(code)).unwrap();
                }
                if let Some(identity) = &report.typed_identity {
                    writeln!(out, "type: {}", JetErrorTerminalText(identity)).unwrap();
                }
            } else {
                indent(out, depth);
                write!(out, "cause: {}", JetErrorTerminalText(&report.message)).unwrap();
                match (&report.typed_identity, &report.code) {
                    (Some(identity), Some(code)) => write!(out, " (type: {}; error code: {})", JetErrorTerminalText(identity), JetErrorTerminalText(code)).unwrap(),
                    (Some(identity), None) => write!(out, " (type: {})", JetErrorTerminalText(identity)).unwrap(),
                    (None, Some(code)) => write!(out, " (error code: {})", JetErrorTerminalText(code)).unwrap(),
                    (None, None) => {}
                }
                out.push('\n');
            }
            for cause in &report.causes {
                render(cause, out, depth + 1, style);
            }
            for frame in &report.context_frames {
                indent(out, depth + 1);
                writeln!(out, "context ({}:{}): {}", JetErrorTerminalText(&frame.file), frame.line, JetErrorTerminalText(&frame.text)).unwrap();
            }
            for conversion in &report.conversion_history {
                indent(out, depth + 1);
                writeln!(out, "conversion: {} -> {}", JetErrorTerminalText(&conversion.source), JetErrorTerminalText(&conversion.target)).unwrap();
            }
            if let Some(details) = &report.details {
                indent(out, depth + 1);
                writeln!(out, "variant: {}", JetErrorTerminalText(&details.variant)).unwrap();
                for field in &details.fields {
                    indent(out, depth + 1);
                    writeln!(out, "{}: {}", JetErrorTerminalText(&field.name), JetErrorTerminalText(&field.value)).unwrap();
                }
                if let Some(span) = &details.source_span {
                    indent(out, depth + 1);
                    writeln!(out, "source span: {}..{}", span.start, span.end).unwrap();
                }
            }
            if depth != 0 {
                report.write_journey_with_style(out, style, depth);
            }
        }

        let row = jet_runtime_diagnostic_row(JET_ENTRY_ERROR_CODE)
            .expect("registered entry error row is missing");
        let mut out = String::new();
        write!(out, "Error [{}]: ", row.code).unwrap();
        for (prefix, template) in [
            ("", row.what),
            (" Why: ", row.why),
            (" Fix: ", row.fix),
        ] {
            out.push_str(prefix);
            jet_write_diagnostic_template(&mut out, template, |out, name| {
                assert_eq!(name, "msg", "entry error row has an unsupported template hole");
                crate::RuntimeDiagnosticCore::jet_write_terminal_text(out, &self.message)
            }).expect("String formatting cannot fail");
            out.push('\n');
        }
        render(self, &mut out, 0, style);
        out
    }

    /// Render only the structured source journey — the `failed here` line
    /// and the trail under it. The report edge uses this beside the root
    /// report for terminal and wire adapters.
    pub fn render_journey_with_style(&self, style: JetReportStyle) -> String {
        let mut rendered = String::new();
        self.write_journey_with_style(&mut rendered, style, 0);
        rendered
    }

    fn write_journey_with_style(&self, out: &mut String, style: JetReportStyle, depth: usize) {
        jet_write_journey_block(
            out,
            self.origin.as_ref().map(JetJourneySite::from),
            self.source_journey.iter().map(JetJourneySite::from),
            style, true, depth,
        );
    }

    /// Render this already-projected report with the one terminal policy.
    /// The projection and the rendering are separate so a wire or inspector
    /// can consume the same facts without first making a display string.
    pub fn render_with_style(&self, style: JetReportStyle) -> String {
        let mut rendered = self.render_root(style);
        self.write_journey_with_style(&mut rendered, style, 0);
        jet_write_diagnostic_more_line(&mut rendered, JET_ENTRY_ERROR_CODE)
            .expect("String formatting cannot fail");
        rendered.push('\n');
        rendered
    }

    /// The stderr report edge: the terminal report, or under `jet run --json`
    /// the `jet.err/v1` wire on one line.
    pub fn render(&self) -> String {
        if jet_report_wire() {
            return format!("{}\n", self.to_json());
        }
        self.render_with_style(JetReportStyle::for_stderr())
    }

    /// Serialize the same report facts for a boundary wire. Optional fields
    /// stay absent for the old empty shape, but when present they are copied
    /// from the report object rather than parsed from its rendered text.
    pub fn to_json(&self) -> String {
        fn quote(value: &str) -> String {
            let mut out = String::with_capacity(value.len() + 2);
            out.push('"');
            for ch in value.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
                    ch => out.push(ch),
                }
            }
            out.push('"');
            out
        }

        let mut fields = vec![
            "\"schema\":\"jet.err/v1\"".to_string(),
            format!("\"message\":{}", quote(&self.message)),
            format!(
                "\"code\":{}",
                self.code.as_deref().map_or_else(|| "null".to_string(), quote)
            ),
            format!(
                "\"cause\":{}",
                match self.causes.as_slice() {
                    [] => "null".to_string(),
                    [cause] => cause.to_json(),
                    causes => format!(
                        "[{}]",
                        causes.iter().map(Self::to_json).collect::<Vec<_>>().join(",")
                    ),
                }
            ),
        ];
        if let Some(identity) = &self.typed_identity {
            fields.push(format!("\"typed_identity\":{}", quote(identity)));
        }
        if !self.context_frames.is_empty() {
            fields.push(format!(
                "\"context_frames\":[{}]",
                self.context_frames
                    .iter()
                    .map(|frame| {
                        format!(
                            "{{\"text\":{},\"file\":{},\"line\":{}}}",
                            quote(&frame.text),
                            quote(&frame.file),
                            frame.line
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        let journey_frame = |frame: &JetErrorJourneyFrame| {
            format!(
                "{{\"fn_name\":{},\"file\":{},\"line\":{},\"column\":{},\"note\":{},\"hops\":{}}}",
                quote(&frame.fn_name),
                quote(&frame.file),
                frame.line,
                frame.column,
                quote(&frame.note),
                frame.hops
            )
        };
        if let Some(origin) = &self.origin {
            fields.push(format!("\"origin\":{}", journey_frame(origin)));
        }
        if !self.source_journey.is_empty() {
            fields.push(format!(
                "\"source_journey\":[{}]",
                self.source_journey
                    .iter()
                    .map(journey_frame)
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !self.conversion_history.is_empty() {
            fields.push(format!(
                "\"conversion_history\":[{}]",
                self.conversion_history
                    .iter()
                    .map(|conversion| {
                        format!(
                            "{{\"source\":{},\"target\":{}}}",
                            quote(&conversion.source),
                            quote(&conversion.target)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if let Some(details) = &self.details {
            fields.push(format!("\"details\":{}", details.to_json()));
        }
        format!("{{{}}}", fields.join(","))
    }

    /// Decode the canonical report wire emitted by [`Self::to_json`].
    ///
    /// Build cache and remote execution records carry this object without
    /// reducing it to display text.  Requiring the canonical spelling keeps a
    /// replayed report byte-for-byte equivalent to the report that crossed
    /// the original action boundary.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value = crate::EncodingJson::parse_json(text, true)
            .map_err(|error| format!("invalid error report: {}", error.message))?;
        let report = jet_error_report_from_json(&value)?;
        if report.to_json() != text {
            return Err("error report is not canonical".to_string());
        }
        Ok(report)
    }
}

fn jet_error_report_from_json(
    value: &crate::Dyn::Dyn,
) -> Result<JetErrorReport, String> {
    let fields = jet_error_json_object(value, "report")?;
    if jet_error_json_text(jet_error_json_field(fields, "schema")?, "schema")?
        != "jet.err/v1"
    {
        return Err("unsupported error report schema".to_string());
    }
    let message = jet_error_json_text(jet_error_json_field(fields, "message")?, "message")?;
    let code = match jet_error_json_field(fields, "code")? {
        crate::Dyn::Dyn::Null => None,
        value => Some(jet_error_json_text(value, "code")?),
    };
    let causes = match jet_error_json_field(fields, "cause")? {
        crate::Dyn::Dyn::Null => Vec::new(),
        crate::Dyn::Dyn::Object(_) => {
            vec![jet_error_report_from_json(jet_error_json_field(fields, "cause")?)?]
        }
        crate::Dyn::Dyn::Array(values) => values
            .iter()
            .map(jet_error_report_from_json)
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("error report cause is not null, an object, or an array".to_string()),
    };
    let typed_identity = jet_error_json_optional_text(fields, "typed_identity")?;
    let context_frames = jet_error_json_optional_array(fields, "context_frames")?
        .map(|values| {
            values
                .iter()
                .map(jet_error_context_from_json)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let origin = jet_error_json_field_optional(fields, "origin")
        .map(jet_error_journey_from_json)
        .transpose()?;
    let source_journey = jet_error_json_optional_array(fields, "source_journey")?
        .map(|values| {
            values
                .iter()
                .map(jet_error_journey_from_json)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let conversion_history = jet_error_json_optional_array(fields, "conversion_history")?
        .map(|values| {
            values
                .iter()
                .map(jet_error_conversion_from_json)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let details = jet_error_json_field_optional(fields, "details")
        .map(jet_error_details_from_json)
        .transpose()?;
    Ok(JetErrorReport {
        code,
        message,
        typed_identity,
        causes,
        context_frames,
        origin,
        source_journey,
        conversion_history,
        details,
    })
}

fn jet_error_json_object<'a>(
    value: &'a crate::Dyn::Dyn,
    name: &str,
) -> Result<&'a [(String, crate::Dyn::Dyn)], String> {
    match value {
        crate::Dyn::Dyn::Object(fields) => Ok(fields),
        _ => Err(format!("error report {name} is not an object")),
    }
}

fn jet_error_json_field<'a>(
    fields: &'a [(String, crate::Dyn::Dyn)],
    name: &str,
) -> Result<&'a crate::Dyn::Dyn, String> {
    jet_error_json_field_optional(fields, name).ok_or_else(|| format!("error report lacks {name}"))
}

fn jet_error_json_field_optional<'a>(
    fields: &'a [(String, crate::Dyn::Dyn)],
    name: &str,
) -> Option<&'a crate::Dyn::Dyn> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

fn jet_error_json_text(
    value: &crate::Dyn::Dyn,
    name: &str,
) -> Result<String, String> {
    match value {
        crate::Dyn::Dyn::Text(value) => Ok(value.clone()),
        _ => Err(format!("error report {name} is not text")),
    }
}

fn jet_error_json_optional_text(
    fields: &[(String, crate::Dyn::Dyn)],
    name: &str,
) -> Result<Option<String>, String> {
    jet_error_json_field_optional(fields, name)
        .map(|value| jet_error_json_text(value, name).map(Some))
        .unwrap_or(Ok(None))
}

fn jet_error_json_optional_array<'a>(
    fields: &'a [(String, crate::Dyn::Dyn)],
    name: &str,
) -> Result<Option<&'a [crate::Dyn::Dyn]>, String> {
    let Some(value) = jet_error_json_field_optional(fields, name) else {
        return Ok(None);
    };
    match value {
        crate::Dyn::Dyn::Array(values) => Ok(Some(values)),
        _ => Err(format!("error report {name} is not an array")),
    }
}

fn jet_error_json_u32(
    value: &crate::Dyn::Dyn,
    name: &str,
) -> Result<u32, String> {
    match value {
        crate::Dyn::Dyn::Int(value) => value.to_i64().and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| format!("error report {name} is not a u32")),
        _ => Err(format!("error report {name} is not an integer")),
    }
}

fn jet_error_context_from_json(
    value: &crate::Dyn::Dyn,
) -> Result<JetErrorContextFrame, String> {
    let fields = jet_error_json_object(value, "context frame")?;
    Ok(JetErrorContextFrame {
        text: jet_error_json_text(jet_error_json_field(fields, "text")?, "context text")?,
        file: jet_error_json_text(jet_error_json_field(fields, "file")?, "context file")?,
        line: jet_error_json_u32(jet_error_json_field(fields, "line")?, "context line")?,
    })
}

fn jet_error_journey_from_json(
    value: &crate::Dyn::Dyn,
) -> Result<JetErrorJourneyFrame, String> {
    let fields = jet_error_json_object(value, "journey frame")?;
    Ok(JetErrorJourneyFrame {
        fn_name: jet_error_json_text(jet_error_json_field(fields, "fn_name")?, "journey function")?,
        file: jet_error_json_text(jet_error_json_field(fields, "file")?, "journey file")?,
        line: jet_error_json_u32(jet_error_json_field(fields, "line")?, "journey line")?,
        column: jet_error_json_u32(jet_error_json_field(fields, "column")?, "journey column")?,
        note: jet_error_json_text(jet_error_json_field(fields, "note")?, "journey note")?,
        hops: jet_error_json_u32(jet_error_json_field(fields, "hops")?, "journey hops")?,
    })
}

fn jet_error_conversion_from_json(
    value: &crate::Dyn::Dyn,
) -> Result<JetErrorConversion, String> {
    let fields = jet_error_json_object(value, "conversion")?;
    Ok(JetErrorConversion {
        source: jet_error_json_text(jet_error_json_field(fields, "source")?, "conversion source")?,
        target: jet_error_json_text(jet_error_json_field(fields, "target")?, "conversion target")?,
    })
}

fn jet_error_details_from_json(
    value: &crate::Dyn::Dyn,
) -> Result<JetErrorDetails, String> {
    let fields = jet_error_json_object(value, "details")?;
    let field_values = jet_error_json_object(jet_error_json_field(fields, "fields")?, "detail fields")?
        .iter()
        .map(|(name, value)| {
            Ok(JetErrorField {
                name: name.clone(),
                value: jet_error_json_encode_value(value),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let source_span = match jet_error_json_field(fields, "source_span")? {
        crate::Dyn::Dyn::Null => None,
        value => {
            let span = jet_error_json_object(value, "source span")?;
            Some(JetErrorSpan {
                start: jet_error_json_u32(jet_error_json_field(span, "start")?, "span start")?
                    as usize,
                end: jet_error_json_u32(jet_error_json_field(span, "end")?, "span end")?
                    as usize,
            })
        }
    };
    Ok(JetErrorDetails {
        variant: jet_error_json_text(jet_error_json_field(fields, "variant")?, "detail variant")?,
        fields: field_values,
        source_span,
    })
}

fn jet_error_json_encode_value(value: &crate::Dyn::Dyn) -> String {
    match value {
        crate::Dyn::Dyn::Null => "null".to_string(),
        crate::Dyn::Dyn::Bool(value) => value.to_string(),
        crate::Dyn::Dyn::Int(value) => value.to_string(),
        crate::Dyn::Dyn::Float(value) => value.to_string(),
        crate::Dyn::Dyn::Number(value) => value.clone(),
        crate::Dyn::Dyn::Text(value) => {
            format!("\"{}\"", jet_error_json_escape(value))
        }
        crate::Dyn::Dyn::Bytes(bytes) => format!("\"{}\"", crate::Dyn::Dyn::bytes_text(bytes)),
        crate::Dyn::Dyn::DateTime(value) => format!("\"{}\"", value.to_rfc3339()),
        crate::Dyn::Dyn::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(jet_error_json_encode_value)
                .collect::<Vec<_>>()
                .join(",")
        ),
        crate::Dyn::Dyn::Object(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(name, value)| {
                    format!(
                        "\"{}\":{}",
                        jet_error_json_escape(name),
                        jet_error_json_encode_value(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn jet_error_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn jet_journey_frame_from_hop(hop: JourneyHop) -> JetErrorJourneyFrame {
    JetErrorJourneyFrame {
        fn_name: hop.site.fn_name,
        file: hop.site.file,
        line: hop.site.line,
        column: hop.site.column,
        note: hop.note,
        hops: hop.hops,
    }
}


/// Project one escaping default error and the journey it accumulated into the
/// report consumed by every report edge. This drains the journey exactly once
/// and never asks `Display` to reconstruct any field.
pub fn jet_error_report(error: &JetErr) -> JetErrorReport {
    let journey = jet_journey_take_hops().0;
    JetErrorReport::from_error(
        error,
        journey.origin.map(jet_journey_frame_from_hop),
        journey.hops.into_iter().map(jet_journey_frame_from_hop).collect(),
    )
}

/// Adopt a journey carried in from another thread. A carried origin means the
/// failure started over there, so it replaces whatever this thread held; its
/// hops then join oldest first under the same consecutive-site collapse a
/// fresh `?` applies — a site that re-propagates across the seam adds its
/// count to one line instead of opening a second.
pub fn jet_journey_adopt(carried: JetJourneyHops) {
    let carried = carried.0;
    JET_JOURNEY.with(|journey| {
        let mut journey = journey.borrow_mut();
        if carried.origin.is_some() {
            journey.origin = carried.origin;
            journey.hops.clear();
        }
        for hop in carried.hops {
            let JourneyHop { site, note, hops } = hop;
            journey.push(site, hops, || note);
        }
    });
}

/// The terminal facts the trail is laid out against.
///
/// Facts in, policy here. [`jet_journey_report`] resolves this ONCE at the
/// report edge, and every tier reaches that edge through that one call — AOT's
/// `jet_entry_report`, the resident Cranelift boundary, the interpreter
/// boundary, the deopt boundary — so no engine owns a terminal decision (I9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetReportStyle {
    /// Dim the trail, so the root failure is the one undimmed line.
    pub color: bool,
    /// Column budget, or `None` for a stream that has no columns. No budget
    /// means no elision: a pipe, a log and a JSON wire keep whole paths.
    pub width: Option<usize>,
}

impl JetReportStyle {
    /// Every consumer that is not a terminal: full bytes, no ANSI.
    pub const PLAIN: Self = Self {
        color: false,
        width: None,
    };

    /// The one detection.
    ///
    /// Colour and width are separate capabilities: `FORCE_COLOR` paints a pipe
    /// but never gives it columns. A terminal's width is `COLUMNS` when that is
    /// a positive integer, else the ratified 80-column default. This edge does
    /// not shell out to `stty` the way `io.terminal_width()` may — the program
    /// is already failing, and spawning a child to lay out its last line is the
    /// worse trade.
    pub fn for_stderr() -> Self {
        let is_tty = {
            use std::io::IsTerminal;
            std::io::stderr().is_terminal()
        };
        Self {
            color: jet_terminal_auto_color(is_tty),
            width: if is_tty {
                Some(jet_report_env_columns().unwrap_or(JET_REPORT_DEFAULT_COLUMNS))
            } else {
                None
            },
        }
    }
}

/// Card #1751 named the 80x24 terminal default once, in
/// `jet-codegen/src/Prelude/TerminalDefault.rs`. This file is emitted verbatim
/// into every generated program (`Codegen/mod.rs`'s `PRELUDE_PARTS`) while that
/// one is a `jet-codegen` module, so the column half is spelled here too.
const JET_REPORT_DEFAULT_COLUMNS: usize = 80;

/// Dim — the same SGR pair as `Terminal.rs`'s `Theme::DIM_SGR`, spelled here
/// for the same reason as the default above.
const JET_REPORT_DIM_SGR: &str = "\x1b[2;37m";
const JET_REPORT_SGR_RESET: &str = "\x1b[0m";

/// `NO_COLOR` presence > `FORCE_COLOR` presence > the stream — THE `auto`
/// colour ladder. `Terminal.rs`'s `ColorChoice::Auto` calls this, so a compile
/// diagnostic and a running program's breach report cannot answer differently.
/// It lives in this file because this is the file a generated program gets.
pub fn jet_terminal_auto_color(is_tty: bool) -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("FORCE_COLOR").is_some() {
        return true;
    }
    is_tty
}

/// `COLUMNS`, under the same positive-integer rule `Prelude/Term.rs` applies to
/// that variable for `io.terminal_width()`.
fn jet_report_env_columns() -> Option<usize> {
    std::env::var("COLUMNS")
        .ok()?
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|columns| *columns > 0)
}

/// Columns, not bytes: `—` and `×` are one column and several bytes each.
fn jet_report_columns(text: &str) -> usize {
    text.chars().count()
}

// A borrowed rendering view, not another journey owner or wire carrier.
#[derive(Clone, Copy)]
struct JetJourneySite<'a> {
    function: &'a str,
    file: &'a str,
    line: u32,
    column: u32,
    note: &'a str,
    hops: u32,
}

impl<'a> From<&'a JourneyHop> for JetJourneySite<'a> {
    fn from(hop: &'a JourneyHop) -> Self {
        Self {
            function: &hop.site.fn_name, file: &hop.site.file,
            line: hop.site.line, column: hop.site.column,
            note: &hop.note, hops: hop.hops,
        }
    }
}

impl<'a> From<&'a JetErrorJourneyFrame> for JetJourneySite<'a> {
    fn from(frame: &'a JetErrorJourneyFrame) -> Self {
        Self {
            function: &frame.fn_name, file: &frame.file,
            line: frame.line, column: frame.column,
            note: &frame.note, hops: frame.hops,
        }
    }
}

#[derive(Clone, Copy)]
enum JetJourneyLabel {
    Origin,
    Hop(usize),
}

impl std::fmt::Display for JetJourneyLabel {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Origin => out.write_str("failed here:"),
            Self::Hop(number) => write!(out, "{number}."),
        }
    }
}

/// The journey block under the root failure: where the failure started, then
/// E3002's trail of every site it passed. The root failure has already been
/// stated by the time this prints, so a line only has to say where: `failed
/// here:` names the origin, the numbered trail reads origin side first, one
/// line per site, `×N` when a site repeated, and the hop's note after an em
/// dash. The header carries the code and the mechanism once instead of
/// repeating `error propagated from … via ?` on every line, which is what
/// buried the root cause under its own trail.
///
/// `style` is the whole terminal-state matrix. Colour dims this block and only
/// this block, so the root failure above stays the one undimmed line and stays
/// what the eye lands on. A column budget sheds this block's disposable parts
/// and never reaches the root failure, which is not this function's line.
fn jet_write_journey_block<'a>(
    out: &mut String,
    origin: Option<JetJourneySite<'a>>,
    hops: impl Iterator<Item = JetJourneySite<'a>> + Clone,
    style: JetReportStyle,
    error_report: bool,
    depth: usize,
) {
    let sites = origin.into_iter().map(|site| (JetJourneyLabel::Origin, site))
        .chain(hops.clone().enumerate().map(|(index, site)| (JetJourneyLabel::Hop(index + 1), site)));
    // One path budget for the whole block, decided by its widest site line.
    let layout = style.width
        .map(|width| (width, jet_journey_path_budget(sites.clone(), width, error_report)));
    for (label, site) in sites {
        if matches!(label, JetJourneyLabel::Hop(1)) {
            let total = hops.clone().map(|hop| hop.hops).sum();
            jet_write_journey_line(out, style, depth, |out| {
                jet_write_journey_header(out, total, style.width)
            });
        }
        jet_write_journey_line(out, style, depth, |out| {
            match layout {
                Some(layout) => jet_write_journey_hop_line(out, label, site, layout, error_report),
                None => jet_write_journey_hop_text(out, label, site, (site.file, false), site.note, false, error_report),
            }
        });
    }
}

fn jet_write_journey_line(
    out: &mut String,
    style: JetReportStyle,
    depth: usize,
    write_line: impl FnOnce(&mut dyn std::fmt::Write) -> std::fmt::Result,
) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    // One SGR pair per line preserves resets in line-oriented filters.
    if style.color {
        out.push_str(JET_REPORT_DIM_SGR);
    }
    write_line(out).expect("String formatting cannot fail");
    if style.color {
        out.push_str(JET_REPORT_SGR_RESET);
    }
    out.push('\n');
}

/// The header separates the root failure from its trail, so it must not wrap.
/// When the full form does not fit, the reminder of the mechanism goes and the
/// facts stay: the code and the hop count. That short form is the floor.
fn jet_write_journey_header(
    out: &mut dyn std::fmt::Write,
    total: u32,
    width: Option<usize>,
) -> std::fmt::Result {
    use std::fmt::Write as _;
    let plural = if total == 1 { "" } else { "s" };
    let code = JET_JOURNEY_DIAGNOSTIC_CODE;
    let short = width.is_some_and(|width| {
        let mut count = JetJourneyColumnCount(0);
        write!(&mut count, " Trail [{code}] ({total} hop{plural} via ?, origin first):")
            .expect("Column counting cannot fail");
        count.0 > width
    });
    if short {
        write!(out, " Trail [{code}] ({total} hop{plural}):")
    } else {
        write!(out, " Trail [{code}] ({total} hop{plural} via ?, origin first):")
    }
}

/// The columns left for a site's file path once the widest site line's fixed
/// parts are paid for.
fn jet_journey_path_budget<'a>(
    sites: impl Iterator<Item = (JetJourneyLabel, JetJourneySite<'a>)>,
    width: usize,
    error_report: bool,
) -> usize {
    let widest = sites.map(|(label, site)| {
        jet_journey_text_columns(label, site, ("", false), site.note, error_report)
    }).max().unwrap_or(0);
    width.saturating_sub(widest)
}

/// One site line, laid out to the block's width.
///
/// A site's identity is its label, its `fn` and `file:line` — that is the
/// address of the code to open, and nothing sheds it. What sheds, in order:
/// leading path segments, then the note's tail. So a narrow terminal loses
/// commentary, never a location, and never the root failure above.
fn jet_write_journey_hop_line(
    out: &mut dyn std::fmt::Write,
    label: JetJourneyLabel,
    site: JetJourneySite<'_>,
    (width, path_budget): (usize, usize),
    error_report: bool,
) -> std::fmt::Result {
    let file = jet_report_elide_path(site.file, path_budget);
    let (note, ellipsis) = if jet_journey_text_columns(label, site, file, site.note, error_report) <= width {
        (site.note, false)
    } else {
        let fixed = jet_journey_text_columns(label, site, file, "", error_report);
        // The em dash and ellipsis cost four columns; omit a note that cannot
        // show one character instead of leaving only ` — …`.
        let room = width.saturating_sub(fixed + 4);
        if room == 0 {
            ("", false)
        } else {
            let end = site.note.char_indices().nth(room).map_or(site.note.len(), |(index, _)| index);
            (&site.note[..end], true)
        }
    };
    jet_write_journey_hop_text(out, label, site, file, note, ellipsis, error_report)
}

fn jet_write_journey_hop_text(
    out: &mut dyn std::fmt::Write,
    label: JetJourneyLabel,
    site: JetJourneySite<'_>,
    file: (&str, bool),
    note: &str,
    ellipsis: bool,
    error_report: bool,
) -> std::fmt::Result {
    write!(out, "  {label} ")?;
    jet_write_journey_source_text(out, site.function, error_report)?;
    out.write_str(" (")?;
    if file.1 {
        jet_write_journey_source_text(out, "…/", error_report)?;
    }
    jet_write_journey_source_text(out, file.0, error_report)?;
    write!(out, ":{}", site.line)?;
    if error_report && site.column != 0 {
        write!(out, ":{}", site.column)?;
    }
    out.write_char(')')?;
    if site.hops > 1 {
        write!(out, " ×{}", site.hops)?;
    }
    if !note.is_empty() || ellipsis {
        out.write_str(" — ")?;
        jet_write_journey_source_text(out, note, error_report)?;
        if ellipsis {
            out.write_char('…')?;
        }
    }
    Ok(())
}

fn jet_write_journey_source_text(
    out: &mut dyn std::fmt::Write,
    text: &str,
    error_report: bool,
) -> std::fmt::Result {
    if error_report {
        crate::RuntimeDiagnosticCore::jet_write_terminal_text(out, text)
    } else {
        // Standalone journey transport keeps its existing ordinary text policy.
        out.write_str(text)
    }
}

struct JetJourneyColumnCount(usize);

impl std::fmt::Write for JetJourneyColumnCount {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0 += jet_report_columns(text);
        Ok(())
    }
}

fn jet_journey_text_columns(
    label: JetJourneyLabel,
    site: JetJourneySite<'_>,
    file: (&str, bool),
    note: &str,
    error_report: bool,
) -> usize {
    let mut count = JetJourneyColumnCount(0);
    jet_write_journey_hop_text(&mut count, label, site, file, note, false, error_report)
        .expect("Column counting cannot fail");
    count.0
}

/// Shed whole leading segments, never characters inside a name, so the file you
/// have to open is still spelled correctly. `…/` marks what went. The basename
/// is the floor: a half-spelled file name is worse than a line that wraps.
fn jet_report_elide_path(file: &str, budget: usize) -> (&str, bool) {
    let mut columns = jet_report_columns(file);
    if columns <= budget {
        return (file, false);
    }
    let mut start = 0;
    while let Some(slash) = file[start..].find('/') {
        let next = start + slash + 1;
        columns -= jet_report_columns(&file[start..next]);
        start = next;
        if columns + 2 <= budget {
            return (&file[start..], true);
        }
    }
    (&file[start..], false)
}

/// D-FAIL-BREACH1=A / D-FAIL-CTX1: the one order for a failure report — the
/// root failure leads, its trail follows.
///
/// Every adapter that holds both halves calls this instead of concatenating its
/// own way. The wasm store built `journey + report` itself, so one tier could
/// print the trail first while the natives printed it last (I9).
pub fn jet_journey_compose(error: &str, trail: &str) -> String {
    let error = error.trim_end_matches('\n');
    let mut report = String::with_capacity(error.len() + trail.len() + 1);
    report.push_str(error);
    report.push('\n');
    report.push_str(trail);
    report
}

/// The one report-edge policy for a failure that escapes the entry (D-FAIL-EDGE1).
///
/// A journey hop is ACCUMULATED at each `?` and printed only here, so a
/// failure recovered by `??` or a `.Err(e)` arm reports nothing. Every engine
/// marshals to this symbol: `jet_entry_report` in the AOT Prelude, the resident
/// Cranelift entry boundary, the interpreter boundary, and the deopt boundary.
/// Each of those printed frames eagerly at the `?` site before this existed,
/// which made a handled error print a report on stderr under those tiers and
/// nothing under AOT (I9).
pub fn jet_journey_report(error: &str) -> String {
    if jet_report_wire() {
        // A typed entry error reaches this edge as its display text; the wire
        // keeps that text as the message and the journey as structured facts.
        let journey = jet_journey_take_hops().0;
        let report = JetErrorReport {
            code: None,
            message: error.trim_end_matches('\n').to_string(),
            typed_identity: None,
            causes: Vec::new(),
            context_frames: Vec::new(),
            origin: journey.origin.map(jet_journey_frame_from_hop),
            source_journey: journey.hops.into_iter().map(jet_journey_frame_from_hop).collect(),
            conversion_history: Vec::new(),
            details: None,
        };
        return format!("{}\n", report.to_json());
    }
    jet_journey_report_styled(error, JetReportStyle::for_stderr())
}

/// The same report against explicit terminal facts, which is how the terminal
/// state matrix is proven. Nothing else may render a second trail.
pub fn jet_journey_report_styled(error: &str, style: JetReportStyle) -> String {
    jet_journey_compose(error, &jet_journey_take_styled(style))
}

/// #3713: claim the site where a failure starts — the `Err(...)` a function
/// returns. A new failure begins here, so any journey this thread still held
/// from an earlier, handled failure is dropped.
pub fn jet_journey_origin(file: &str, line: u32, column: u32, fn_name: &str) {
    JET_JOURNEY.with(|journey| {
        let mut journey = journey.borrow_mut();
        journey.hops.clear();
        journey.origin = Some(JourneyHop {
            site: JourneyFrame {
                fn_name: fn_name.to_string(),
                file: file.to_string(),
                line,
                column,
            },
            note: String::new(),
            hops: 1,
        });
    });
}

/// Claim one `?` site (written or automatic) for the failure now on its way
/// out. Nothing prints here: the hop reaches stderr only at the report edge,
/// and only if the failure escapes the entry. A failure that has no origin yet
/// began inside this call, so this site becomes its origin.
pub fn jet_journey_frame<F: FnOnce() -> String>(
    file: &str,
    line: u32,
    column: u32,
    fn_name: &str,
    note: F,
) {
    JET_JOURNEY.with(|journey| {
        journey.borrow_mut().push(
            JourneyFrame {
                fn_name: fn_name.to_string(),
                file: file.to_string(),
                line,
                column,
            },
            1,
            note,
        );
    });
}

/// Scalar MIR adapter for a checked failure-site note.
pub fn jet_journey_frame_text(file: &str, line: u32, column: u32, fn_name: &str, note: &str) {
    jet_journey_frame(file, line, column, fn_name, || note.to_string());
}

/// Add one evaluated note to the default error and its source journey.
pub fn jet_err_with_context_frame(
    mut error: JetErr,
    file: &str,
    line: u32,
    column: u32,
    fn_name: &str,
    note: String,
) -> JetErr {
    jet_err_add_context(&mut error, note.clone(), file.to_string(), line);
    jet_journey_frame(file, line, column, fn_name, || note);
    error
}
#[cfg(target_arch = "wasm32")]
mod jet_error_wasm_bridge {
    use super::*;
    use std::cell::RefCell;

    fn jet_journey_hop_from_frame(frame: &JetErrorJourneyFrame) -> JourneyHop {
        JourneyHop {
            site: JourneyFrame {
                fn_name: frame.fn_name.clone(),
                file: frame.file.clone(),
                line: frame.line,
                column: frame.column,
            },
            note: frame.note.clone(),
            hops: frame.hops,
        }
    }

    // The Web tier owns only these byte slots. All error facts, journey
    // accumulation, conversion history, and report rendering stay in the
    // Foundation Prelude.
    const INPUT_SLOT_COUNT: usize = 8;
    const MESSAGE_SLOT: u32 = 0;
    const ERROR_SLOT: u32 = 1;
    const FILE_SLOT: u32 = 2;
    const FUNCTION_SLOT: u32 = 3;
    const NOTE_SLOT: u32 = 4;
    const ORIGINAL_SLOT: u32 = 5;
    const SOURCE_SLOT: u32 = 6;
    const TARGET_SLOT: u32 = 7;
    const OK: i32 = 1;
    const BRIDGE_ERROR: i32 = -1;

    thread_local! {
        static INPUT: RefCell<[Vec<u8>; INPUT_SLOT_COUNT]> =
            RefCell::new(std::array::from_fn(|_| Vec::new()));
        static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    fn clear_output() {
        OUTPUT.with(|cell| {
            let mut output = cell.borrow_mut();
            output.clear();
            output.shrink_to_fit();
        });
    }

    fn clear_error() {
        ERROR.with(|cell| {
            let mut error = cell.borrow_mut();
            error.clear();
            error.shrink_to_fit();
        });
    }

    fn begin_result() {
        clear_output();
        clear_error();
    }

    fn set_output(value: impl AsRef<str>) {
        OUTPUT.with(|cell| {
            let mut output = cell.borrow_mut();
            output.clear();
            output.extend_from_slice(value.as_ref().as_bytes());
        });
    }
    fn set_error(value: impl AsRef<str>) {
        ERROR.with(|cell| {
            let mut error = cell.borrow_mut();
            error.clear();
            error.extend_from_slice(value.as_ref().as_bytes());
        });
    }

    fn bridge_error(value: impl AsRef<str>) -> i32 {
        set_error(value);
        BRIDGE_ERROR
    }

    fn input_slot(slot: u32) -> Result<usize, String> {
        let slot = usize::try_from(slot)
            .map_err(|_| "invalid error Web input slot".to_string())?;
        (slot < INPUT_SLOT_COUNT)
            .then_some(slot)
            .ok_or_else(|| "invalid error Web input slot".to_string())
    }

    fn read_input(slot: u32, pointer: u32, length: u32, label: &str) -> Result<String, String> {
        let slot = input_slot(slot)?;
        let length = usize::try_from(length)
            .map_err(|_| format!("error Web {label} length is invalid"))?;
        INPUT.with(|cell| {
            let slots = cell.borrow();
            let bytes = &slots[slot];
            if bytes.len() != length {
                return Err(format!(
                    "error Web {label} length does not match its allocation"
                ));
            }
            let owned = if length == 0 {
                pointer == 0
            } else {
                pointer as usize == bytes.as_ptr() as usize
            };
            if !owned {
                return Err(format!(
                    "error Web {label} pointer is not owned by the bridge"
                ));
            }
            String::from_utf8(bytes.clone())
                .map_err(|_| format!("error Web {label} must be UTF-8"))
        })
    }


    fn report_to_error(report: &JetErrorReport) -> Result<JetErr, String> {
        let cause = match report.causes.as_slice() {
            [] => Err(JetAbsent),
            [cause] => Ok(report_to_error(cause)?),
            _ => return Err("error report has multiple causes".to_string()),
        };
        let mut error = match report.typed_identity.clone() {
            Some(identity) => jet_err_with_identity(
                report.message.clone(),
                report.code.clone().map_or(Err(JetAbsent), Ok),
                cause,
                identity,
            ),
            None => jet_err(
                report.message.clone(),
                report.code.clone().map_or(Err(JetAbsent), Ok),
                cause,
            ),
        };
        for frame in &report.context_frames {
            jet_err_add_context(
                &mut error,
                frame.text.clone(),
                frame.file.clone(),
                frame.line,
            );
        }
        for conversion in &report.conversion_history {
            jet_err_record_conversion(
                &mut error,
                conversion.source.clone(),
                conversion.target.clone(),
            );
        }
        if let Some(details) = &report.details {
            jet_err_set_details(&mut error, details.clone());
        }
        Ok(error)
    }
    fn read_error_at(slot: u32, pointer: u32, length: u32) -> Result<(JetErr, Journey), String> {
        let wire = read_input(slot, pointer, length, "error")?;
        parse_error(&wire)
    }

    fn read_error(pointer: u32, length: u32) -> Result<(JetErr, Journey), String> {
        read_error_at(ERROR_SLOT, pointer, length)
    }
    fn parse_error(wire: &str) -> Result<(JetErr, Journey), String> {
        let report = JetErrorReport::from_json(wire)?;
        let journey = Journey {
            origin: report.origin.as_ref().map(jet_journey_hop_from_frame),
            hops: report
                .source_journey
                .iter()
                .map(jet_journey_hop_from_frame)
                .collect(),
        };
        let error = report_to_error(&report)?;
        Ok((error, journey))
    }

    fn restore_journey(carried: Journey) {
        if carried.is_empty() {
            return;
        }
        JET_JOURNEY.with(|cell| *cell.borrow_mut() = carried);
    }

    fn write_carrier(error: &JetErr) {
        // Keep the source journey with the structured carrier.  The JS edge
        // may cross several `?` sites before the terminal report, so dropping
        // the thread-local hops here would make the Web report lose its trail.
        set_output(jet_error_report(error).to_json());
    }


    #[no_mangle]
    pub extern "C" fn jet_error_wasm_input_alloc(slot: u32, length: u32) -> u32 {
        let Ok(slot) = input_slot(slot) else {
            return 0;
        };
        let length = length as usize;
        INPUT.with(|cell| {
            let mut slots = cell.borrow_mut();
            let bytes = &mut slots[slot];
            if !bytes.is_empty() || length == 0 {
                return 0;
            }
            bytes.resize(length, 0);
            bytes.as_mut_ptr() as usize as u32
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_input_free(slot: u32, pointer: u32) -> u32 {
        let Ok(slot) = input_slot(slot) else {
            return 0;
        };
        INPUT.with(|cell| {
            let mut slots = cell.borrow_mut();
            let bytes = &mut slots[slot];
            let owned = if bytes.is_empty() {
                pointer == 0
            } else {
                pointer as usize == bytes.as_ptr() as usize
            };
            if !owned {
                return 0;
            }
            bytes.clear();
            bytes.shrink_to_fit();
            OK as u32
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_result_clear() {
        begin_result();
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_output_ptr() -> u32 {
        OUTPUT.with(|cell| cell.borrow().as_ptr() as usize as u32)
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_output_len() -> u32 {
        OUTPUT.with(|cell| {
            u32::try_from(cell.borrow().len()).expect("error Web output exceeds the u32 ABI")
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_error_ptr() -> u32 {
        ERROR.with(|cell| cell.borrow().as_ptr() as usize as u32)
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_error_len() -> u32 {
        ERROR.with(|cell| {
            u32::try_from(cell.borrow().len()).expect("error Web diagnostic exceeds the u32 ABI")
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_journey_reset() -> i32 {
        begin_result();
        jet_journey_reset();
        OK
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_journey_origin(
        file_pointer: u32,
        file_length: u32,
        line: u32,
        column: u32,
        function_pointer: u32,
        function_length: u32,
    ) -> i32 {
        begin_result();
        let file = match read_input(FILE_SLOT, file_pointer, file_length, "source file") {
            Ok(file) => file,
            Err(error) => return bridge_error(error),
        };
        let function =
            match read_input(FUNCTION_SLOT, function_pointer, function_length, "function") {
                Ok(function) => function,
                Err(error) => return bridge_error(error),
            };
        jet_journey_origin(&file, line, column, &function);
        OK
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_journey_frame_text(
        file_pointer: u32,
        file_length: u32,
        line: u32,
        column: u32,
        function_pointer: u32,
        function_length: u32,
        note_pointer: u32,
        note_length: u32,
    ) -> i32 {
        begin_result();
        let file = match read_input(FILE_SLOT, file_pointer, file_length, "source file") {
            Ok(file) => file,
            Err(error) => return bridge_error(error),
        };
        let function =
            match read_input(FUNCTION_SLOT, function_pointer, function_length, "function") {
                Ok(function) => function,
                Err(error) => return bridge_error(error),
            };
        let note = match read_input(NOTE_SLOT, note_pointer, note_length, "journey note") {
            Ok(note) => note,
            Err(error) => return bridge_error(error),
        };
        jet_journey_frame_text(&file, line, column, &function, &note);
        OK
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_err_from_message(
        message_pointer: u32,
        message_length: u32,
    ) -> i32 {
        begin_result();
        let message = match read_input(MESSAGE_SLOT, message_pointer, message_length, "message") {
            Ok(message) => message,
            Err(error) => return bridge_error(error),
        };
        write_carrier(&jet_err_from_message(message));
        OK
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_with_context_frame(
        error_pointer: u32,
        error_length: u32,
        file_pointer: u32,
        file_length: u32,
        line: u32,
        column: u32,
        function_pointer: u32,
        function_length: u32,
        note_pointer: u32,
        note_length: u32,
    ) -> i32 {
        begin_result();
        let (error, journey) = match read_error(error_pointer, error_length) {
            Ok(error) => error,
            Err(error) => return bridge_error(error),
        };
        let file = match read_input(FILE_SLOT, file_pointer, file_length, "source file") {
            Ok(file) => file,
            Err(error) => return bridge_error(error),
        };
        let function =
            match read_input(FUNCTION_SLOT, function_pointer, function_length, "function") {
                Ok(function) => function,
                Err(error) => return bridge_error(error),
            };
        let note = match read_input(NOTE_SLOT, note_pointer, note_length, "context note") {
            Ok(note) => note,
            Err(error) => return bridge_error(error),
        };
        restore_journey(journey);
        let error = jet_err_with_context_frame(error, &file, line, column, &function, note);
        write_carrier(&error);
        OK
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_from_conversion(
        converted_pointer: u32,
        converted_length: u32,
        original_pointer: u32,
        original_length: u32,
        source_pointer: u32,
        source_length: u32,
        target_pointer: u32,
        target_length: u32,
    ) -> i32 {
        begin_result();
        let (mut converted, converted_journey) =
            match read_error(converted_pointer, converted_length) {
                Ok(error) => error,
                Err(error) => return bridge_error(error),
            };
        let original = if original_length == 0 {
            None
        } else {
            match read_error_at(ORIGINAL_SLOT, original_pointer, original_length) {
                Ok(error) => Some(error),
                Err(error) => return bridge_error(error),
            }
        };
        let source = match read_input(SOURCE_SLOT, source_pointer, source_length, "conversion source")
        {
            Ok(source) => source,
            Err(error) => return bridge_error(error),
        };
        let target = match read_input(TARGET_SLOT, target_pointer, target_length, "conversion target")
        {
            Ok(target) => target,
            Err(error) => return bridge_error(error),
        };
        let original = original.map(|(original, journey)| {
            if converted_journey.is_empty() {
                restore_journey(journey);
            }
            original
        });
        restore_journey(converted_journey);
        if let Some(original) = original {
            if converted.typed_identity.is_none() {
                converted.typed_identity = original.typed_identity;
            }
            if converted.context.is_empty() {
                converted.context = original.context;
            }
            if converted.conversions.is_empty() {
                converted.conversions = original.conversions;
            }
            if converted.details.is_none() {
                converted.details = original.details;
            }
        }
        converted = jet_err_apply_conversion(converted, source, target);
        write_carrier(&converted);
        OK
    }

    #[no_mangle]
    pub extern "C" fn jet_error_wasm_entry_error_exit(
        error_pointer: u32,
        error_length: u32,
    ) -> i32 {
        begin_result();
        let (error, journey) = match read_error(error_pointer, error_length) {
            Ok(error) => error,
            Err(error) => return bridge_error(error),
        };
        restore_journey(journey);
        let report = jet_error_report(&error);
        let rendered = report.render();
        let journey_text = report.render_journey_with_style(JetReportStyle::PLAIN);
        set_output(format!(
            "{{\"tag\":\"Err\",\"error\":{},\"report\":{},\"journey\":{}}}",
            report.to_json(),
            jet_error_json_quote(&rendered),
            jet_error_json_quote(&journey_text)
        ));
        OK
    }

}


pub fn jet_render_err(error: &JetErr) -> String {
    error.to_string()
}

impl std::fmt::Display for JetErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        jet_write_err_value(f, self, 0)
    }
}

// Printing a handled error value is not the unhandled-entry report edge.
fn jet_write_err_value(out: &mut dyn std::fmt::Write, error: &JetErr, depth: usize) -> std::fmt::Result {
    if depth == 0 {
        match &error.code {
            Ok(code) => write!(out, "Error [{code}]: {}", error.message)?,
            Err(_) => write!(out, "Error: {}", error.message)?,
        }
    } else {
        for _ in 0..depth {
            out.write_str("  ")?;
        }
        write!(out, "cause: {}", error.message)?;
    }
    if let Some(identity) = &error.typed_identity {
        write!(out, " (type: {identity})")?;
    }
    if let Ok(cause) = &error.cause {
        out.write_char('\n')?;
        jet_write_err_value(out, cause, depth + 1)?;
    }
    for frame in &error.context {
        out.write_char('\n')?;
        for _ in 0..=depth {
            out.write_str("  ")?;
        }
        write!(out, "context ({}:{}): {}", frame.file, frame.line, frame.text)?;
    }
    for conversion in &error.conversions {
        out.write_char('\n')?;
        for _ in 0..=depth {
            out.write_str("  ")?;
        }
        write!(out, "conversion: {} -> {}", conversion.source, conversion.target)?;
    }
    Ok(())
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

    #[test]
    fn default_report_keeps_structured_error_and_source_facts() {
        jet_journey_reset();
        let cause = jet_err_with_identity(
            "disk offline".to_string(),
            Ok("IO001".to_string()),
            Err(JetAbsent),
            "IoError".to_string(),
        );
        let mut error = jet_err(
            "loading config".to_string(),
            Ok("CFG404".to_string()),
            Ok(cause),
        );
        jet_err_add_context(
            &mut error,
            "while loading app.toml".to_string(),
            "config.jet".to_string(),
            42,
        );
        error = jet_err_apply_conversion(
            error,
            "IoError".to_string(),
            "ConfigError".to_string(),
        );
        jet_journey_origin("config.jet", 40, 12, "load");
        jet_journey_frame("config.jet", 42, 5, "run", String::new);

        let report = jet_error_report(&error);
        assert_eq!(report.code, Some("CFG404".to_string()));
        assert_eq!(report.message, "loading config");
        assert_eq!(report.typed_identity, Some("IoError".to_string()));
        assert_eq!(report.causes.len(), 1);
        assert_eq!(report.causes[0].message, "disk offline");
        assert_eq!(report.causes[0].typed_identity, Some("IoError".to_string()));
        assert_eq!(report.context_frames.len(), 1);
        assert_eq!(report.context_frames[0].file, "config.jet");
        assert_eq!(report.context_frames[0].line, 42);
        let origin = report.origin.as_ref().expect("origin");
        assert_eq!(
            (origin.fn_name.as_str(), origin.file.as_str(), origin.line, origin.column),
            ("load", "config.jet", 40, 12)
        );
        assert_eq!(report.source_journey.len(), 1);
        assert_eq!(report.source_journey[0].fn_name, "run");
        assert_eq!(report.source_journey[0].file, "config.jet");
        assert_eq!(report.source_journey[0].line, 42);
        assert_eq!(report.source_journey[0].column, 5);
        assert_eq!(report.conversion_history.len(), 1);
        assert_eq!(report.conversion_history[0].source, "IoError");
        assert_eq!(report.conversion_history[0].target, "ConfigError");
        let wire = report.to_json();
        assert_eq!(
            wire,
            "{\"schema\":\"jet.err/v1\",\"message\":\"loading config\",\"code\":\"CFG404\",\"cause\":{\"schema\":\"jet.err/v1\",\"message\":\"disk offline\",\"code\":\"IO001\",\"cause\":null,\"typed_identity\":\"IoError\"},\"typed_identity\":\"IoError\",\"context_frames\":[{\"text\":\"while loading app.toml\",\"file\":\"config.jet\",\"line\":42}],\"origin\":{\"fn_name\":\"load\",\"file\":\"config.jet\",\"line\":40,\"column\":12,\"note\":\"\",\"hops\":1},\"source_journey\":[{\"fn_name\":\"run\",\"file\":\"config.jet\",\"line\":42,\"column\":5,\"note\":\"\",\"hops\":1}],\"conversion_history\":[{\"source\":\"IoError\",\"target\":\"ConfigError\"}]}"
        );
        // The wire is the report: a replayed wire keeps the origin and every
        // column instead of degrading to the rendered text.
        assert_eq!(JetErrorReport::from_json(&wire), Ok(report.clone()));
        assert_eq!(
            report.render_with_style(JetReportStyle::PLAIN),
            "Error [E3015]: Entry returned an unhandled error: loading config\n\
             \x20Why: The entry returned a failed outcome without a caller to handle it.\n\
             \x20Fix: Handle the failed outcome before it reaches the entry, or fix the operation that returned the error.\n\
             error code: CFG404\n\
             type: IoError\n\
             \x20\x20cause: disk offline (type: IoError; error code: IO001)\n\
             \x20\x20context (config.jet:42): while loading app.toml\n\
             \x20\x20conversion: IoError -> ConfigError\n\
             \x20\x20failed here: load (config.jet:40:12)\n\
             \x20Trail [E3002] (1 hop via ?, origin first):\n\
             \x20 1. run (config.jet:42:5)\n\
             More: jet-lang.dev/e/E3015\n"
        );
    }

    #[test]
    fn entry_report_escapes_data_and_keeps_recursive_facts_without_inventing_locations() {
        jet_journey_reset();
        let mut cause = jet_err_with_identity(
            "disk\n\x1boffline".to_string(), Ok("IO001".to_string()),
            Err(JetAbsent), "IoError".to_string(),
        );
        jet_err_add_context(&mut cause, "inner\tcontext".to_string(), "inner.jet".to_string(), 7);
        jet_err_set_details(&mut cause, JetErrorDetails {
            variant: "Offline".to_string(),
            fields: vec![JetErrorField { name: "attempts".to_string(), value: "17".to_string() }],
            source_span: Some(JetErrorSpan { start: 9, end: 12 }),
        });
        let error = jet_err("loading\u{2028}config".to_string(), Ok("CFG404".to_string()), Ok(cause));
        let mut report = jet_error_report(&error);
        report.causes[0].origin = Some(JetErrorJourneyFrame {
            fn_name: "read".to_string(), file: "inner.jet".to_string(), line: 7,
            column: 4, note: String::new(), hops: 1,
        });
        let wire = report.to_json();
        let text = report.render_with_style(JetReportStyle::PLAIN);
        assert!(text.starts_with("Error [E3015]: Entry returned an unhandled error: loading\\u{2028}config\n"));
        assert!(text.contains("error code: CFG404\n"));
        assert!(text.contains("cause: disk\\n\\u{001b}offline (type: IoError; error code: IO001)\n"));
        assert!(text.contains("context (inner.jet:7): inner\\tcontext\n"));
        assert!(text.contains("variant: Offline\n"));
        assert!(text.contains("attempts: 17\n"));
        assert!(text.contains("source span: 9..12\n"));
        assert!(text.contains("failed here: read (inner.jet:7:4)\n"));
        assert_eq!(text.matches("Error [E3015]").count(), 1);
        assert_eq!(text.matches("More: ").count(), 1);
        assert!(text.ends_with("More: jet-lang.dev/e/E3015\n"));
        assert!(!text.contains("Stop ["));
        assert!(!text.contains("Error [CFG404]"));
        assert!(!text.contains("  --> "));
        assert!(!text.contains(":0"));
        assert!(!text.contains('\x1b'));
        assert_eq!(report.to_json(), wire, "terminal escaping must not rewrite source data");
        assert_eq!(JetErrorReport::from_json(&wire), Ok(report));
    }
}

#[cfg(test)]
mod journey_tests {
    use super::*;

    // Each `#[test]` runs on its own thread, so the thread-local hop buffer is
    // this test's alone.
    /// The example the card's before/after uses, so these cells and the real
    /// runs in `tests/terminal.rs` are the same program.
    const EXAMPLE: &str = "Examples/features/errors/error_context.jet";

    #[test]
    fn standalone_journey_keeps_ordinary_text_policy() {
        jet_journey_reset();
        jet_journey_origin("app.jet", 3, 7, "load");
        jet_journey_frame("app.jet", 8, 9, "run", || "raw\n\x1bnote".to_string());
        assert_eq!(
            jet_journey_take(),
            "  failed here: load (app.jet:3)\n\
             \x20Trail [E3002] (1 hop via ?, origin first):\n\
             \x20 1. run (app.jet:8) — raw\n\x1bnote\n"
        );
    }

    #[test]
    fn journey_note_elision_preserves_raw_character_room() {
        for (note, error_report, suffix) in [
            ("abéΩtail", false, "abé…"),
            ("abéΩtail", true, "abé…"),
            ("a\nétail", false, "a\né…"),
            ("a\nétail", true, "a\\né…"),
        ] {
            let site = JetJourneySite {
                function: "run", file: "app.jet", line: 3, column: 0,
                note, hops: 1,
            };
            let label = JetJourneyLabel::Hop(1);
            let fixed = jet_journey_text_columns(label, site, (site.file, false), "", error_report);
            let mut rendered = String::new();
            jet_write_journey_hop_line(&mut rendered, label, site, (fixed + 7, 64), error_report)
                .unwrap();
            assert_eq!(rendered, format!("  1. run (app.jet:3) — {suffix}"));
        }
    }

    #[test]
    fn journey_path_elision_preserves_segment_and_basename_policy() {
        for (file, budget, expected) in [
            ("dir/app.jet", 32, ("dir/app.jet", false)),
            ("dir/app.jet", 9, ("app.jet", true)),
            ("dir/app.jet", 8, ("app.jet", false)),
            ("dir/sub/app.jet", 13, ("sub/app.jet", true)),
            ("dir/sub/app.jet", 12, ("app.jet", true)),
            ("singlelong.jet", 1, ("singlelong.jet", false)),
            ("dir/é.jet", 7, ("é.jet", true)),
            ("dir/é.jet", 6, ("é.jet", false)),
            ("dir/", 2, ("", true)),
            ("dir/", 1, ("", false)),
            ("dir/\napp.jet", 10, ("\napp.jet", true)),
        ] {
            assert_eq!(jet_report_elide_path(file, budget), expected);
        }
        for (error_report, expected) in [
            (false, "  1. run (…/\napp.jet:3)"),
            (true, "  1. run (…/\\napp.jet:3)"),
        ] {
            let site = JetJourneySite {
                function: "run", file: "dir/\napp.jet", line: 3, column: 0,
                note: "", hops: 1,
            };
            let mut rendered = String::new();
            jet_write_journey_hop_line(
                &mut rendered, JetJourneyLabel::Hop(1), site, (usize::MAX, 10), error_report,
            ).unwrap();
            assert_eq!(rendered, expected);
        }
    }

    /// `error_context.jet`: `read_raw` returns `Err("file not found")` on line
    /// 4, and three calls carry it out.
    fn three_real_hops() {
        jet_journey_reset();
        jet_journey_origin(EXAMPLE, 4, 27, "read_raw");
        jet_journey_frame(EXAMPLE, 7, 12, "parse_config", || {
            "reading raw config".to_string()
        });
        jet_journey_frame(EXAMPLE, 12, 12, "load_config", || {
            "loading config app.toml".to_string()
        });
        jet_journey_frame(EXAMPLE, 16, 17, "run", String::new);
    }

    #[test]
    fn report_leads_with_the_failure_then_where_it_started_then_the_trail() {
        jet_journey_reset();
        jet_journey_origin("app.jet", 3, 5, "read_raw");
        jet_journey_frame("app.jet", 7, 12, "parse_config", || {
            "reading raw config".to_string()
        });
        jet_journey_frame("app.jet", 16, 5, "run", String::new);

        assert_eq!(
            jet_journey_report_styled("Error: file not found", JetReportStyle::PLAIN),
            "Error: file not found\n\
             \x20\x20failed here: read_raw (app.jet:3)\n\
             \x20Trail [E3002] (2 hops via ?, origin first):\n\
             \x20 1. parse_config (app.jet:7) — reading raw config\n\
             \x20 2. run (app.jet:16)\n"
        );
    }

    #[test]
    fn a_failure_without_an_err_site_starts_at_its_first_hop() {
        // A failing Core call returns no `Err(...)` of the program's own, so
        // the call site it failed at is where the failure started.
        jet_journey_reset();
        jet_journey_frame("app.jet", 6, 13, "load", || "reading config".to_string());
        jet_journey_frame("app.jet", 11, 5, "run", String::new);

        let report = jet_error_report(&jet_err_from_message("file not found".to_string()));
        let origin = report.origin.as_ref().expect("the first hop is the origin");
        assert_eq!((origin.fn_name.as_str(), origin.line, origin.column), ("load", 6, 13));
        assert_eq!(origin.note, "reading config");
        assert_eq!(report.source_journey.len(), 1);
        assert_eq!(
            report.render_with_style(JetReportStyle::PLAIN),
            "Error [E3015]: Entry returned an unhandled error: file not found\n\
             \x20Why: The entry returned a failed outcome without a caller to handle it.\n\
             \x20Fix: Handle the failed outcome before it reaches the entry, or fix the operation that returned the error.\n\
             \x20\x20failed here: load (app.jet:6:13) — reading config\n\
             \x20Trail [E3002] (1 hop via ?, origin first):\n\
             \x20 1. run (app.jet:11:5)\n\
             More: jet-lang.dev/e/E3015\n"
        );
    }

    #[test]
    fn a_new_origin_drops_the_journey_of_an_earlier_handled_failure() {
        jet_journey_reset();
        jet_journey_origin("app.jet", 2, 5, "stale");
        jet_journey_frame("app.jet", 8, 5, "handled", String::new);
        jet_journey_origin("app.jet", 20, 9, "fresh");
        jet_journey_frame("app.jet", 30, 5, "run", String::new);

        assert_eq!(
            jet_journey_report_styled("Error: fresh", JetReportStyle::PLAIN),
            "Error: fresh\n\
             \x20\x20failed here: fresh (app.jet:20)\n\
             \x20Trail [E3002] (1 hop via ?, origin first):\n\
             \x20 1. run (app.jet:30)\n"
        );
    }

    #[test]
    fn a_failure_created_at_the_edge_names_its_line_and_has_no_trail() {
        jet_journey_reset();
        jet_journey_origin("app.jet", 5, 12, "run");
        assert_eq!(
            jet_journey_report_styled("Error: nothing to do", JetReportStyle::PLAIN),
            "Error: nothing to do\n\
             \x20\x20failed here: run (app.jet:5)\n"
        );
    }

    #[test]
    fn a_carried_journey_keeps_its_origin_across_threads() {
        jet_journey_reset();
        jet_journey_origin("app.jet", 2, 5, "stale");
        let carried = std::thread::spawn(|| {
            jet_journey_origin("worker.jet", 4, 9, "work");
            jet_journey_frame("worker.jet", 9, 5, "step", String::new);
            jet_journey_take_hops()
        })
        .join()
        .expect("worker");
        jet_journey_adopt(carried);
        jet_journey_frame("app.jet", 12, 5, "run", String::new);

        assert_eq!(
            jet_journey_report_styled("Error: worker failed", JetReportStyle::PLAIN),
            "Error: worker failed\n\
             \x20\x20failed here: work (worker.jet:4)\n\
             \x20Trail [E3002] (2 hops via ?, origin first):\n\
             \x20 1. step (worker.jet:9)\n\
             \x20 2. run (app.jet:12)\n"
        );
    }

    #[test]
    fn a_repeating_site_collapses_to_one_line_with_a_count() {
        jet_journey_reset();
        jet_journey_origin("app.jet", 3, 5, "bottom");
        jet_journey_frame("app.jet", 6, 5, "dive", String::new);
        for _ in 0..3 {
            jet_journey_frame("app.jet", 6, 5, "dive", || {
                panic!("a collapsed repeat must not evaluate its note")
            });
        }
        jet_journey_frame("app.jet", 9, 5, "run", String::new);

        assert_eq!(
            jet_journey_report_styled("Error: bottom", JetReportStyle::PLAIN),
            "Error: bottom\n\
             \x20\x20failed here: bottom (app.jet:3)\n\
             \x20Trail [E3002] (5 hops via ?, origin first):\n\
             \x20 1. dive (app.jet:6) ×4\n\
             \x20 2. run (app.jet:9)\n"
        );
    }

    #[test]
    fn a_failure_that_crossed_no_hop_reports_only_itself() {
        jet_journey_reset();
        assert_eq!(
            jet_journey_report_styled("Error: no trail", JetReportStyle::PLAIN),
            "Error: no trail\n"
        );
    }

    // The terminal state matrix (card #2044 criterion 2). Four cells, one
    // renderer, explicit facts — `jet_journey_report` resolves the same facts
    // from the process, and `tests/terminal.rs` proves that resolution against
    // a real PTY. Both read this same program.

    #[test]
    fn a_pipe_gets_no_ansi_and_whole_paths() {
        three_real_hops();
        assert_eq!(
            jet_journey_report_styled("Error: file not found", JetReportStyle::PLAIN),
            "Error: file not found\n\
             \x20\x20failed here: read_raw (Examples/features/errors/error_context.jet:4)\n\
             \x20Trail [E3002] (3 hops via ?, origin first):\n\
             \x20 1. parse_config (Examples/features/errors/error_context.jet:7) — reading raw config\n\
             \x20 2. load_config (Examples/features/errors/error_context.jet:12) — loading config app.toml\n\
             \x20 3. run (Examples/features/errors/error_context.jet:16)\n"
        );
    }

    #[test]
    fn a_colour_terminal_dims_the_trail_and_leaves_the_failure_bright() {
        three_real_hops();
        let report = jet_journey_report_styled(
            "Error: file not found",
            JetReportStyle {
                color: true,
                width: Some(80),
            },
        );
        assert_eq!(
            report,
            "Error: file not found\n\
             \x1b[2;37m  failed here: read_raw (…/errors/error_context.jet:4)\x1b[0m\n\
             \x1b[2;37m Trail [E3002] (3 hops via ?, origin first):\x1b[0m\n\
             \x1b[2;37m  1. parse_config (…/errors/error_context.jet:7) — reading raw config\x1b[0m\n\
             \x1b[2;37m  2. load_config (…/errors/error_context.jet:12) — loading config app.toml\x1b[0m\n\
             \x1b[2;37m  3. run (…/errors/error_context.jet:16)\x1b[0m\n"
        );
        // The root failure carries no SGR at all, which is the whole point: it
        // is the one line the eye lands on.
        assert!(!report.lines().next().expect("root line").contains('\x1b'));
    }

    #[test]
    fn no_color_on_a_terminal_keeps_the_layout_and_drops_the_ansi() {
        three_real_hops();
        // Same 80-column layout as the colour cell above, byte for byte, minus
        // the SGR pairs: colour and width are separate capabilities. Every
        // site also elides its one file to one spelling — the block's budget
        // is decided once, not per line.
        assert_eq!(
            jet_journey_report_styled(
                "Error: file not found",
                JetReportStyle {
                    color: false,
                    width: Some(80),
                },
            ),
            "Error: file not found\n\
             \x20\x20failed here: read_raw (…/errors/error_context.jet:4)\n\
             \x20Trail [E3002] (3 hops via ?, origin first):\n\
             \x20 1. parse_config (…/errors/error_context.jet:7) — reading raw config\n\
             \x20 2. load_config (…/errors/error_context.jet:12) — loading config app.toml\n\
             \x20 3. run (…/errors/error_context.jet:16)\n"
        );
    }

    #[test]
    fn a_narrow_terminal_keeps_every_location_and_sheds_the_prose() {
        three_real_hops();
        // 40 columns. The header drops its mechanism reminder, the paths fall
        // back to the file name that is never truncated, and the notes go —
        // three addressable hops under the root failure, nothing wrapped. The
        // origin keeps its whole address even when that is wider.
        let report = jet_journey_report_styled(
            "Error: file not found",
            JetReportStyle {
                color: false,
                width: Some(40),
            },
        );
        assert_eq!(
            report,
            "Error: file not found\n\
             \x20\x20failed here: read_raw (error_context.jet:4)\n\
             \x20Trail [E3002] (3 hops):\n\
             \x20 1. parse_config (error_context.jet:7)\n\
             \x20 2. load_config (error_context.jet:12)\n\
             \x20 3. run (error_context.jet:16)\n"
        );
        for line in report.lines().skip(2) {
            assert!(
                jet_report_columns(line) <= 40,
                "the trail must fit the terminal it was laid out for: {line:?}"
            );
        }
    }

    #[test]
    fn a_note_too_long_for_the_line_keeps_its_head_and_an_ellipsis() {
        jet_journey_reset();
        jet_journey_origin("a.jet", 1, 1, "g");
        jet_journey_frame("a.jet", 2, 5, "f", || {
            "a very long note that will not fit at all".to_string()
        });
        assert_eq!(
            jet_journey_report_styled(
                "Error: nope",
                JetReportStyle {
                    color: false,
                    width: Some(30),
                },
            ),
            "Error: nope\n\
             \x20\x20failed here: g (a.jet:1)\n\
             \x20Trail [E3002] (1 hop):\n\
             \x20 1. f (a.jet:2) — a very lon…\n"
        );
    }
}

/// The clean report: no payload, nothing to say. This is what `?T` puts on the
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

/// The optional view of the carrier: `?T`.
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

/// Build a present payload. `?T` and `T !E` build the same carrier.
pub fn jet_present<T, E>(value: T) -> JetOutcome<T, E> {
    Ok(value)
}

/// Build a clean absence.
pub fn jet_absent<T>() -> JetOutcome<T, JetAbsent> {
    Err(JetAbsent)
}

/// Mechanical native-kernel optional ABI conversion. An already-owned
/// absence carrier passes through without allocating or copying its payload.
pub trait JetNativeOptional<T, A> {
    fn into_owned_optional(self) -> Result<T, A>;
}

impl<T, A: Default> JetNativeOptional<T, A> for Option<T> {
    #[inline]
    fn into_owned_optional(self) -> Result<T, A> {
        self.ok_or_else(A::default)
    }
}

impl<T, A> JetNativeOptional<T, A> for Result<T, A> {
    #[inline]
    fn into_owned_optional(self) -> Result<T, A> {
        self
    }
}

#[inline]
pub fn jet_optional_from_native<T, A>(value: impl JetNativeOptional<T, A>) -> Result<T, A> {
    value.into_owned_optional()
}

#[cfg(test)]
mod native_optional_bridge_tests {
    use super::{jet_optional_from_native, JetAbsent};

    #[test]
    fn native_optional_bridge_preserves_payload_absence_and_outer_error() {
        let bytes = vec![128_u8, 255];
        let allocation = bytes.as_ptr();
        let native: Result<Option<Vec<u8>>, &str> = Ok(Some(bytes));
        let owned: Result<Result<Vec<u8>, JetAbsent>, &str> =
            native.map(jet_optional_from_native);
        assert_eq!(owned.as_ref().unwrap().as_ref().unwrap().as_ptr(), allocation);
        let absent: Result<Option<Vec<u8>>, &str> = Ok(None);
        let owned: Result<Result<Vec<u8>, JetAbsent>, &str> =
            absent.map(jet_optional_from_native);
        assert_eq!(owned, Ok(Err(JetAbsent)));
        let failed: Result<Option<Vec<u8>>, &str> = Err("kernel error");
        let owned: Result<Result<Vec<u8>, JetAbsent>, &str> =
            failed.map(jet_optional_from_native);
        assert_eq!(owned, Err("kernel error"));
        let already_owned: Result<Vec<u8>, JetAbsent> = Ok(vec![7]);
        let allocation = already_owned.as_ref().unwrap().as_ptr();
        let owned: Result<Vec<u8>, JetAbsent> = jet_optional_from_native(already_owned);
        assert_eq!(owned.as_ref().unwrap().as_ptr(), allocation);
    }
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
/// caller reads the whole thing as `?T`.
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
/// past it, `?T` is the carrier and nothing else.
pub fn jet_outcome_of<T>(value: Option<T>) -> JetOutcome<T, JetAbsent> {
    value.ok_or(JetAbsent)
}

/// One process-boundary queue law for every native execution tier.
pub fn jet_runtime_register_atexit<T>(handlers: &mut Vec<T>, handler: T) {
    handlers.push(handler);
}

/// Drain registered handlers exactly once, in registration order.
pub fn jet_runtime_drain_atexit<T>(handlers: &mut Vec<T>, mut invoke: impl FnMut(T)) {
    let pending = std::mem::take(handlers);
    for handler in pending {
        invoke(handler);
    }
}

#[cfg(test)]
mod process_exit_queue_tests {
    use super::{jet_runtime_drain_atexit, jet_runtime_register_atexit};

    #[test]
    fn pre_registered_exit_handlers_run_once_in_registration_order() {
        let mut handlers = Vec::new();
        for handler in [1, 2, 3] {
            jet_runtime_register_atexit(&mut handlers, handler);
        }

        let mut observed = Vec::new();
        jet_runtime_drain_atexit(&mut handlers, |handler| observed.push(handler));
        assert_eq!(observed, [1, 2, 3]);
        assert!(handlers.is_empty());

        jet_runtime_drain_atexit(&mut handlers, |handler| observed.push(handler));
        assert_eq!(observed, [1, 2, 3]);
    }
}

/// D-REPORT-RUNTIME1=A: the dependency-free runtime projection of one
/// registered diagnostic. AOT, JIT, and the interpreter marshal into this
/// value; none of those engines owns report text or exit policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetRuntimeObligationStatus {
    Failed,
}

impl JetRuntimeObligationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "false",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetRuntimeObligation {
    pub name: String,
    pub status: JetRuntimeObligationStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetRuntimeForeignEdge {
    pub component: String,
    pub fenced: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetRuntimeDiagnostic {
    pub code: &'static str,
    pub source: &'static str,
    pub what: String,
    pub why: String,
    pub fix: String,
    pub rendered: String,
    pub exit_code: i32,
    pub obligation: Option<JetRuntimeObligation>,
    pub foreign: Option<JetRuntimeForeignEdge>,
}

/// One active runtime row projected into a standalone Prelude. The host
/// wrapper below adapts Foundation's RegistryRow to this self-contained shape;
/// AOT and Wasm emit the same shape from the active Registry rows.
pub use crate::RuntimeDiagnosticCore::{
    JetRequireEqMessage, JetRuntimeDiagnosticRow, jet_require_eq_message,
    jet_require_message, jet_runtime_stop_has_context,
};
use crate::RuntimeDiagnosticCore::{
    JetRuntimeStopContext, jet_runtime_stop_fields, jet_runtime_stop_status,
    jet_write_diagnostic_template, jet_write_runtime_stop, jet_write_sentence_case,
};

impl JetRuntimeDiagnosticRow {
    fn render(self, holes: &[(&str, &str)]) -> (String, String, String) {
        for &(name, _) in holes {
            assert!(
                self.template_holes.iter().any(|hole| *hole == name),
                "diagnostic `{}` has no template hole `{name}`",
                self.code
            );
            assert_eq!(
                holes.iter().filter(|entry| entry.0 == name).count(),
                1,
                "diagnostic `{}` receives template hole `{name}` more than once",
                self.code
            );
        }
        for &name in self.template_holes {
            assert!(
                holes.iter().any(|entry| entry.0 == name),
                "diagnostic `{}` is missing template hole `{name}`",
                self.code
            );
        }

        (
            jet_render_diagnostic_template(self.what, holes),
            jet_render_diagnostic_template(self.why, holes),
            jet_render_diagnostic_template(self.fix, holes),
        )
    }
}

/// Fill one registered diagnostic template without treating replacement text
/// as a second template. Escaped braces remain literal.
pub fn jet_render_diagnostic_template(template: &str, holes: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    jet_write_diagnostic_template(&mut out, template, |out, name| {
        let value = holes.iter().find(|entry| entry.0 == name)
            .map(|entry| entry.1)
            .unwrap_or_else(|| panic!("missing diagnostic template hole `{name}`"));
        out.write_str(value)
    }).expect("String formatting cannot fail");
    out
}

/// Capitalize the first ordinary prose word while preserving the case of a
/// leading flag, identifier, ref, path, keyword, or code fragment. Runtime-
/// built diagnostic facts use this same product rule as table rows.
pub fn jet_sentence_case_line(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    jet_write_sentence_case(&mut out, &input).expect("String formatting cannot fail");
    out
}

/// D-DIAG-URL1: every human-readable diagnostic ends with this stable lookup
/// line. The code is kept as data so custom project reports use the same
/// shape without duplicating the host string.
pub fn jet_diagnostic_more_line(code: &str) -> String {
    let mut rendered = String::new();
    jet_write_diagnostic_more_line(&mut rendered, code).expect("String formatting cannot fail");
    rendered
}

fn jet_write_diagnostic_more_line(out: &mut dyn std::fmt::Write, code: &str) -> std::fmt::Result {
    write!(out, "More: jet-lang.dev/e/{code}")
}




/// Shared wording for a checked list position. Collection adapters marshal
/// the length and index here; they do not own the user-facing text.
pub fn jet_list_bounds_message(len: impl std::fmt::Display, index: i64) -> String {
    format!(
        "the list has {} items, so position {} doesn't exist",
        len, index
    )
}

/// Shared wording for a stale `Id<T>` — a pool slot that was removed. Sibling
/// of `jet_list_bounds_message`, and for the same reason: `jet_pool_get`,
/// `jet_pool_get_mut` and the JIT's pool host all reach a stale slot, and none
/// of them owns the user-facing text.
pub fn jet_pool_stale_message() -> &'static str {
    "this ID no longer refers to a live value — its pool slot was removed"
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

/// Dyn accesses share the map-index stop envelope; this helper owns only wording.
pub fn jet_dyn_stop_message(site: &str, detail: &str) -> String {
    format!("{site}: {detail}")
}

pub fn jet_test_expect_fail_matches(
    expected_code: Option<&str>,
    expected_message: Option<&str>,
    code: &str,
    message: &str,
) -> bool {
    expected_code.is_none_or(|expected| expected == code)
        && expected_message.is_none_or(|expected| message.contains(expected))
}

/// D-TEST-STOPMSG1=A: compare the raw stop, never its rendered diagnostic.
pub fn jet_test_expect_fail_error(
    expected_code: Option<&str>,
    expected_message: Option<&str>,
    stop: Option<(&str, &str)>,
) -> Option<String> {
    if let Some((code, message)) = stop {
        if jet_test_expect_fail_matches(expected_code, expected_message, code, message) {
            return None;
        }
        let expected = match (expected_code, expected_message) {
            (Some(code), Some(message)) => format!("a stop with {code} whose message contains {message:?}"),
            (None, Some(message)) => format!("a stop whose message contains {message:?}"),
            (Some(code), None) => format!("a stop with {code}"),
            (None, None) => unreachable!(),
        };
        return Some(format!("expected {expected}, got {code}: {message}"));
    }
    Some(match (expected_code, expected_message) {
        (Some(code), Some(message)) => format!("expected this region to stop with {code} whose message contains {message:?}, but it passed"),
        (None, Some(message)) => format!("expected a stop whose message contains {message:?}, but it passed"),
        (Some(code), None) => format!("expected this region to stop with {code}, but it passed"),
        (None, None) => "expected this region to fail, but it passed".to_string(),
    })
}

pub fn jet_test_timeout_message(elapsed_ns: i64, limit_ns: i64) -> String {
    format!("timeout: region took {elapsed_ns}ns, limit was {limit_ns}ns")
}

/// Shared wording for a reached typed goal.
pub fn jet_todo_message(file: &str, line: u32, expected_type: &str) -> String {
    format!("#Todo at {file}:{line} — expected {expected_type}")
}
/// Extract the payload from the legacy comptime-panic transport. `Diagnostic::error`
/// applies sentence casing at construction, so the registered `why` starts with
/// `While` even though the raise site uses `while`; compare the fixed prefix
/// without making the payload part of the presentation contract. This is
/// transport decoding only; runtime semantic policy remains at the stop site.
pub fn jet_comptime_panic_message<'a>(why: &'a str, what: &'a str) -> &'a str {
    const PREFIX: &str =
        "while computing this value at compile time, the program panicked: ";
    why.get(..PREFIX.len())
        .filter(|head| head.eq_ignore_ascii_case(PREFIX))
        .map(|_| &why[PREFIX.len()..])
        .unwrap_or(what)
}


/// Shared wording for the guarded recursion stop.
pub fn jet_stack_overflow_message(fn_name: &str) -> String {
    format!("stack overflow in `{fn_name}`")
}

pub fn jet_loop_stride_message() -> &'static str {
    "E0123: loop stride must be positive"
}

/// One recursion budget for every execution tier. The evaluator, AOT Prelude,
/// and JIT host all stop at the same Jet-defined depth before a native stack
/// fault can escape as a process crash.
// The evaluator keeps a larger Rust frame than generated Jet code. Keep the
// shared limit below the smallest native worker stack so it can report the
// stop before the host stack aborts. This is every program's budget; only a
// host that knows its threads' real stacks scales each thread's AOT budget to
// its stack (`jet_runtime_stack_scale_to_host`, Prelude/Core/RuntimeStack.rs).
pub const JET_RUNTIME_STACK_LIMIT: usize = 1024;



/// D-FAIL-BREACH1=A: the one renderer for a running program's breach stop.
///
/// The source location is data supplied by an execution tier. The active row,
/// report shape, and breach exit code live here so AOT, JIT, and TIR cannot
/// drift into separate panic printers.
pub fn jet_render_runtime_stop_from_row(
    row: Option<JetRuntimeDiagnosticRow>,
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
    let context = JetRuntimeStopContext {
        file, line, function: fn_name, source_line: src_line,
        column: col, caret_len,
        expected_type: message.rsplit_once(" — expected ").map(|(_, expected)| expected).unwrap_or(message),
    };
    let mut rendered = String::new();
    jet_write_runtime_stop(
        &mut rendered, row, code, context, &message,
        (!locals.is_empty()).then_some(&locals as &dyn std::fmt::Display),
    ).expect("String formatting cannot fail");
    let (source, [what, why, fix]) = match row {
        Some(row) => ("runtime", jet_runtime_stop_fields(row, code, context, &message).map(|field| field.to_string())),
        None => ("host", [
            format!("runtime diagnostic `{code}` is not an active runtime row"),
            "Jet could not resolve this stop through the active diagnostic registry".to_string(),
            "report this as a Jet compiler or host defect".to_string(),
        ]),
    };
    JetRuntimeDiagnostic {
        code, source, what, why, fix, rendered,
        exit_code: jet_runtime_stop_status(row),
        obligation: None,
        foreign: None,
    }
}

// JET_HOST_RUNTIME_STOP_BEGIN
// JET_HOST_RUNTIME_DIAGNOSTIC_LOOKUP_BEGIN
fn jet_runtime_diagnostic_row(code: &str) -> Option<JetRuntimeDiagnosticRow> {
    crate::Registry::active_runtime_diagnostic(code).map(|row| JetRuntimeDiagnosticRow {
        code: row.code,
        what: row.what,
        why: row.why,
        fix: row.fix,
        template_holes: row.template_holes,
    })
}
// JET_HOST_RUNTIME_DIAGNOSTIC_LOOKUP_END

/// Foundation-side adapter. Embedded Preludes strip only this wrapper and
/// append a generated lookup over the active Registry rows.
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
    let row = jet_runtime_diagnostic_row(code);
    jet_render_runtime_stop_from_row(
        row, code, file, line, fn_name, src_line, col, caret_len, message, locals,
    )
}
// JET_HOST_RUNTIME_STOP_END

/// D-MEM-SENTRY1: render one memory witness through the registered R0801
/// series. The source location and sentry facts come from the engine; the
/// diagnostic row owns What, Why, and Fix text for every execution tier.
pub fn jet_render_runtime_sentry_from_row(
    row: Option<JetRuntimeDiagnosticRow>,
    code: &'static str,
    file: &str,
    line: u32,
    gate: &str,
    operation: &str,
    obligation: &str,
    detail: &str,
    obligation_status: &str,
    foreign_component: Option<&str>,
    foreign_fenced: Option<bool>,
) -> JetRuntimeDiagnostic {
    let exit_code = jet_runtime_stop_status(row);
    let obligation_report = Some(JetRuntimeObligation {
        name: obligation.to_string(),
        status: JetRuntimeObligationStatus::Failed,
    });
    let foreign_report = foreign_component
        .filter(|component| !component.is_empty())
        .map(|component| JetRuntimeForeignEdge {
            component: component.to_string(),
            fenced: foreign_fenced.unwrap_or(false),
        });
    let Some(row) = row else {
        let what = format!("runtime diagnostic `{code}` is not an active runtime row");
        let why = "Jet could not resolve this sentry through the active diagnostic registry";
        let fix = "report this as a Jet compiler or host defect";
        let mut rendered = format!("Internal error: {what}\n Why: {why}\n Fix: {fix}\n");
        rendered.push_str(&format!(
            "obligation: {obligation} = {obligation_status}\n"
        ));
        if let Some(foreign) = foreign_report.as_ref() {
            rendered.push_str(&format!(
                "foreign: {} = {}\n",
                foreign.component,
                if foreign.fenced { "fenced" } else { "unfenced" }
            ));
        }
        rendered.push_str(&jet_diagnostic_more_line(code));
        rendered.push('\n');
        return JetRuntimeDiagnostic {
            code,
            source: "host",
            what: what.clone(),
            why: why.to_string(),
            fix: fix.to_string(),
            rendered,
            exit_code,
            obligation: obligation_report,
            foreign: foreign_report,
        };
    };

    let holes = row
        .template_holes
        .iter()
        .map(|hole| {
            let value = match *hole {
                "operation" => operation,
                "gate" => gate,
                "obligation" => obligation,
                "detail" | "msg" => detail,
                _ => detail,
            };
            (*hole, value)
        })
        .collect::<Vec<_>>();
    let (what, why, fix) = row.render(&holes);
    let what = jet_sentence_case_line(&what);
    let why = jet_sentence_case_line(&why);
    let fix = jet_sentence_case_line(&fix);
    let mut rendered = format!("{what}\n");
    if !file.is_empty() {
        rendered.push_str(&format!(
            "  --> {file}:{line}, in #Unsafe gate {gate:?}\n"
        ));
    }
    rendered.push_str(&format!(" Why: {why}\n Fix: {fix}\n"));
    rendered.push_str(&format!(
        "obligation: {obligation} = {obligation_status}\n"
    ));
    if let Some(foreign) = foreign_report.as_ref() {
        rendered.push_str(&format!(
            "foreign: {} = {}\n",
            foreign.component,
            if foreign.fenced { "fenced" } else { "unfenced" }
        ));
    }
    std::fmt::Write::write_fmt(&mut rendered, format_args!("exit: {exit_code}\n"))
        .expect("String formatting cannot fail");
    rendered.push_str(&jet_diagnostic_more_line(code));
    rendered.push('\n');
    JetRuntimeDiagnostic {
        code,
        source: "runtime",
        what,
        why,
        fix,
        rendered,
        exit_code,
        obligation: obligation_report,
        foreign: foreign_report,
    }
}

// JET_HOST_RUNTIME_SENTRY_BEGIN
/// Foundation-side adapter. Embedded Preludes strip this wrapper and append a
/// generated lookup over the active Registry rows.
pub fn jet_render_runtime_sentry(
    code: &'static str,
    file: &str,
    line: u32,
    gate: &str,
    operation: &str,
    obligation: &str,
    detail: &str,
) -> JetRuntimeDiagnostic {
    jet_render_runtime_sentry_with_context(
        code,
        file,
        line,
        gate,
        operation,
        obligation,
        detail,
        "false",
        None,
        None,
    )
}

pub fn jet_render_runtime_sentry_with_context(
    code: &'static str,
    file: &str,
    line: u32,
    gate: &str,
    operation: &str,
    obligation: &str,
    detail: &str,
    obligation_status: &str,
    foreign_component: Option<&str>,
    foreign_fenced: Option<bool>,
) -> JetRuntimeDiagnostic {
    let row = crate::Registry::active_runtime_diagnostic(code).map(|row| JetRuntimeDiagnosticRow {
        code: row.code,
        what: row.what,
        why: row.why,
        fix: row.fix,
        template_holes: row.template_holes,
    });
    jet_render_runtime_sentry_from_row(
        row,
        code,
        file,
        line,
        gate,
        operation,
        obligation,
        detail,
        obligation_status,
        foreign_component,
        foreign_fenced,
    )
}
// JET_HOST_RUNTIME_SENTRY_END

#[cfg(test)]
mod expected_stop_message_tests {
    use super::*;

    #[test]
    fn expected_stop_compares_code_and_raw_case_sensitive_substring() {
        let stop = Some(("E3001", "internal compiler error: E0859 fingerprint collision"));
        assert_eq!(jet_test_expect_fail_error(None, Some("fingerprint collision"), stop), None);
        assert_eq!(jet_test_expect_fail_error(Some("E3001"), Some("fingerprint collision"), stop), None);
        assert_eq!(jet_test_expect_fail_error(Some("E3010"), None, Some(("E3010", "index 99"))), None);
        assert_eq!(
            jet_test_expect_fail_error(None, Some("fingerprint collision"), Some(("E3001", "unrelated panic"))),
            Some("expected a stop whose message contains \"fingerprint collision\", got E3001: unrelated panic".to_string()),
        );
        assert!(jet_test_expect_fail_error(Some("E3010"), Some("fingerprint collision"), stop).is_some());
        assert!(jet_test_expect_fail_error(None, Some("Fingerprint collision"), stop).is_some());
        assert!(jet_test_expect_fail_error(None, Some("panic:"), stop).is_some());
        assert!(jet_test_expect_fail_error(None, Some(".*"), stop).is_some());
        assert_eq!(jet_test_expect_fail_error(None, Some("[.*]"), Some(("E3001", "literal [.*] text"))), None);
        assert_eq!(
            jet_test_expect_fail_error(None, Some("fingerprint collision"), None),
            Some("expected a stop whose message contains \"fingerprint collision\", but it passed".to_string()),
        );
    }

    #[test]
    fn producer_stop_transports_raw_code_message_and_frame_together() {
        jet_stream_record_failure_report("E3010", "index 99", "rendered frame, not the message".to_string());
        let stop = jet_stream_take_failure_report().expect("producer stop");
        assert_eq!(stop.code, "E3010");
        assert_eq!(stop.message, "index 99");
        assert_eq!(stop.rendered, "rendered frame, not the message");
        assert_eq!(jet_test_expect_fail_error(Some("E3010"), Some("99"), Some((&stop.code, &stop.message))), None);
        assert!(jet_stream_take_failure_report().is_none());
    }
}
