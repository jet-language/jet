// D-FAIL-CARRIER1=A / D-FAIL-MODEL1=A (ratified 2026-08-06) — the one outcome
// carrier under `T?` and `T E!`.
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
//   * `T E!`  is `JetOutcome<T, E>` — the report matters, so `E` is the report.
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
}

/// One source-linked hop in the report's structured journey. This is the
/// report view of the private journey accumulator below; callers never need
/// to recover source facts from rendered text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JetErrorJourneyFrame {
    pub fn_name: String,
    pub file: String,
    pub line: u32,
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
    pub source_journey: Vec<JetErrorJourneyFrame>,
    pub conversion_history: Vec<JetErrorConversion>,
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
    let mut error = jet_err(message, code, cause);
    jet_err_apply_conversion(&mut error, source, target);
    error
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

pub fn jet_err_add_context(error: &mut JetErr, text: String, file: String, line: u32) {
    error.context.push(JetErrorContextFrame { text, file, line });
}

pub fn jet_err_record_conversion(error: &mut JetErr, source: String, target: String) {
    error.conversions.push(JetErrorConversion { source, target });
}

/// Apply one declared conversion to an existing structured error. The
/// conversion boundary owns the source identity and history; the converted
/// error keeps its message, code, cause, context, and any earlier metadata.
pub fn jet_err_apply_conversion(error: &mut JetErr, source: String, target: String) {
    if error.typed_identity.is_none() {
        error.typed_identity = Some(source.clone());
    }
    jet_err_record_conversion(error, source, target);
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
    error
        .cause
        .as_ref()
        .map(|cause| (**cause).clone())
        .map_err(|_| JetAbsent)
}

// D-FAIL-CTX1: shared development journey state and rendering. Each `?`
// adapter claims its source site here, so AOT, JIT, and TIR-eval print one
// journey vocabulary and collapse only consecutive duplicate hops.
const JET_JOURNEY_DIAGNOSTIC_CODE: &str = "E3002";

/// One `?` site on the failure's way out.
struct JourneyFrame {
    fn_name: String,
    file: String,
    line: u32,
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

thread_local! {
    static JET_JOURNEY_HOPS: std::cell::RefCell<Vec<JourneyHop>> = const {
        std::cell::RefCell::new(Vec::new())
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
    JET_JOURNEY_HOPS.with(|hops| hops.borrow_mut().clear());
}

/// Drain the accumulated hops as E3002's rendered trail block, or an empty
/// string when the failure never crossed a `?`.
///
/// Plain on purpose. The generated wasm store puts this string into JSON, where
/// ANSI and a column budget are both wrong. The terminal form of the same block
/// is [`jet_journey_report`].
pub fn jet_journey_take() -> String {
    jet_journey_take_styled(JetReportStyle::PLAIN)
}

fn jet_journey_take_styled(style: JetReportStyle) -> String {
    JET_JOURNEY_HOPS.with(|hops| jet_journey_trail(&std::mem::take(&mut *hops.borrow_mut()), style))
}

/// The hops a run has accumulated so far, moved off this thread.
///
/// The journey belongs to the running program, but it lives in a thread-local
/// because each `task` owns its own. An engine that moves one program across
/// threads for its own reasons therefore has to move the journey with it: the
/// TIR evaluator runs every program on a 64 MiB worker thread and joins before
/// its report edge, so `?` pushed hops the worker owned and
/// `jet_journey_report` drained the caller's empty list — `jet run --interpret`
/// printed the failure with no trail while AOT and the resident tier printed
/// the hops (I9).
///
/// The hop list never leaves this module. An engine can only move the opaque
/// carrier and hand it back to [`jet_journey_adopt`], so collapse, order, and
/// rendering stay owned here. The structured default-error edge uses
/// [`jet_error_report`] so the journey is projected beside the error facts;
/// string-only edges use this renderer directly.
pub struct JetJourneyHops(Vec<JourneyHop>);

/// Move this thread's accumulated hops out, for a caller that is about to
/// carry them across a thread boundary it created.
pub fn jet_journey_take_hops() -> JetJourneyHops {
    JET_JOURNEY_HOPS.with(|hops| JetJourneyHops(std::mem::take(&mut *hops.borrow_mut())))
}

impl JetErrorReport {
    fn from_error(error: &JetErr, source_journey: Vec<JetErrorJourneyFrame>) -> Self {
        let causes = match &error.cause {
            Ok(cause) => vec![Self::from_error(cause, Vec::new())],
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
            source_journey,
            conversion_history: error.conversions.clone(),
        }
    }

    fn render_root(&self) -> String {
        fn render(report: &JetErrorReport, out: &mut String, depth: usize) {
            if depth == 0 {
                match &report.code {
                    Some(code) => out.push_str(&format!("Error [{code}]: {}", report.message)),
                    None => out.push_str(&format!("Error: {}", report.message)),
                }
            } else {
                out.push_str(&"  ".repeat(depth));
                out.push_str("cause: ");
                out.push_str(&report.message);
            }
            if let Some(identity) = &report.typed_identity {
                out.push_str(&format!(" (type: {identity})"));
            }
            for cause in &report.causes {
                out.push('\n');
                render(cause, out, depth + 1);
            }
            for frame in &report.context_frames {
                out.push('\n');
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&format!(
                    "context ({}:{}): {}",
                    frame.file, frame.line, frame.text
                ));
            }
            for conversion in &report.conversion_history {
                out.push('\n');
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&format!(
                    "conversion: {} -> {}",
                    conversion.source, conversion.target
                ));
            }
        }

        let mut out = String::new();
        render(self, &mut out, 0);
        out
    }

    fn journey_hops(&self) -> Vec<JourneyHop> {
        self
            .source_journey
            .iter()
            .map(|frame| JourneyHop {
                site: JourneyFrame {
                    fn_name: frame.fn_name.clone(),
                    file: frame.file.clone(),
                    line: frame.line,
                },
                note: frame.note.clone(),
                hops: frame.hops,
            })
            .collect()
    }

    /// Render only the structured source journey. The report edge uses this
    /// beside the root report for terminal and wire adapters.
    pub fn render_journey_with_style(&self, style: JetReportStyle) -> String {
        jet_journey_trail(&self.journey_hops(), style)
    }

    /// Render this already-projected report with the one terminal policy.
    /// The projection and the rendering are separate so a wire or inspector
    /// can consume the same facts without first making a display string.
    pub fn render_with_style(&self, style: JetReportStyle) -> String {
        jet_journey_compose(
            &self.render_root(),
            &self.render_journey_with_style(style),
        )
    }

    pub fn render(&self) -> String {
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
        if !self.source_journey.is_empty() {
            fields.push(format!(
                "\"source_journey\":[{}]",
                self.source_journey
                    .iter()
                    .map(|frame| {
                        format!(
                            "{{\"fn_name\":{},\"file\":{},\"line\":{},\"note\":{},\"hops\":{}}}",
                            quote(&frame.fn_name),
                            quote(&frame.file),
                            frame.line,
                            quote(&frame.note),
                            frame.hops
                        )
                    })
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
        format!("{{{}}}", fields.join(","))
    }
}

/// Project one escaping default error and the journey it accumulated into the
/// report consumed by every report edge. This drains the journey exactly once
/// and never asks `Display` to reconstruct any field.
pub fn jet_error_report(error: &JetErr) -> JetErrorReport {
    let journey = jet_journey_take_hops();
    let source_journey = journey
        .0
        .into_iter()
        .map(|hop| JetErrorJourneyFrame {
            fn_name: hop.site.fn_name,
            file: hop.site.file,
            line: hop.site.line,
            note: hop.note,
            hops: hop.hops,
        })
        .collect();
    JetErrorReport::from_error(error, source_journey)
}

/// Adopt hops carried in from another thread, oldest first, applying the same
/// consecutive-site collapse a fresh `?` applies — a site that re-propagates
/// across the seam adds its count to one line instead of opening a second.
pub fn jet_journey_adopt(carried: JetJourneyHops) {
    JET_JOURNEY_HOPS.with(|hops| {
        let mut hops = hops.borrow_mut();
        for hop in carried.0 {
            let collapsed = hops.last_mut().is_some_and(|last| {
                if last
                    .site
                    .same_site(&hop.site.file, hop.site.line, &hop.site.fn_name)
                {
                    last.hops += hop.hops;
                    return true;
                }
                false
            });
            if !collapsed {
                hops.push(hop);
            }
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

/// E3002's trail block. The root failure has already been stated by the time
/// this prints, so a hop only has to say where the failure passed: the numbered
/// list reads origin first, one line per site, `×N` when a site repeated, and
/// the hop's note after an em dash. The header carries the code and the
/// mechanism once instead of repeating `error propagated from … via ?` on every
/// line, which is what buried the root cause under its own trail.
///
/// `style` is the whole terminal-state matrix. Colour dims this block and only
/// this block, so the root failure above stays the one undimmed line and stays
/// what the eye lands on. A column budget sheds this block's disposable parts
/// and never reaches the root failure, which is not this function's line.
fn jet_journey_trail(hops: &[JourneyHop], style: JetReportStyle) -> String {
    if hops.is_empty() {
        return String::new();
    }
    let total: u32 = hops.iter().map(|hop| hop.hops).sum();
    // One path budget for the whole block, decided by its widest hop line, so
    // one file never renders two different ways under one header.
    let layout = style
        .width
        .map(|width| (width, jet_journey_path_budget(hops, width)));
    let mut lines = Vec::with_capacity(hops.len() + 1);
    lines.push(jet_journey_header(total, style.width));
    for (index, hop) in hops.iter().enumerate() {
        lines.push(jet_journey_hop_line(index + 1, hop, layout));
    }
    let mut trail = String::new();
    for line in &lines {
        // One SGR pair per line, not one around the block: a pager or a
        // line-oriented filter that keeps one line still gets its reset.
        if style.color {
            trail.push_str(JET_REPORT_DIM_SGR);
        }
        trail.push_str(line);
        if style.color {
            trail.push_str(JET_REPORT_SGR_RESET);
        }
        trail.push('\n');
    }
    trail
}

/// The header separates the root failure from its trail, so it must not wrap.
/// When the full form does not fit, the reminder of the mechanism goes and the
/// facts stay: the code and the hop count. That short form is the floor.
fn jet_journey_header(total: u32, width: Option<usize>) -> String {
    let plural = if total == 1 { "" } else { "s" };
    let code = JET_JOURNEY_DIAGNOSTIC_CODE;
    let full = format!(" Trail [{code}] ({total} hop{plural} via ?, origin first):");
    match width {
        Some(width) if jet_report_columns(&full) > width => {
            format!(" Trail [{code}] ({total} hop{plural}):")
        }
        _ => full,
    }
}

/// The columns left for a hop's file path once the widest hop line's fixed
/// parts are paid for.
fn jet_journey_path_budget(hops: &[JourneyHop], width: usize) -> usize {
    let widest = hops
        .iter()
        .enumerate()
        .map(|(index, hop)| {
            jet_report_columns(&jet_journey_hop_text(index + 1, hop, "", &hop.note))
        })
        .max()
        .unwrap_or(0);
    width.saturating_sub(widest)
}

/// One hop line, laid out to the block's width.
///
/// A hop's identity is its number, its `fn` and `file:line` — that is the
/// address of the code to open, and nothing sheds it. What sheds, in order:
/// leading path segments, then the note's tail. So a narrow terminal loses
/// commentary, never a location, and never the root failure above.
fn jet_journey_hop_line(number: usize, hop: &JourneyHop, layout: Option<(usize, usize)>) -> String {
    let Some((width, path_budget)) = layout else {
        return jet_journey_hop_text(number, hop, &hop.site.file, &hop.note);
    };
    let file = jet_report_elide_path(&hop.site.file, path_budget);
    let full = jet_journey_hop_text(number, hop, &file, &hop.note);
    if jet_report_columns(&full) <= width {
        return full;
    }
    let identity = jet_journey_hop_text(number, hop, &file, "");
    // ` — ` plus the ellipsis costs four columns; a note that cannot show one
    // character on top of that goes entirely rather than leaving ` — …`.
    let room = width.saturating_sub(jet_report_columns(&identity) + 4);
    if room == 0 {
        return identity;
    }
    let note: String = hop.note.chars().take(room).collect();
    jet_journey_hop_text(number, hop, &file, &format!("{note}…"))
}

fn jet_journey_hop_text(number: usize, hop: &JourneyHop, file: &str, note: &str) -> String {
    let mut line = format!(
        "  {number}. {} ({file}:{})",
        hop.site.fn_name, hop.site.line
    );
    if hop.hops > 1 {
        line.push_str(&format!(" ×{}", hop.hops));
    }
    if !note.is_empty() {
        line.push_str(&format!(" — {note}"));
    }
    line
}

/// Shed whole leading segments, never characters inside a name, so the file you
/// have to open is still spelled correctly. `…/` marks what went. The basename
/// is the floor: a half-spelled file name is worse than a line that wraps.
fn jet_report_elide_path(file: &str, budget: usize) -> String {
    if jet_report_columns(file) <= budget {
        return file.to_string();
    }
    let mut start = 0;
    while let Some(slash) = file[start..].find('/') {
        start += slash + 1;
        let candidate = format!("…/{}", &file[start..]);
        if jet_report_columns(&candidate) <= budget {
            return candidate;
        }
    }
    file[start..].to_string()
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
    jet_journey_report_styled(error, JetReportStyle::for_stderr())
}

/// The same report against explicit terminal facts, which is how the terminal
/// state matrix is proven. Nothing else may render a second trail.
pub fn jet_journey_report_styled(error: &str, style: JetReportStyle) -> String {
    jet_journey_compose(error, &jet_journey_take_styled(style))
}

/// Claim one `?` site for the failure now on its way out. Nothing prints here:
/// the hop reaches stderr only at the report edge, and only if the failure
/// escapes the entry.
pub fn jet_journey_frame<F: FnOnce() -> String>(file: &str, line: u32, fn_name: &str, note: F) {
    JET_JOURNEY_HOPS.with(|hops| {
        let mut hops = hops.borrow_mut();
        if let Some(last) = hops.last_mut() {
            if last.site.same_site(file, line, fn_name) {
                last.hops += 1;
                return;
            }
        }
        hops.push(JourneyHop {
            site: JourneyFrame {
                fn_name: fn_name.to_string(),
                file: file.to_string(),
                line,
            },
            note: note(),
            hops: 1,
        });
    });
}

/// Propagate a default error with one explicit context frame. The note is
/// evaluated once on the failure path, then stored both as structured context
/// and as the source-journey note.
pub fn jet_trace_err_note_jet<T, F: FnOnce() -> String>(
    result: JetOutcome<T, JetErr>,
    file: &str,
    line: u32,
    fn_name: &str,
    note: F,
) -> JetOutcome<T, JetErr> {
    match result {
        Ok(value) => {
            jet_journey_reset();
            Ok(value)
        }
        Err(mut error) => {
            let text = note();
            jet_err_add_context(&mut error, text.clone(), file.to_string(), line);
            jet_journey_frame(file, line, fn_name, || text);
            Err(error)
        }
    }
}

pub fn jet_render_err(error: &JetErr) -> String {
    JetErrorReport::from_error(error, Vec::new()).render_root()
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
        jet_err_apply_conversion(
            &mut error,
            "IoError".to_string(),
            "ConfigError".to_string(),
        );
        jet_journey_frame("config.jet", 42, "run", String::new);

        let report = jet_error_report(&error);
        assert_eq!(report.code, Some("CFG404".to_string()));
        assert_eq!(report.message, "loading config");
        assert_eq!(report.typed_identity, Some("ConfigError".to_string()));
        assert_eq!(report.causes.len(), 1);
        assert_eq!(report.causes[0].message, "disk offline");
        assert_eq!(report.causes[0].typed_identity, Some("IoError".to_string()));
        assert_eq!(report.context_frames.len(), 1);
        assert_eq!(report.context_frames[0].file, "config.jet");
        assert_eq!(report.context_frames[0].line, 42);
        assert_eq!(report.source_journey.len(), 1);
        assert_eq!(report.source_journey[0].fn_name, "run");
        assert_eq!(report.source_journey[0].file, "config.jet");
        assert_eq!(report.source_journey[0].line, 42);
        assert_eq!(report.conversion_history.len(), 1);
        assert_eq!(report.conversion_history[0].source, "IoError");
        assert_eq!(report.conversion_history[0].target, "ConfigError");
        assert_eq!(
            report.to_json(),
            "{\"schema\":\"jet.err/v1\",\"message\":\"loading config\",\"code\":\"CFG404\",\"cause\":{\"schema\":\"jet.err/v1\",\"message\":\"disk offline\",\"code\":\"IO001\",\"cause\":null,\"typed_identity\":\"IoError\"},\"typed_identity\":\"ConfigError\",\"context_frames\":[{\"text\":\"while loading app.toml\",\"file\":\"config.jet\",\"line\":42}],\"source_journey\":[{\"fn_name\":\"run\",\"file\":\"config.jet\",\"line\":42,\"note\":\"\",\"hops\":1}],\"conversion_history\":[{\"source\":\"IoError\",\"target\":\"ConfigError\"}]}"
        );
        assert_eq!(
            report.render_with_style(JetReportStyle::PLAIN),
            "Error [CFG404]: loading config (type: ConfigError)\n\
             \x20\x20cause: disk offline (type: IoError)\n\
             \x20\x20context (config.jet:42): while loading app.toml\n\
             \x20\x20conversion: IoError -> ConfigError\n\
             \x20Trail [E3002] (1 hop via ?, origin first):\n\
             \x20 1. run (config.jet:42)\n"
        );
    }
}

#[cfg(test)]
mod journey_tests {
    use super::*;

    // Each `#[test]` runs on its own thread, so the thread-local hop buffer is
    // this test's alone.
    /// The example the card's before/after uses, so these cells and the real
    /// runs in `tests/terminal.rs` are the same program.
    const EXAMPLE: &str = "examples/features/errors/error_context.jet";

    fn three_real_hops() {
        jet_journey_reset();
        jet_journey_frame(EXAMPLE, 7, "parse_config", || {
            "reading raw config".to_string()
        });
        jet_journey_frame(EXAMPLE, 12, "load_config", || {
            "loading config app.toml".to_string()
        });
        jet_journey_frame(EXAMPLE, 16, "run", String::new);
    }

    #[test]
    fn report_leads_with_the_failure_and_puts_the_trail_under_it() {
        jet_journey_reset();
        jet_journey_frame("app.jet", 7, "parse_config", || {
            "reading raw config".to_string()
        });
        jet_journey_frame("app.jet", 12, "load_config", || {
            "loading config".to_string()
        });
        jet_journey_frame("app.jet", 16, "run", String::new);

        assert_eq!(
            jet_journey_report_styled("Error: file not found", JetReportStyle::PLAIN),
            "Error: file not found\n\
             \x20Trail [E3002] (3 hops via ?, origin first):\n\
             \x20 1. parse_config (app.jet:7) — reading raw config\n\
             \x20 2. load_config (app.jet:12) — loading config\n\
             \x20 3. run (app.jet:16)\n"
        );
    }

    #[test]
    fn a_repeating_site_collapses_to_one_line_with_a_count() {
        jet_journey_reset();
        jet_journey_frame("app.jet", 6, "dive", String::new);
        for _ in 0..3 {
            jet_journey_frame("app.jet", 6, "dive", || {
                panic!("a collapsed repeat must not evaluate its note")
            });
        }
        jet_journey_frame("app.jet", 9, "run", String::new);

        assert_eq!(
            jet_journey_report_styled("Error: bottom", JetReportStyle::PLAIN),
            "Error: bottom\n\
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
             \x20Trail [E3002] (3 hops via ?, origin first):\n\
             \x20 1. parse_config (examples/features/errors/error_context.jet:7) — reading raw config\n\
             \x20 2. load_config (examples/features/errors/error_context.jet:12) — loading config app.toml\n\
             \x20 3. run (examples/features/errors/error_context.jet:16)\n"
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
        // the SGR pairs: colour and width are separate capabilities. Every hop
        // also elides its one file to one spelling — the block's budget is
        // decided once, not per line.
        assert_eq!(
            jet_journey_report_styled(
                "Error: file not found",
                JetReportStyle {
                    color: false,
                    width: Some(80),
                },
            ),
            "Error: file not found\n\
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
        // three addressable sites under the root failure, nothing wrapped.
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
             \x20Trail [E3002] (3 hops):\n\
             \x20 1. parse_config (error_context.jet:7)\n\
             \x20 2. load_config (error_context.jet:12)\n\
             \x20 3. run (error_context.jet:16)\n"
        );
        for line in report.lines().skip(1) {
            assert!(
                jet_report_columns(line) <= 40,
                "the trail must fit the terminal it was laid out for: {line:?}"
            );
        }
    }

    #[test]
    fn a_note_too_long_for_the_line_keeps_its_head_and_an_ellipsis() {
        jet_journey_reset();
        jet_journey_frame("a.jet", 1, "f", || {
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
             \x20Trail [E3002] (1 hop):\n\
             \x20 1. f (a.jet:1) — a very lon…\n"
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

/// Build a present payload. `T?` and `T E!` build the same carrier.
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

/// One process-boundary queue law for every native execution tier.
pub fn jet_runtime_register_atexit<T>(handlers: &mut Vec<T>, handler: T) {
    handlers.push(handler);
}

pub fn jet_runtime_drain_atexit<T>(handlers: &mut Vec<T>, mut invoke: impl FnMut(T)) {
    let pending = std::mem::take(handlers);
    for handler in pending {
        invoke(handler);
    }
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

/// One active runtime row projected into a standalone Prelude. The host
/// wrapper below adapts Foundation's RegistryRow to this self-contained shape;
/// AOT and Wasm emit the same shape from the active Registry rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetRuntimeDiagnosticRow {
    pub code: &'static str,
    pub what: &'static str,
    pub why: &'static str,
    pub fix: &'static str,
    pub template_holes: &'static [&'static str],
}

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
            jet_sentence_case_line(&jet_render_diagnostic_template(self.what, holes)),
            jet_sentence_case_line(&jet_render_diagnostic_template(self.why, holes)),
            jet_sentence_case_line(&jet_render_diagnostic_template(self.fix, holes)),
        )
    }
}

/// Fill one registered diagnostic template without treating replacement text
/// as a second template. Escaped braces remain literal.
pub fn jet_render_diagnostic_template(template: &str, holes: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' && bytes.get(index + 1) == Some(&b'{') {
            out.push('{');
            index += 2;
            continue;
        }
        if bytes[index] == b'}' && bytes.get(index + 1) == Some(&b'}') {
            out.push('}');
            index += 2;
            continue;
        }
        if bytes[index] == b'{' {
            if let Some(close_offset) = template[index + 1..].find('}') {
                let close = index + 1 + close_offset;
                let name = &template[index + 1..close];
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    let value = holes
                        .iter()
                        .find(|entry| entry.0 == name)
                        .map(|entry| entry.1)
                        .unwrap_or_else(|| panic!("missing diagnostic template hole `{name}`"));
                    out.push_str(value);
                    index = close + 1;
                    continue;
                }
            }
        }
        let character = template[index..]
            .chars()
            .next()
            .expect("template index is on a character boundary");
        out.push(character);
        index += character.len_utf8();
    }
    out
}

/// Keep the first ordinary prose word in sentence case while preserving the
/// case of a leading flag, identifier, ref, path, keyword, or code fragment.
/// Runtime-built diagnostic facts use this same product rule as table rows.
pub fn jet_sentence_case_line(input: &str) -> String {
    let Some((start, end)) = first_diagnostic_prose_token(input) else {
        return input.to_string();
    };
    let Some(first) = input[start..end].chars().next() else {
        return input.to_string();
    };
    if !first.is_ascii_uppercase() {
        return input.to_string();
    }
    let mut output = input.to_string();
    output.replace_range(start..start + first.len_utf8(), &first.to_ascii_lowercase().to_string());
    output
}

/// D-DIAG-URL1: every human-readable diagnostic ends with this stable lookup
/// line. The code is kept as data so custom project reports use the same
/// shape without duplicating the host string.
pub fn jet_diagnostic_more_line(code: &str) -> String {
    format!("More: jet-lang.dev/e/{code}")
}

fn first_diagnostic_prose_token(input: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    while offset < input.len() {
        let rest = &input[offset..];
        let ch = rest.chars().next()?;
        if ch.is_whitespace() || matches!(ch, '*' | '~' | '_') {
            offset += ch.len_utf8();
            continue;
        }
        if matches!(ch, '`' | '"' | '\'') {
            let after = &rest[ch.len_utf8()..];
            if let Some(close) = after.find(ch) {
                offset += ch.len_utf8() + close + ch.len_utf8();
                continue;
            }
        }
        if ch == '{' {
            if let Some(close) = rest.find('}') {
                offset += close + 1;
                continue;
            }
        }
        let start = offset;
        let mut end = 0;
        while end < rest.len() {
            let value = rest[end..]
                .chars()
                .next()
                .expect("diagnostic token index is on a character boundary");
            if end > 0
                && (value.is_whitespace()
                    || matches!(value, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '!'))
            {
                break;
            }
            if value == '.' {
                let next = rest[end + value.len_utf8()..].chars().next();
                if next.is_none_or(|next| {
                    next.is_whitespace()
                        || matches!(next, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '!')
                }) {
                    if end > 0 {
                        break;
                    }
                }
            }
            end += value.len_utf8();
        }
        let end = offset + end;
        let token = &input[start..end];
        if token.is_empty() {
            offset += ch.len_utf8();
            continue;
        }
        offset = end;
        if diagnostic_token_keeps_case(token) {
            continue;
        }
        return Some((start, end));
    }
    None
}

fn diagnostic_token_keeps_case(token: &str) -> bool {
    if DIAGNOSTIC_TYPE_NAMES.contains(&token)
        || matches!(token.chars().next(), Some('-' | '#' | '@'))
        || token == "C"
        || matches!(
            token,
            "App"
                | "Canvas"
                | "Cell"
                | "Codable"
                | "Core"
                | "Dart"
                | "Debug"
                | "Decimal"
                | "Display"
                | "Float"
                | "Hangar"
                | "Int"
                | "Jet"
                | "Jetpack"
                | "Nix"
                | "Output"
                | "Package"
                | "Quantity"
                | "Rscript"
                | "Rust"
                | "Runtime"
                | "Set"
                | "Source"
                | "Store"
                | "String"
                | "Syntax"
                | "Target"
                | "Tensor"
                | "Terminal"
                | "Type"
                | "Unit"
                | "Wasm"
                | "Web"
        )
    {
        return true;
    }
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let has_structural_case = token
        .chars()
        .any(|ch| matches!(ch, '_' | '/' | '\\' | '@' | '#' | '.'));
    let all_code = token
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'));
    let camel_case = token
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
        && token.chars().skip(1).any(|ch| ch.is_ascii_uppercase());
    has_digit || has_structural_case || all_code || camel_case || token.starts_with("C-")
}

// Keep this list dependency-free: Outcome.rs is embedded into standalone AOT
// and Web Preludes, where the host Syntax module does not exist.
const DIAGNOSTIC_TYPE_NAMES: &[&str] = &[
    "Bool",
    "Char",
    "Float",
    "Int",
    "String",
    "Unit",
    "Shared",
    "SharedGuard",
    "Shared.Weak",
    "Condition",
    "Task",
    "Receiver",
    "Sender",
    "TaskFailure",
    "HashMap",
    "BTreeMap",
    "Map",
    "Queue",
    "Set",
    "Rank",
    "PriorityQueue",
    "Cache",
    "Tally",
    "Bits",
    "Bytes",
    "I8",
    "I16",
    "I32",
    "I64",
    "U8",
    "U16",
    "U32",
    "U64",
    "F32",
    "F64",
];

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
    "this Id no longer refers to a live value — its pool slot was removed"
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

/// Shared wording for a reached typed goal.
pub fn jet_todo_message(file: &str, line: u32, expected_type: &str) -> String {
    format!("#Todo at {file}:{line} — expected {expected_type}")
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
// stop before the host stack aborts.
pub const JET_RUNTIME_STACK_LIMIT: usize = 6;

/// Whether a runtime row carries the rich source context frame.
pub fn jet_runtime_stop_has_context(code: &str) -> bool {
    matches!(code, "E3001" | "E3012")
}

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
    let Some(row) = row else {
        let what = format!("runtime diagnostic `{code}` is not an active runtime row");
        let why = "Jet could not resolve this stop through the active diagnostic registry";
        let fix = "report this as a Jet compiler or host defect";
        return JetRuntimeDiagnostic {
            code,
            source: "host",
            what: what.clone(),
            why: why.to_string(),
            fix: fix.to_string(),
            rendered: format!(
                "Internal error: {what}\n Why: {why}\n Fix: {fix}\n{}\n",
                jet_diagnostic_more_line(code)
            ),
            exit_code: 101,
        };
    };

    let line_text = line.to_string();
    let todo_type = message
        .rsplit_once(" — expected ")
        .map(|(_, expected)| expected)
        .unwrap_or(message);
    let holes = row
        .template_holes
        .iter()
        .map(|hole| {
            let value = match *hole {
                "msg" => message,
                "file" => file,
                "line" | "n" => &line_text,
                "fn" => fn_name,
                "type" => todo_type,
                _ => message,
            };
            (*hole, value)
        })
        .collect::<Vec<_>>();
    let (row_what, row_why, row_fix) = row.render(&holes);
    let what = if code == "E3005" {
        message.to_string()
    } else {
        row_what
    };
    let what = jet_sentence_case_line(&what);
    let why = jet_sentence_case_line(&row_why);
    let fix = jet_sentence_case_line(&row_fix);

    let show_context = jet_runtime_stop_has_context(code);
    let mut rendered = format!("Stop [{code}]: {what}\n");
    if !file.is_empty() {
        rendered.push_str(&format!(
            "  --> {}:{}{}\n",
            file,
            line,
            if !show_context || fn_name.is_empty() {
                String::new()
            } else {
                format!(" in {fn_name}")
            }
        ));
    }
    if show_context && !src_line.is_empty() {
        let line_s = line.to_string();
        let margin = line_s.len();
        let pad = " ".repeat(margin);
        rendered.push_str(&format!("   {pad}|\n"));
        rendered.push_str(&format!("{line_s} | {src_line}\n"));
        let col_offset = col.saturating_sub(1) as usize;
        let caret = "^".repeat(caret_len.max(1) as usize);
        rendered.push_str(&format!("   {pad}| {}{}\n", " ".repeat(col_offset), caret));
    }
    if show_context && !locals.is_empty() {
        rendered.push_str(&format!("locals: {locals}\n"));
    }
    rendered.push_str(&format!(" Why: {why}\n Fix: {fix}\n"));
    rendered.push_str(&jet_diagnostic_more_line(code));
    rendered.push('\n');

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

// JET_HOST_RUNTIME_STOP_BEGIN
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
    let row = crate::Registry::active_runtime_diagnostic(code).map(|row| JetRuntimeDiagnosticRow {
        code: row.code,
        what: row.what,
        why: row.why,
        fix: row.fix,
        template_holes: row.template_holes,
    });
    jet_render_runtime_stop_from_row(
        row, code, file, line, fn_name, src_line, col, caret_len, message, locals,
    )
}
// JET_HOST_RUNTIME_STOP_END

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
    // Keep the diagnostic wording intact without putting the Rust keyword in
    // the generated source. I1 scans generated tokens, not runtime prose.
    let gate = if gate.is_empty() {
        concat!("this un", "safe gate")
    } else {
        gate
    };
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
    let what = jet_sentence_case_line(&what);
    let why = jet_sentence_case_line(&why);
    let fix = jet_sentence_case_line(&fix);
    let mut rendered = format!("Runtime fault [{code}]: {what}\n");
    if !file.is_empty() {
        rendered.push_str(&format!(
            "  --> {file}:{line}, in #Unsafe gate {file}:{line}\n"
        ));
    }
    rendered.push_str(&format!(" Why: {why}\n Fix: {fix}\n"));
    rendered.push_str(&jet_diagnostic_more_line(code));
    rendered.push('\n');
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
