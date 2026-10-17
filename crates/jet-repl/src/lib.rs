//! E2-M18 — `jet repl`: interactive REPL session.
//!
//! Ratified decisions:
//!   D-EXEC1/D-EXEC2=A  every turn is built and executed by the normal
//!               `jet run` pipeline (`Executor`); no in-process evaluator,
//!               no fuel cap, no REPL-only feature subset
//!   D-REPL7=C   one accumulating module (default)
//!   D-REPL8=A   real move semantics across inputs
//!   D-REPL9=A   cooked-mode brace/paren/bracket balance → `...` prompt
//!   D-FE-REPL-MULTILINE1=A raw-mode parser-aware multiline editing
//!   D-REPL11=A  std-only; no external line-editing crate (I6)
//!   D-REPL15=B  meta-commands: `:quit`, `:reset`, `:load`, `:type`, `:help`
//!   D-REPL16=B  `x : T = v` echo for last expression; `;` suppresses
//!   D-REPL17=A  diagnostics byte-identical to batch compiler (I4)
//!   D-REPL-BANNER=A banner + `:help` hint on startup
//!   D-REPL-COLOR=A  respect the shared Jet color policy
//!   D-REPL-PRELOAD=A auto-import `core.term`; teaching note on first use
//!
//! Error codes: E1803 — a right the session's invocation policy did not
//! allow (default: never prompt; an interactive TTY may approve it).
//!
//! D-FE-REPL1=D (2026-07-08): hybrid REPL — the line REPL's mental model with
//! notebook/workspace layers one keystroke away. `lib.rs` keeps the session
//! model, input classification, sema plumbing, and the non-TTY floor
//! (`run_transcript`/`run_cooked`) byte-identical to the pre-redesign REPL.
//! The interactive-TTY rendering lives in sibling modules so its pieces are
//! unit-testable without a real pty:
//!   `Render`      — pure turn-gutter/pin-rail/fold-marker/bindings-pane text
//!   `RerunPlan`   — D-FE-REPL-RERUN1=A replay-plan semantics
//!   `Docs`        — D-FE-REPL-DOCS1=B `?name` lookup over the shared docs index
//!   `Terminal`    — raw-mode guard (`stty` shell-out, I6) + key decoding
//!   `Interactive` — the raw-mode event loop wiring the above together

#![allow(non_snake_case)]

// D-ARCH-SOURCE1=A: the REPL is a real outer seam over the compiler driver.
// Re-export inward compiler vocabulary so moved subsystem source keeps its
// established `crate::AST`/`crate::Comptime` paths without depending on root.
pub use jet_driver::{Comptime, Diagnostics, Lexer, Loader, Manifest, Parser, Sema, Syntax, AST};

pub mod SemanticSymbols;
pub mod Term;

use jet_foundation::Authority::{answer, parse_right, Holds, Verdict};
use jet_foundation::Terminal::{ColorChoice, Theme};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::Diagnostics::Diagnostic;
use crate::AST::{
    AccessConvention, CallArg, EnumDef, Expr, Func, ImportKind, Item, Stmt, StructDef, Type,
};

pub mod Executor;

pub mod Docs;
pub mod Console;
pub mod Sql;
pub mod ConsoleHost;
pub use Console::{
    ConsoleAuditEvent, ConsoleAuditOutcome, ConsoleCapability, ConsoleCommand, ConsoleDataBackend,
    ConsoleDataColumn, ConsoleDataMutation, ConsoleDataRequest, ConsoleDataResult,
    ConsoleDataStatement, ConsoleDataTransaction, ConsoleDataValue, ConsoleDatabaseHandle,
    ConsoleError, ConsoleGrant, ConsoleHistory, ConsoleHistoryEntry, ConsoleIdentity, ConsoleMode,
    ConsoleOptions, ConsoleOutput, ConsoleOutputKind, ConsoleProject, ConsoleRequest,
    ConsoleResponse, ConsoleServiceHandle, ConsoleSession, InProcessRouter,
};
pub use ConsoleHost::{open_attached, run_attached, ConsoleHostAdapters};
pub use Sql::{SqlEval, SqlSession};
mod History;
mod Interactive;
pub mod Notebook;
pub mod Render;
pub mod RerunPlan;
// `Terminal` lives in this seam's shared `Term` module; root Help/dev callers
// consume the same mechanism through the seam (I8).
// every existing `Terminal::…` reference in this module + `Interactive.rs`
// unchanged.
use crate::Term as Terminal;

#[derive(Clone, Debug, Default)]
pub struct ReplFlags {
    pub allow: HashSet<String>,
    pub deny: HashSet<String>,
    pub color: ColorChoice,
    /// #2038: a file named on the command line (`jet repl <file>.jet`), loaded
    /// into the session once it exists. The CLI has already proven the path is
    /// there; loading itself goes through `cmd_load`, the one `:load`
    /// mechanism (D-REPL15=B), with `run_entry: false`.
    pub preload: Option<String>,
}

impl ReplFlags {
    pub fn new(allow: &[String], deny: &[String]) -> Self {
        Self {
            allow: allow.iter().map(|s| canonical_flag(s)).collect(),
            deny: deny.iter().map(|s| canonical_flag(s)).collect(),
            color: ColorChoice::Auto,
            preload: None,
        }
    }

    pub fn with_color(mut self, color: ColorChoice) -> Self {
        self.color = color;
        self
    }

    pub fn with_preload(mut self, file: String) -> Self {
        self.preload = Some(file);
        self
    }
}

fn canonical_flag(value: &str) -> String {
    parse_right(value).unwrap_or_else(|| value.to_ascii_lowercase())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptChoice {
    Once,
    Session,
    Deny,
    Continue,
    Revoke,
}

/// One right a turn needs that invocation policy left undecided, as the
/// interactive approval prompt shows it (`FS.Write` → root `FS`, operation
/// `Write`, resource: the turn).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ReplEffectRequest {
    pub root: String,
    pub operation: String,
    pub resource: String,
}

impl ReplEffectRequest {
    fn for_right(right: &str, input: &str) -> Self {
        let (root, operation) = right.split_once('.').unwrap_or((right, "Use"));
        Self {
            root: root.to_string(),
            operation: operation.to_string(),
            resource: input.to_string(),
        }
    }
}

pub(crate) trait EffectPrompt {
    fn choose(&mut self, request: &ReplEffectRequest, reused: bool) -> PromptChoice;
}

/// Invocation authority of one REPL/notebook/eval session. Turns run in a
/// child `jet` (`Executor`), whose application-authority check is the one
/// per-operation gate: rights this policy holds are forwarded as `--allow`;
/// a right it leaves undecided is refused with E1803 unless an interactive
/// prompt approves it (default: no prompt).
pub(crate) struct ReplPolicy {
    flags: ReplFlags,
    session: HashSet<String>,
    input: VecDeque<String>,
    /// Input lines earlier accepted turns were given; replay feeds them again
    /// so rebuilt state reads the same values.
    consumed_input: String,
}

impl ReplPolicy {
    pub(crate) fn new(flags: ReplFlags, _root: &Path) -> Self {
        Self {
            flags,
            session: HashSet::new(),
            input: VecDeque::new(),
            consumed_input: String::new(),
        }
    }

    pub(crate) fn for_notebook(flags: ReplFlags, root: &Path) -> Self {
        Self::new(flags, root)
    }

    pub(crate) fn push_input(&mut self, line: String) {
        self.input.push_back(line);
    }

    pub(crate) fn pending_input(&self) -> usize {
        self.input.len()
    }

    pub(crate) fn authorizer<'a>(
        &'a mut self,
        prompt: Option<&'a mut dyn EffectPrompt>,
    ) -> ReplAuthorization<'a> {
        ReplAuthorization {
            policy: self,
            prompt,
        }
    }
}

pub(crate) struct ReplAuthorization<'a> {
    policy: &'a mut ReplPolicy,
    prompt: Option<&'a mut dyn EffectPrompt>,
}

impl ReplAuthorization<'_> {
    fn e1803(&self, right: &str, why: &str) -> Diagnostic {
        let authority = jet_foundation::Authority::ApplicationAuthority {
            required_effects: Holds::from([right.to_string()]),
            granted_effects: self.policy.flags.allow.iter().cloned().collect(),
            denied_effects: self.policy.flags.deny.iter().cloned().collect(),
            authority: "jet repl invocation policy".to_string(),
        };
        Diagnostic::error(
            "E1803",
            format!("{right} was denied"),
            jet_foundation::Authority::repl_refusal_why(&authority, why),
            format!("approve this exact operation interactively, or restart with `jet repl --allow={right}`"),
            None,
        )
        .with_detail(authority.policy_facts())
    }

    /// The rights a child run holds: invocation allows, session approvals,
    /// and this turn's one-time approvals; invocation denials always win.
    fn rights(&self, once: &[String]) -> Executor::Policy {
        let mut allow: Vec<String> = self.policy.flags.allow.iter().cloned().collect();
        allow.extend(self.policy.session.iter().cloned());
        allow.extend(once.iter().cloned());
        allow.sort();
        allow.dedup();
        let mut deny: Vec<String> = self.policy.flags.deny.iter().cloned().collect();
        deny.sort();
        Executor::Policy { allow, deny }
    }

    /// Decide every right the child left undecided: approved rights come
    /// back for a re-run; the first refusal is E1803.
    fn decide(&mut self, undecided: &[String], input: &str) -> Result<Vec<String>, Diagnostic> {
        let mut approved = Vec::new();
        for right in undecided {
            let deny: Holds = self.policy.flags.deny.iter().cloned().collect();
            if answer(&Holds::new(), &deny, right) == Verdict::Denied {
                return Err(self.e1803(
                    right,
                    "an explicit `--deny` policy overrides every prompt and allowance",
                ));
            }
            let request = ReplEffectRequest::for_right(right, input);
            let Some(prompt) = self.prompt.as_deref_mut() else {
                return Err(self.e1803(
                    right,
                    "non-interactive sessions never prompt and no matching `--allow` right was supplied",
                ));
            };
            match prompt.choose(&request, false) {
                PromptChoice::Once | PromptChoice::Continue => approved.push(right.clone()),
                PromptChoice::Session => {
                    self.policy.session.insert(right.clone());
                    approved.push(right.clone());
                }
                PromptChoice::Deny | PromptChoice::Revoke => {
                    return Err(self.e1803(right, "interactive authority was denied"));
                }
            }
        }
        Ok(approved)
    }

    pub(crate) fn reset_session(&mut self) {
        self.policy.session.clear();
        self.policy.consumed_input.clear();
    }
}

/// Effect markers used to classify a Stmts turn as "effectful" for
/// D-FE-REPL-RERUN1=A replay gating (`ReplTurn::had_effect`). Textual, not a
/// full purity analysis — deliberately conservative (a false positive just
/// asks for one extra confirmation; a false negative would silently replay
/// a side effect, which the ratified design forbids).
const EFFECT_MARKERS: &[&str] = &[
    "print(",
    "eprint(",
    "input(",
    ".write(",
    ".append(",
    ".delete(",
    ".remove_file(",
    "fs.write",
    "fs.append",
    "fs.delete",
    "fs.remove",
    "io.print",
    "io.eprint",
    "io.read_line",
    "io.write",
    ".send(",
    ".emit(",
    ".emit_async(",
];

/// D-FE-REPL-RERUN1=A: whether `input` looks like it produced (or would
/// produce) an observable side effect, so a rerun of it must pause for
/// confirmation instead of auto-replaying.
pub(crate) fn looks_effectful(input: &str) -> bool {
    EFFECT_MARKERS.iter().any(|m| input.contains(m))
}

const TERMINAL_SHIFT_IN: &str = "\x0f";

fn strip_terminal_control_bytes(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || matches!(*c, '\t' | '\n' | '\r'))
        .collect()
}

fn normalize_repl_input(input: &str) -> String {
    strip_terminal_control_bytes(input.trim_end_matches('\n').trim_end_matches('\r'))
}

// ── color helpers ──────────────────────────────────────────────────────────

/// Return true if color output is desired on stdout.
/// Respects the shared `NO_COLOR` / `FORCE_COLOR` policy.
pub fn color_on() -> bool {
    use std::io::IsTerminal;
    ColorChoice::Auto.resolve(io::stdout().is_terminal())
}

pub(crate) fn bold(s: &str, color: bool) -> String {
    Theme::new(color).bold(s)
}

pub(crate) fn dim(s: &str, color: bool) -> String {
    Theme::new(color).dim(s)
}

fn green(s: &str, color: bool) -> String {
    Theme::new(color).success(s)
}

fn yellow(s: &str, color: bool) -> String {
    Theme::new(color).warn(s)
}

// ── session state ─────────────────────────────────────────────────────────

/// A single accumulated REPL session. All state is in memory (REPL-I4/I5).
///
/// Session stores item declarations and accepted statements as source text.
/// Every turn is checked in-process and then built and executed by the
/// `Executor`, which replays the accepted statements to rebuild live values
/// (D-EXEC2). `scope` holds each live binding's last rendered value.
pub struct Session {
    /// Raw source text for each accumulated top-level item declaration.
    /// Re-parsed on each sema check step so the full item list is visible.
    pub item_srcs: Vec<String>,
    /// D-REPL10/S16: accumulated `use …;` import lines. Prepended to every
    /// synthetic program so an import typed in one input (e.g.
    /// `use core.math as math`) keeps resolving in later inputs.
    pub import_srcs: Vec<String>,
    /// Exact statement ASTs accepted in prior turns. Sema rechecks this typed
    /// history directly, preserving declarations, assignments, and moves.
    pub sema_stmts: Vec<Stmt>,
    /// The interpreter's function table: all `fn` items by name.
    /// Rebuilt when a new function is added.
    pub func_defs: HashMap<String, Func>,
    /// Session struct/enum defs (rebuilt with `func_defs`). The executor's
    /// type-directed JSON renderer reads their fields and variants.
    pub struct_defs: HashMap<String, StructDef>,
    pub enum_defs: HashMap<String, EnumDef>,
    /// Live bindings (D-REPL7): name → the built program's Display text of
    /// its value after the last turn that reported it.
    pub scope: HashMap<String, String>,
    /// Checked Jet types of the live bindings (declared or inferred).
    pub binding_types: HashMap<String, crate::AST::Type>,
    /// Names bound with `:=` (mutable). Everything else in `scope` was bound
    /// with `::`. Drives the bare `name := value` / `name :: value` line shape (D-BIND-BARE1).
    pub mutable_names: HashSet<String>,
    /// Whether the teaching note for `print` (D-REPL-PRELOAD) has been shown.
    pub shown_preload_note: bool,
    /// Input counter — used in synthetic spans (diagnostics say `<repl:N>`).
    pub step: usize,
    /// D-REPL8=A: bindings moved/consumed in a prior input. Excluded from
    /// binding stubs so sema sees them as undefined in later inputs.
    pub moved_names: HashSet<String>,
    /// Accepted statement turns in materialized form; the executor replays
    /// them ahead of every later turn and `:run` runs them as one program.
    pub stmt_srcs: Vec<String>,
    /// D-CTCORE1: alias → Core module path (e.g. `"math"` → `"core.math"`),
    /// derived from successfully accepted `use` declarations; docs and
    /// completion resolve module members through it.
    pub core_imports: HashMap<String, String>,
    /// Per-session token for compiler-owned names in materialized turns.
    pub(crate) nonce: String,
    /// Source/project state loaded before turn 1. Replay resets to this
    /// baseline, never to an empty synthetic module.
    baseline_item_srcs: Vec<String>,
    baseline_import_srcs: Vec<String>,
    baseline_func_defs: HashMap<String, Func>,
    baseline_struct_defs: HashMap<String, StructDef>,
    baseline_enum_defs: HashMap<String, EnumDef>,
    baseline_core_imports: HashMap<String, String>,
    /// Notebook controls state: every accepted input gets an addressable turn.
    pub turns: Vec<ReplTurn>,
    /// D-FE-REPL-HISTORY1=A: successful submissions from this and prior
    /// sessions. Persistence is enabled only by the real REPL entry point;
    /// deterministic transcript helpers stay session-only.
    history: History::History,
}

#[derive(Clone, Debug)]
pub struct ReplTurn {
    pub id: usize,
    pub input: String,
    pub summary: String,
    pub status: ReplTurnStatus,
    pub folded: bool,
    pub pinned: bool,
    /// Skipped during a downstream replay. Stale turns are retained for
    /// notebook truth but did not contribute to the rebuilt live session.
    pub stale: bool,
    /// D-FE-REPL-RERUN1=A: whether this turn produced an observable side
    /// effect (print/write/…) — gates replay-plan auto vs confirm-effect.
    pub had_effect: bool,
    /// If this turn introduced exactly one new session binding, its name —
    /// used by the pin rail (`📌 name : Type = value`) to show the binding's
    /// live current value rather than a frozen turn summary.
    pub bound_name: Option<String>,
    /// D-FE-REPL1=D: the full echoed text an interactive auto-fold elided
    /// (`⋯ N rows folded …`), if any — `:unfold`/`^F`/Enter-on-the-marker
    /// print this rather than leaving unfold a no-op.
    pub pending_unfold: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplTurnStatus {
    Ok,
    Error,
    Interrupted,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Session {
            item_srcs: Vec::new(),
            import_srcs: Vec::new(),
            sema_stmts: Vec::new(),
            func_defs: HashMap::new(),
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            scope: HashMap::new(),
            binding_types: HashMap::new(),
            mutable_names: HashSet::new(),
            shown_preload_note: false,
            step: 0,
            moved_names: HashSet::new(),
            stmt_srcs: Vec::new(),
            core_imports: HashMap::new(),
            nonce: Executor::session_nonce(),
            baseline_item_srcs: Vec::new(),
            baseline_import_srcs: Vec::new(),
            baseline_func_defs: HashMap::new(),
            baseline_struct_defs: HashMap::new(),
            baseline_enum_defs: HashMap::new(),
            baseline_core_imports: HashMap::new(),
            turns: Vec::new(),
            history: History::History::session_only(),
        }
    }

    fn enable_persistent_history(&mut self) {
        let (history, warning) = History::History::open_from_env();
        self.history = history;
        if let Some(warning) = warning {
            eprintln!("{warning}");
        }
    }

    pub fn reset(&mut self) {
        self.item_srcs = self.baseline_item_srcs.clone();
        self.import_srcs = self.baseline_import_srcs.clone();
        self.sema_stmts.clear();
        self.func_defs = self.baseline_func_defs.clone();
        self.struct_defs = self.baseline_struct_defs.clone();
        self.enum_defs = self.baseline_enum_defs.clone();
        self.scope.clear();
        self.binding_types.clear();
        self.mutable_names.clear();
        self.step = 0;
        self.moved_names.clear();
        self.stmt_srcs.clear();
        self.core_imports = self.baseline_core_imports.clone();
        self.turns.clear();
        // Keep shown_preload_note — no need to repeat the teaching note.
    }

    fn preserve_project_baseline(&mut self) {
        self.baseline_item_srcs = self.item_srcs.clone();
        self.baseline_import_srcs = self.import_srcs.clone();
        self.baseline_func_defs = self.func_defs.clone();
        self.baseline_struct_defs = self.struct_defs.clone();
        self.baseline_enum_defs = self.enum_defs.clone();
        self.baseline_core_imports = self.core_imports.clone();
    }

    fn record_turn(&mut self, input: &str, status: ReplTurnStatus, summary: String) {
        self.record_turn_ex(input, status, summary, false, None);
    }

    /// Full turn recorder: also stamps `had_effect` (D-FE-REPL-RERUN1=A replay
    /// gating) and `bound_name` (pin-rail live-value lookup) when known.
    fn record_turn_ex(
        &mut self,
        input: &str,
        status: ReplTurnStatus,
        summary: String,
        had_effect: bool,
        bound_name: Option<String>,
    ) {
        let id = self.turns.len() + 1;
        self.turns.push(ReplTurn {
            id,
            input: input.to_string(),
            summary: turn_summary(&summary),
            status,
            folded: summary.lines().count() > 6 || summary.len() > 240,
            pinned: false,
            stale: false,
            had_effect,
            bound_name,
            pending_unfold: None,
        });
    }

    fn remember_success(&mut self, input: &str) {
        if let Some(warning) = self.history.record(input) {
            eprintln!("{warning}");
        }
    }

    /// Build the accumulated item declarations source text (functions, structs…).
    /// This is inserted at program top-level, so value bindings are NOT here.
    pub(crate) fn accumulated_src(&self) -> String {
        self.item_srcs.join("\n")
    }

    /// Notebook declarations are file-wide even when cells execute out of order.
    pub(crate) fn replace_notebook_items(&mut self, item_srcs: Vec<String>) {
        if self.item_srcs != item_srcs {
            self.item_srcs = item_srcs;
            rebuild_funcs(self);
        }
    }

    /// Accumulated `use …;` import lines, one per line, with a trailing newline
    /// when non-empty so they sit cleanly above the items in a synthetic program.
    pub(crate) fn import_src(&self) -> String {
        if self.import_srcs.is_empty() {
            String::new()
        } else {
            format!("{}\n", self.import_srcs.join("\n"))
        }
    }

    /// Register the exact binding declaration for later sema turns.
    pub fn record_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            if let Stmt::Val(binding) = stmt {
                if binding.name == "__repl_echo__" {
                    continue;
                }
                if binding.mutable {
                    self.mutable_names.insert(binding.name.clone());
                } else {
                    self.mutable_names.remove(&binding.name);
                }
                if let Some(ty) = &binding.ty {
                    self.binding_types.insert(binding.name.clone(), ty.clone());
                }
            }
            self.sema_stmts.push(stmt.clone());
        }
    }
}

fn turn_summary(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "ok".to_string()
    } else {
        trimmed
            .lines()
            .next()
            .unwrap_or("ok")
            .chars()
            .take(96)
            .collect()
    }
}

fn parse_turn_id(arg: Option<&str>) -> Result<usize, String> {
    let Some(arg) = arg else {
        return Err("usage: turn command needs a turn id".to_string());
    };
    let id = arg
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("`{arg}` is not a turn id"))?;
    if id == 0 {
        Err("turn ids start at 1".to_string())
    } else {
        Ok(id)
    }
}

fn status_word(status: ReplTurnStatus) -> &'static str {
    match status {
        ReplTurnStatus::Ok => "ok",
        ReplTurnStatus::Error => "error",
        ReplTurnStatus::Interrupted => "interrupted",
    }
}

fn render_turns(session: &Session, color: bool) {
    if !color {
        print!("{}", render_turns_text(session));
        return;
    }
    if session.turns.is_empty() {
        println!("No turns yet");
        return;
    }
    for turn in &session.turns {
        let pin = if turn.pinned { " pinned" } else { "" };
        let fold = if turn.folded { " folded" } else { "" };
        let stale = if turn.stale { " stale" } else { "" };
        println!(
            "{} {}{}{}{}  {}",
            bold(&format!("#{}", turn.id), color),
            status_word(turn.status),
            pin,
            fold,
            stale,
            turn.summary
        );
    }
}

fn render_turns_text(session: &Session) -> String {
    if session.turns.is_empty() {
        return "No turns yet\n".to_string();
    }
    let mut out = String::new();
    for turn in &session.turns {
        let pin = if turn.pinned { " pinned" } else { "" };
        let fold = if turn.folded { " folded" } else { "" };
        let stale = if turn.stale { " stale" } else { "" };
        out.push_str(&format!(
            "#{} {}{}{}{}  {}\n",
            turn.id,
            status_word(turn.status),
            pin,
            fold,
            stale,
            turn.summary
        ));
    }
    out
}

/// Unfold turn `id`: clears its folded flag and returns any full text an
/// interactive auto-fold elided (`ReplTurn::pending_unfold`), if present.
/// Shared by `handle_meta`'s `:unfold` arm and `Interactive`'s `^F`/Enter
/// handling so both paths show the same stashed content.
pub(crate) fn unfold_turn(session: &mut Session, id: usize) -> Result<Option<String>, String> {
    let Some(turn) = session.turns.iter_mut().find(|t| t.id == id) else {
        return Err(format!("turn #{id} does not exist"));
    };
    turn.folded = false;
    Ok(turn.pending_unfold.take())
}

fn set_turn_flag(session: &mut Session, id: usize, flag: &str, value: bool) -> Result<(), String> {
    let Some(turn) = session.turns.iter_mut().find(|t| t.id == id) else {
        return Err(format!("turn #{id} does not exist"));
    };
    match flag {
        "folded" => turn.folded = value,
        "pinned" => turn.pinned = value,
        _ => {}
    }
    Ok(())
}

// ── move detection (D-REPL8=A) ─────────────────────────────────────────────

/// Scan the successfully-parsed stmts for names that were moved from the
/// session's current bindings. A move occurs when:
///
/// 1. `t :: s` — the init is a bare identifier that names a
///    session binding of non-scalar type. Sema's `note_move_if_direct_ident`
///    marks non-scalar bindings moved at this point.
/// 2. A call argument uses `take name` convention — `CallArg::convention == Move`
///    with an `Ident` expr naming a session binding.
///
/// `types` (the checked binding types) decides whether a binding is scalar
/// (Int/Float/Bool/Char and fixed-width numbers) — scalars are copied, not moved.
///
/// We only scan depth-1 (the stmt list passed to the REPL step). Nested moves
/// inside `fn` bodies are not relevant — they stay within the item's own scope.
fn collect_moved_names(
    stmts: &[Stmt],
    session_bindings: &HashSet<String>,
    scope: &HashMap<String, Type>,
) -> HashSet<String> {
    let mut moved = HashSet::new();
    for stmt in stmts {
        collect_moved_in_stmt(stmt, session_bindings, scope, &mut moved);
    }
    moved
}

/// Return true if a checked type is a scalar (copy semantics, not moved).
fn type_is_scalar(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Float32 | Type::Bool | Type::Char => true,
        Type::IntN { .. } | Type::InlineRange { .. } => true,
        Type::Tagged { inner, .. } => type_is_scalar(inner),
        _ => false,
    }
}

fn collect_moved_in_stmt(
    stmt: &Stmt,
    session_bindings: &HashSet<String>,
    scope: &HashMap<String, Type>,
    moved: &mut HashSet<String>,
) {
    match stmt {
        Stmt::Val(b) => {
            // `t :: s` — if init is a bare Ident naming a non-scalar session
            // binding, that's a move (sema marks it via note_move_if_direct_ident).
            if let Expr::Ident(name, _) = &b.init {
                if session_bindings.contains(name.as_str()) {
                    // Only non-scalars are moved.
                    let is_scalar = scope
                        .get(name.as_str())
                        .map(type_is_scalar)
                        .unwrap_or(false);
                    if !is_scalar {
                        moved.insert(name.clone());
                    }
                }
            } else {
                collect_moved_in_expr(&b.init, session_bindings, scope, moved);
            }
        }
        Stmt::Assign { value, .. } => {
            collect_moved_in_expr(value, session_bindings, scope, moved);
        }
        Stmt::Expr(e) | Stmt::DeferClose { close: e, .. } => {
            collect_moved_in_expr(e, session_bindings, scope, moved);
        }
        Stmt::Return(Some(e), _) => {
            collect_moved_in_expr(e, session_bindings, scope, moved);
        }
        _ => {}
    }
}

fn collect_moved_in_expr(
    expr: &Expr,
    session_bindings: &HashSet<String>,
    scope: &HashMap<String, Type>,
    moved: &mut HashSet<String>,
) {
    match expr {
        Expr::Call(call) => {
            for arg in &call.args {
                collect_moved_in_callarg(arg, session_bindings, scope, moved);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_moved_in_expr(receiver, session_bindings, scope, moved);
            for arg in args {
                collect_moved_in_callarg(arg, session_bindings, scope, moved);
            }
        }
        // Don't recurse into nested lambdas/closures — they capture, not move
        // session bindings (those would be in their own `take_names`).
        _ => {}
    }
}

fn collect_moved_in_callarg(
    arg: &CallArg,
    session_bindings: &HashSet<String>,
    scope: &HashMap<String, Type>,
    moved: &mut HashSet<String>,
) {
    if arg.convention == AccessConvention::Move {
        if let Expr::Ident(name, _) = &arg.expr {
            if session_bindings.contains(name.as_str()) {
                let is_scalar = scope
                    .get(name.as_str())
                    .map(type_is_scalar)
                    .unwrap_or(false);
                if !is_scalar {
                    moved.insert(name.clone());
                }
            }
        }
    }
}

/// A process-and-counter-unique temp file name so concurrent REPL sessions
/// (e.g. parallel transcript tests) don't clobber each other's temp files.
pub(crate) fn unique_temp_name(tag: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!("__jet_repl_{}_{}_{}.jet", tag, std::process::id(), n)
}

fn write_unique_temp_file(tag: &str, source: &str) -> std::io::Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(unique_temp_name(tag));
    write_temp_file_at(&path, source)?;
    Ok(path)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const REPL_O_NOFOLLOW: i32 = 0o400000;
#[cfg(any(target_os = "macos", target_os = "ios"))]
const REPL_O_NOFOLLOW: i32 = 0x0100;

fn write_temp_file_at(path: &std::path::Path, source: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Refuse a final symlink substituted between the name checks and open.
            options.custom_flags(REPL_O_NOFOLLOW);
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "this Unix target cannot enforce no-follow temporary-file creation",
            ));
        }
    }
    let mut file = options.open(&path)?;
    if let Err(error) = file.write_all(source.as_bytes()) {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

/// What one checked statement turn did once the executor ran it.
pub(crate) enum TurnResult {
    Done {
        stdout: String,
        stderr: String,
        echo: Option<Executor::TurnValue>,
        echo_ty: Option<Type>,
        bound_name: Option<String>,
        had_effect: bool,
    },
    /// Check, build, authority, or runtime failure. `report` is the runtime
    /// stop text the program printed; `diagnostics` the refusals before it ran.
    Failed {
        stdout: String,
        stderr: String,
        diagnostics: Vec<Diagnostic>,
        report: Option<String>,
    },
    Interrupted,
}

const ECHO_SENTINEL: &str = "__repl_echo__ :: ";

/// The one statement-turn mechanism shared by the cooked and interactive
/// loops, transcripts, notebook cells, Console, Sql and `jet eval`: check the
/// turn against the session, materialize it after the replayed history,
/// build and execute it (`Executor`), resolve undecided rights through the
/// session's authorization, and commit the turn's bindings only on success.
pub(crate) fn run_stmts_turn(
    session: &mut Session,
    stmts: &[Stmt],
    suppress: bool,
    check_src: &str,
    trimmed: &str,
    base_dir: &Path,
    authorizer: &mut ReplAuthorization<'_>,
    json: bool,
) -> TurnResult {
    let input_start = stmts
        .first()
        .map(|stmt| stmt.span().start)
        .unwrap_or(0)
        .saturating_add(check_src.find(trimmed).unwrap_or(0));
    let checked_stmts = match type_check_stmts(session, stmts, session.step, check_src) {
        Ok((checked, _)) => checked,
        Err(mut errors) => {
            for diagnostic in &mut errors {
                reanchor_repl_diagnostic(diagnostic, input_start);
            }
            return TurnResult::Failed {
                stdout: String::new(),
                stderr: String::new(),
                diagnostics: errors,
                report: None,
            };
        }
    };
    let echo_name = Executor::echo_name(&session.nonce, session.step);
    let (statements, echo_ty) = match check_src.strip_prefix(ECHO_SENTINEL) {
        Some(expr) => {
            let ty = checked_stmts.iter().rev().find_map(|stmt| match stmt {
                Stmt::Val(binding) if binding.name == "__repl_echo__" => binding.ty.clone(),
                _ => None,
            });
            (format!("{echo_name} :: {expr}"), ty)
        }
        None => (
            check_src.trim_end().trim_end_matches(';').to_string(),
            None,
        ),
    };
    let echo = check_src.starts_with(ECHO_SENTINEL)
        && !suppress
        && !matches!(echo_ty, Some(Type::Named(ref name)) if name == crate::Syntax::INTERNAL_UNIT_TYPE);

    let session_names: HashSet<String> = session.scope.keys().cloned().collect();
    let newly_moved = collect_moved_names(stmts, &session_names, &session.binding_types);
    let mut new_bindings: Vec<(String, Option<Type>)> = Vec::new();
    for stmt in &checked_stmts {
        if let Stmt::Val(binding) = stmt {
            if binding.name != "__repl_echo__" && !newly_moved.contains(&binding.name) {
                new_bindings.retain(|(name, _)| name != &binding.name);
                new_bindings.push((binding.name.clone(), binding.ty.clone()));
            }
        }
    }
    let mut live_mutables: Vec<(String, Option<Type>)> = session
        .mutable_names
        .iter()
        .filter(|name| session.scope.contains_key(*name))
        .filter(|name| !newly_moved.contains(*name))
        .filter(|name| !new_bindings.iter().any(|(bound, _)| bound == *name))
        .map(|name| (name.clone(), session.binding_types.get(name).cloned()))
        .collect();
    live_mutables.sort_by(|left, right| left.0.cmp(&right.0));
    let mut reported: Vec<(String, Option<Type>)> = Vec::new();
    if echo {
        reported.push((echo_name.clone(), echo_ty.clone()));
    }
    reported.extend(new_bindings.iter().cloned());
    reported.extend(live_mutables);
    let reports: Vec<Executor::ReportedValue<'_>> = reported
        .iter()
        .map(|(name, ty)| Executor::ReportedValue {
            name,
            ty: ty.as_ref(),
        })
        .collect();

    let imports = session.import_src();
    let items = session.accumulated_src();
    let input = format!(
        "{}{}",
        authorizer.policy.consumed_input,
        authorizer
            .policy
            .input
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    );
    let mut once: Vec<String> = Vec::new();
    let outcome = loop {
        let program = Executor::TurnProgram {
            nonce: &session.nonce,
            imports: &imports,
            items: &items,
            replay: &session.stmt_srcs,
            statements: &statements,
            reports: &reports,
            json,
            types: Executor::TypeCatalog {
                structs: &session.struct_defs,
                enums: &session.enum_defs,
            },
            input: &input,
        };
        let outcome = match Executor::run_turn(&program, &authorizer.rights(&once), base_dir) {
            Ok(outcome) => outcome,
            Err(error) => {
                return TurnResult::Failed {
                    stdout: String::new(),
                    stderr: String::new(),
                    diagnostics: vec![Diagnostic::error(
                        "E2105",
                        format!("couldn't start the build for this turn: {error}"),
                        "every REPL turn is built and run by a child `jet`".to_string(),
                        "check that the `jet` executable is still in place".to_string(),
                        None,
                    )],
                    report: None,
                };
            }
        };
        let Some(Executor::TurnFailure::Refused { undecided, .. }) = &outcome.failure else {
            break outcome;
        };
        let fresh: Vec<String> = undecided
            .iter()
            .filter(|right| !once.contains(right))
            .cloned()
            .collect();
        if fresh.is_empty() {
            break outcome;
        }
        match authorizer.decide(&fresh, trimmed) {
            Ok(approved) if !approved.is_empty() => once.extend(approved),
            Ok(_) => break outcome,
            Err(mut refusal) => {
                reanchor_repl_diagnostic(&mut refusal, input_start);
                return TurnResult::Failed {
                    stdout: String::new(),
                    stderr: String::new(),
                    diagnostics: vec![refusal],
                    report: None,
                };
            }
        }
    };

    let Executor::TurnOutcome {
        stdout,
        stderr,
        values,
        failure,
    } = outcome;
    match failure {
        Some(Executor::TurnFailure::Interrupted) => return TurnResult::Interrupted,
        Some(Executor::TurnFailure::Refused { diagnostics, .. }) => {
            return TurnResult::Failed {
                stdout,
                stderr,
                diagnostics,
                report: None,
            };
        }
        Some(Executor::TurnFailure::Stopped { report }) => {
            return TurnResult::Failed {
                stdout,
                stderr,
                diagnostics: Vec::new(),
                report: Some(report),
            };
        }
        None => {}
    }

    // Commit: the turn joins the replayed history and its values the scope.
    authorizer.policy.consumed_input = input;
    authorizer.policy.input.clear();
    session.stmt_srcs.push(statements);
    session.record_stmts(&checked_stmts);
    let mut echo_value = None;
    for value in values {
        if value.name == echo_name {
            echo_value = Some(value);
        } else {
            session.scope.insert(value.name, value.shown);
        }
    }
    for name in &newly_moved {
        session.scope.remove(name);
    }
    session.moved_names.extend(newly_moved);
    let bound_name = match new_bindings.as_slice() {
        [(only, _)] => Some(only.clone()),
        _ => None,
    };
    let had_effect =
        !stdout.is_empty() || !stderr.is_empty() || !once.is_empty() || looks_effectful(trimmed);
    TurnResult::Done {
        stdout,
        stderr,
        echo: echo_value,
        echo_ty,
        bound_name,
        had_effect,
    }
}

/// `x : T` echo text (D-REPL16=B): strings are shown quoted.
pub(crate) fn echo_text(value: &Executor::TurnValue) -> String {
    let ty = value.ty.as_deref().unwrap_or("?");
    if ty == "String" {
        format!("\"{}\" : {}", value.shown, ty)
    } else {
        format!("{} : {}", value.shown, ty)
    }
}

/// One-line turn summary of a failure (`:turns`).
pub(crate) fn turn_failure_summary(diagnostics: &[Diagnostic], report: Option<&str>) -> String {
    let mut parts: Vec<String> = diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code, d.what))
        .collect();
    if let Some(first) = report.and_then(|report| report.lines().find(|line| !line.trim().is_empty())) {
        parts.push(first.trim().to_string());
    }
    parts.join("; ")
}

/// Plain-text rendering of a failed turn for transcript-style callers.
pub(crate) fn failure_text(diagnostics: &[Diagnostic], report: Option<&str>) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        out.push_str(&format!("error [{}]: {}\n", diagnostic.code, diagnostic.what));
    }
    if let Some(report) = report {
        out.push_str(report);
        if !report.is_empty() && !report.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

// ── :run ───────────────────────────────────────────────────────────────────

/// Run `statements` once after the session's accepted history as one built
/// program (no value records); used by `:run` and by `:load` of a program
/// whose `fn run` should execute.
fn run_session_program(
    session: &Session,
    statements: &str,
    base_dir: &Path,
    authorizer: &mut ReplAuthorization<'_>,
) -> String {
    let imports = session.import_src();
    let items = session.accumulated_src();
    let input = authorizer.policy.consumed_input.clone();
    let program = Executor::TurnProgram {
        nonce: &session.nonce,
        imports: &imports,
        items: &items,
        replay: &[],
        statements,
        reports: &[],
        json: false,
        types: Executor::TypeCatalog {
            structs: &session.struct_defs,
            enums: &session.enum_defs,
        },
        input: &input,
    };
    match Executor::run_turn(&program, &authorizer.rights(&[]), base_dir) {
        Ok(outcome) => {
            let mut out = format!("{}{}", outcome.stdout, outcome.stderr);
            match outcome.failure {
                None => {}
                Some(Executor::TurnFailure::Interrupted) => {
                    out.push_str("Interrupted. External effects already performed were not rolled back.\n")
                }
                Some(Executor::TurnFailure::Refused { diagnostics, .. }) => {
                    out.push_str(&failure_text(&diagnostics, None))
                }
                Some(Executor::TurnFailure::Stopped { report }) => {
                    out.push_str(&failure_text(&[], Some(&report)))
                }
            }
            out
        }
        Err(error) => format!("error: couldn't invoke jet run: {error}\n"),
    }
}

/// `:run` — the whole session (accepted statements, or the loaded program's
/// own `fn run`) as one built program.
fn cmd_run(session: &Session, base_dir: &Path, authorizer: &mut ReplAuthorization<'_>) -> String {
    if session.stmt_srcs.is_empty() && session.item_srcs.is_empty() {
        return "note: session is empty — nothing to run\n".to_string();
    }
    let statements = if session.func_defs.contains_key("run") {
        "run()".to_string()
    } else {
        session.stmt_srcs.join("\n")
    };
    run_session_program(session, &statements, base_dir, authorizer)
}

// ── --project manifest loading (D-REPL10) ──────────────────────────────────

/// Load a project's source files into the session as accumulated items.
/// Scans `project_dir/src/*.jet` and `project_dir/*.jet` (excluding `pkg.jet`)
/// and loads each as an item source so functions and types are available
/// without `use` in REPL inputs.
fn load_project_items(project_dir: &Path, session: &mut Session, out_sink: &mut impl Write) {
    // Try src/ subdirectory first (standard project layout), then project root.
    let src_dirs = [project_dir.join("src"), project_dir.to_path_buf()];
    let mut loaded = 0usize;
    for dir in &src_dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jet") {
                continue;
            }
            // Skip pkg.jet — that's the manifest, not source.
            if path.file_name().and_then(|n| n.to_str()) == Some("pkg.jet") {
                continue;
            }
            let src = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            // Parse to check it has items.
            let (toks, lex_diags) = crate::Lexer::lex(&src);
            if !lex_diags.is_empty() {
                continue;
            }
            match crate::Parser::parse_with_source(&toks, &src) {
                Ok(prog) if !prog.items.is_empty() => {
                    session.item_srcs.push(src);
                    loaded += 1;
                }
                _ => {}
            }
        }
        if loaded > 0 {
            break; // src/ had files — don't double-load from root
        }
    }
    rebuild_funcs(session);
    if loaded > 0 {
        let _ = writeln!(out_sink, "project: loaded {} source file(s)", loaded);
    }
}

// ── input classification ───────────────────────────────────────────────────

/// What a raw input line (or multi-line block) was identified as.
enum InputKind {
    /// A `:command [arg]` meta-command.
    Meta(String, Option<String>),
    /// One or more Jet statements to evaluate.
    /// The `String` is the source snippet that was parsed into `Vec<Stmt>`,
    /// used by `type_check_stmts` to rebuild a synthetic program.
    Stmts(
        Vec<Stmt>,
        bool,   /* suppress echo */
        String, /* check_src */
    ),
    /// A top-level item declaration to add to the session.
    Item(String /* raw src */),
    /// A `use …;` import line to carry across the session (S16/D-REPL10).
    Import(String /* normalized src, ends in `;\n` */),
    /// Empty input — nothing to do.
    Empty,
}

/// Count net open braces/parens/brackets. Used for D-REPL9 multi-line
/// continuation: if balance > 0, keep reading continuation lines.
fn bracket_balance(s: &str) -> i32 {
    let mut bal: i32 = 0;
    let mut in_str = false;
    let mut in_char = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if !in_char => in_str = !in_str,
            '\'' if !in_str => in_char = !in_char,
            '\\' if in_str || in_char => {
                chars.next(); // skip escaped char
            }
            '{' | '(' | '[' if !in_str && !in_char => bal += 1,
            '}' | ')' | ']' if !in_str && !in_char => bal -= 1,
            _ => {}
        }
    }
    bal
}

/// Read a complete input (possibly multi-line) using D-REPL9 rules.
/// Returns `None` on EOF.
fn read_input(stdin: &mut impl BufRead, color: bool, prompt: &str) -> Option<String> {
    let terminal_shift = if color { TERMINAL_SHIFT_IN } else { "" };
    print!("{}{}", terminal_shift, bold(prompt, color));
    io::stdout().flush().ok();
    let mut buf = String::new();
    if stdin.read_line(&mut buf).ok()? == 0 {
        return None; // EOF
    }
    let trimmed = normalize_repl_input(&buf);

    // D-REPL9: keep reading if brackets are unbalanced.
    let mut bal = bracket_balance(&trimmed);
    let mut full = trimmed;
    while bal > 0 {
        print!("{}{}", terminal_shift, dim("...  ", color));
        io::stdout().flush().ok();
        let mut cont = String::new();
        if stdin.read_line(&mut cont).ok()? == 0 {
            break;
        }
        let line = normalize_repl_input(&cont);
        bal += bracket_balance(&line);
        full.push('\n');
        full.push_str(&line);
    }
    Some(full)
}

/// Detect whether the text starts with a keyword that only makes sense as a
/// top-level item declaration (not a statement).
fn looks_like_item(text: &str) -> bool {
    let t = skip_leading_comments(text);
    let t = t.strip_prefix("pub").map(|s| s.trim_start()).unwrap_or(t);
    let t = t.strip_prefix("pure").map(|s| s.trim_start()).unwrap_or(t);
    t.starts_with("fn ")
        || t.starts_with("fn(")
        || t.starts_with("struct ")
        || t.starts_with("enum ")
        || t.starts_with("trait ")
        || t.starts_with("impl ")
        || t.starts_with("const ")
        || t.starts_with("module ")
}

fn skip_leading_comments(mut text: &str) -> &str {
    loop {
        text = text.trim_start();
        if let Some(rest) = text.strip_prefix("//") {
            text = rest.split_once('\n').map_or("", |(_, rest)| rest);
            continue;
        }
        let Some(mut rest) = text.strip_prefix("/*") else {
            return text;
        };
        let mut depth = 1;
        while depth > 0 {
            if let Some(next) = rest.strip_prefix("/*") {
                depth += 1;
                rest = next;
            } else if let Some(next) = rest.strip_prefix("*/") {
                depth -= 1;
                rest = next;
            } else {
                let Some(first) = rest.chars().next() else {
                    return "";
                };
                rest = &rest[first.len_utf8()..];
            }
        }
        text = rest;
    }
}

pub(crate) fn is_item_input(text: &str) -> bool {
    let normalized = normalize_repl_input(text);
    let trimmed = normalized.trim();
    if trimmed.is_empty() || looks_like_import(trimmed) {
        return false;
    }
    if looks_like_item(trimmed) {
        return true;
    }

    // Notebook cells share the parser's complete item plane. Keep the REPL's
    // fast keyword classifier above, but let declarations such as `@name ::`
    // and marker-owned items join the file-wide notebook set without growing a
    // second list of declaration spellings here.
    let (tokens, lex_diags) = crate::Lexer::lex(trimmed);
    if !lex_diags.is_empty() {
        return false;
    }
    crate::Parser::parse_with_source(&tokens, trimmed)
        .map(|program| !program.items.is_empty() && program.script_body.is_empty())
        .unwrap_or(false)
}

/// D-CTCORE1: project accepted Core aliases from sema's name ledger. The
/// interpreter receives this adapter map, never a second raw-source resolver.
pub(crate) fn update_core_imports_from_ledger(
    bundle: &crate::AST::ProgramBundle,
    map: &mut HashMap<String, String>,
) {
    map.clear();
    let module_idx = bundle.entry;
    let module = &bundle.modules[module_idx];
    for import in &module.imports {
        if matches!(import.kind, ImportKind::Unqualified { .. }) {
            for binding in import.walk_bindings() {
                let local = binding.local;
                if let Some(binding) = bundle.name_ledger.effective_alias(module_idx, &local) {
                    let target = binding.target.as_str();
                    let module = if target == "core" || crate::Syntax::is_known_core_module(target)
                    {
                        Some(target.to_string())
                    } else {
                        target
                            .rsplit_once('.')
                            .filter(|(module, _)| crate::Syntax::is_known_core_module(module))
                            .map(|(module, _)| module.to_string())
                    };
                    if let Some(module) = module {
                        map.insert(local, module);
                    }
                }
            }
            continue;
        }
        let alias = import.import_alias();
        let Some(binding) = bundle.name_ledger.effective_alias(module_idx, &alias) else {
            continue;
        };
        let target = binding.target.as_str();
        let module = if target == "core" || crate::Syntax::is_known_core_module(target) {
            Some(target.to_string())
        } else {
            target
                .rsplit_once('.')
                .filter(|(module, _)| crate::Syntax::is_known_core_module(module))
                .map(|(module, _)| module.to_string())
        };
        if let Some(module) = module {
            map.insert(alias, module);
        }
    }
}

/// Detect whether the text is a `use …` import line (S16). `pub use` re-exports
/// are also imports. The parser collects these into `Program.imports`.
fn looks_like_import(text: &str) -> bool {
    let t = text.trim_start();
    let t = t.strip_prefix("pub").map(|s| s.trim_start()).unwrap_or(t);
    t.starts_with("use ") || t == "use"
}

/// Detect whether text is a statement (vs. a bare expression to echo).
/// D-BIND4: a sigil binding (`name :: v` / `name := v`) is a statement; D-IF1:
/// `if` covers multi-arm dispatch (former `when`/`switch`); loops are `loop`.
fn starts_with_stmt_keyword(t: &str) -> bool {
    // A sigil binding contains `::` or `:=` (D-BIND4).
    t.contains("::")
        || t.contains(":=")
        || t.starts_with("return ")
        || t.starts_with("return")
        || t.starts_with("if ")
        || t.starts_with("loop ")
        || t.starts_with("break")
        || t.starts_with("continue")
        || t.starts_with("print(")
        || t.starts_with("eprint(")
}

/// D-FE-REPL-MULTILINE1=A: raw-mode Enter follows parser completeness.
/// Each probe uses the same item/statement/expression shape as `classify`.
/// A diagnostic on synthetic wrapper text means the user's prefix can still
/// become valid with more source; a diagnostic inside their text is complete
/// (and should submit now so the normal owned diagnostic can explain it).
pub(crate) fn raw_input_is_complete(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with(':')
        || trimmed.starts_with('?')
    {
        return true;
    }

    fn probe(prefix: &str, input: &str, suffix: &str) -> bool {
        let cutoff = prefix.len() + input.len();
        let source = format!("{prefix}{input}{suffix}");
        let (tokens, lex_diags) = if prefix.is_empty() {
            crate::Lexer::lex(&source)
        } else {
            crate::Lexer::lex_generated(&source)
        };
        if !lex_diags.is_empty() {
            // E0002 at the user's current end is an unfinished lexical
            // construct (quote/comment/interpolation). If the error ends
            // earlier, another line cannot repair it and Enter must submit.
            return !lex_diags.iter().any(|diag| {
                diag.code == "E0002" && diag.span.is_some_and(|span| span.end >= cutoff)
            });
        }
        match crate::Parser::parse_for_check_with_source(&tokens, &source) {
            Ok(_) => true,
            Err(diags) => !diags
                .iter()
                .any(|diag| diag.span.is_some_and(|span| span.start >= cutoff)),
        }
    }

    if looks_like_item(trimmed) || looks_like_import(trimmed) {
        return probe("", trimmed, "");
    }
    if starts_with_stmt_keyword(trimmed) || trimmed.ends_with(';') {
        probe("fn __repl__() {\n", trimmed, "\n}\n")
    } else {
        probe("fn __repl__() {\n__repl_echo__ :: ", trimmed, "\n}\n")
    }
}

/// Classify the raw input text.
fn classify(text: &str, step: usize) -> Result<InputKind, Vec<Diagnostic>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(InputKind::Empty);
    }

    // Meta-commands (`:cmd [arg]`)
    if let Some(rest) = trimmed.strip_prefix(':') {
        let mut parts = rest.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("").to_string();
        let arg = parts
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        return Ok(InputKind::Meta(cmd, arg));
    }

    // D-FE-REPL-DOCS1=B: bare `?name` is the primary REPL docs spelling —
    // parsed here, never reaches the Jet lexer/parser as source (no new
    // user-typeable syntax). `:?` (above) stays the colon-command alias;
    // both route to the same `"?"` meta-command handler.
    if let Some(rest) = trimmed.strip_prefix('?') {
        let name = rest.trim();
        let arg = if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        };
        return Ok(InputKind::Meta("?".to_string(), arg));
    }

    // `use …` import line (S16). Carry it across the session so a later input
    // can resolve through its alias (D-REPL10).
    if looks_like_import(trimmed) {
        // Imports are line-terminated; an authored `;` stays for the parser to
        // report (E0373), and none is added.
        let normalized = trimmed.trim_end().to_string();
        let full = format!("// repl:{}\n{}\n", step, normalized);
        let (toks, lex_diags) = crate::Lexer::lex(&full);
        if !lex_diags.is_empty() {
            return Err(lex_diags);
        }
        match crate::Parser::parse_with_source(&toks, &full) {
            Ok(prog) if prog.items.is_empty() && !prog.imports.is_empty() => {
                return Ok(InputKind::Import(format!("{}\n", normalized)));
            }
            Ok(_) => {
                // Parsed but isn't a clean single import — fall through to the
                // normal statement/item paths to produce a precise diagnostic.
            }
            Err(ds) => return Err(ds),
        }
    }

    // `;` at end suppresses expression echo (D-REPL16=B).
    // A block ending in `};` is a statement, not a suppressed expression.
    let suppress = trimmed.ends_with(';') && !looks_like_item(trimmed) && !trimmed.ends_with("};");

    // Top-level item?
    if looks_like_item(trimmed) {
        let src = format!("{}\n", trimmed);
        let full = format!("// repl:{}\n{}", step, src);
        let (toks, lex_diags) = crate::Lexer::lex(&full);
        if !lex_diags.is_empty() {
            return Err(lex_diags);
        }
        if let Err(ds) = crate::Parser::parse_with_source(&toks, &full) {
            return Err(ds);
        }
        return Ok(InputKind::Item(src));
    }

    // Wrappers below use Jet's reserved namespace. Validate user text before
    // entering that lane so a source-written `__name` still owns E0067.
    let (_, user_lex_diags) = crate::Lexer::lex(trimmed);
    if !user_lex_diags.is_empty() {
        return Err(user_lex_diags);
    }

    // For everything (statements AND bare expressions), try wrapping as:
    //   fn __repl__() { __repl_echo__ :: <input> }
    // first. If it parses, the last statement is a Val with the magic name.
    // In run_repl_step we detect `__repl_echo__` and echo its value (D-REPL16=B).
    // If input already ends in `;`, suppress the echo.
    //
    // For inputs that are already statements (bindings/print/if/loop/…),
    // the `__repl_echo__ :: stmt` form won't parse; we fall through to
    // plain statement wrapping.
    // Strategy: if the input isn't a statement keyword, try wrapping it as
    // `__repl_echo__ :: <input>`. This lets bare expressions like `1 + 2`
    // parse as a `Val` binding with the magic sentinel name. `run_repl_step`
    // then echoes the value without adding it to the session scope.
    //
    // For statements (bindings/print/if/loop/…) or suppressed inputs, try
    // the plain statement-wrapping form first; if that fails, try the echo
    // sentinel.
    let try_stmt_first = starts_with_stmt_keyword(trimmed) || suppress;

    // Keep generated wrappers in canonical Jet syntax. The lexer inserts
    // synthetic line terminators before the closing brace; authored
    // semicolons would be reported as E0373 by the source-aware parser.
    let plain_input = trimmed.trim_end().to_string();
    let plain_src = format!("// repl:{}\nfn __repl__() {{\n{}\n}}\n", step, plain_input);
    // Echo-sentinel wrapping (D-BIND4: `::` sigil binding).
    let echo_stmt = format!("__repl_echo__ :: {}", trimmed);
    let echo_src = format!("// repl:{}\nfn __repl__() {{\n{}\n}}\n", step, echo_stmt);
    if try_stmt_first {
        // Try plain statement form; if it succeeds, return it.
        // The check source uses the same line-boundary terminator.
        let (toks, lex_diags) = crate::Lexer::lex_generated(&plain_src);
        if lex_diags.is_empty() {
            if let Ok(prog) = crate::Parser::parse_with_source(&toks, &plain_src) {
                if let Some(Item::Func(f)) = prog.items.into_iter().next() {
                    if !f.body.is_empty() {
                        return Ok(InputKind::Stmts(f.body, suppress, plain_input.clone()));
                    }
                }
            }
        }
    }

    // Try echo-sentinel form.
    {
        let (toks, lex_diags) = crate::Lexer::lex_generated(&echo_src);
        if lex_diags.is_empty() {
            if let Ok(prog) = crate::Parser::parse_with_source(&toks, &echo_src) {
                if let Some(Item::Func(f)) = prog.items.into_iter().next() {
                    if !f.body.is_empty() {
                        // The check source uses the same line-boundary
                        // terminator before the generated closing brace.
                        let echo_check_src = echo_stmt.clone();
                        return Ok(InputKind::Stmts(f.body, suppress, echo_check_src));
                    }
                }
            }
        }
    }

    // Fallback: plain statement form (returns parse error if any).
    let (toks, lex_diags) = crate::Lexer::lex_generated(&plain_src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    match crate::Parser::parse_with_source(&toks, &plain_src) {
        Ok(prog) => {
            if let Some(Item::Func(f)) = prog.items.into_iter().next() {
                return Ok(InputKind::Stmts(f.body, suppress, plain_input));
            }
            Ok(InputKind::Empty)
        }
        Err(ds) => Err(ds),
    }
}

// ── sema check helpers ─────────────────────────────────────────────────────

/// Standard prelude import injected into every REPL program (D-REPL-PRELOAD=A).
/// Note: `print`/`eprint` are builtins that don't require an import in Jet v1;
/// the `use core.term` line is included for teaching purposes (the REPL shows a
/// note about it on first use) but the import itself is a no-op for the checker.
const PRELOAD_SRC: &str = "";

/// Build a complete synthetic Jet source from accumulated declarations +
/// a new check function, then run sema. `check_src` is the statement(s) text
/// as produced by `classify` — it already ends in `;` (or `}`).
///
/// Returns the *checked* statements — the AST sema normalized, which is the
/// only AST any tier executes — together with the checked module's Core alias
/// map (D-CTCORE1). The map matters because sema's one prelude opening
/// (D-NAME-ALIAS1=A) rewrites an ambient name such as `read_file` into a call
/// on a compiler-owned alias that exists only in this turn's synthetic
/// module, so the interpreter needs the same alias map sema resolved against.
fn type_check_stmts(
    session: &Session,
    stmts: &[Stmt],
    step: usize,
    check_src: &str,
) -> Result<(Vec<Stmt>, HashMap<String, String>), Vec<Diagnostic>> {
    let base_src = format!(
        "{}{}{}",
        PRELOAD_SRC,
        session.import_src(),
        session.accumulated_src(),
    );
    let prog_src = format!("{base_src}\nfn __repl_check_{step}__() {{\n{check_src}\n}}\n");
    let mut body = session.sema_stmts.clone();
    body.extend_from_slice(stmts);
    run_sema_with_body(
        &prog_src,
        &base_src,
        &format!("__repl_check_{}__", step),
        body,
        stmts.len(),
    )
}

fn run_sema_with_body(
    src: &str,
    base_src: &str,
    fn_name: &str,
    body: Vec<Stmt>,
    current_len: usize,
) -> Result<(Vec<Stmt>, HashMap<String, String>), Vec<Diagnostic>> {
    // Session declarations remain user source even though the final check
    // function is compiler-owned.
    let (_, base_lex_diags) = crate::Lexer::lex(base_src);
    if !base_lex_diags.is_empty() {
        return Err(base_lex_diags);
    }
    let (toks, lex_diags) = crate::Lexer::lex_generated(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = match crate::Parser::parse_with_source(&toks, src) {
        Ok(p) => p,
        Err(ds) => return Err(ds),
    };
    if let Some(Item::Func(f)) = prog
        .items
        .iter_mut()
        .find(|item| matches!(item, Item::Func(f) if f.name == fn_name))
    {
        f.body = body;
    }
    let (diags, mut bundle) = if prog.imports.is_empty() {
        let mut bundle = program_bundle(src, prog, crate::Lexer::core_identifier_demand(&toks));
        let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Check);
        (diags, bundle)
    } else {
        let tmp_path = match write_unique_temp_file("check_ast", base_src) {
            Ok(path) => path,
            Err(_) => {
                return Ok((
                    current_stmts(&mut prog.items, fn_name, current_len),
                    HashMap::new(),
                ));
            }
        };
        let path_str = tmp_path.to_string_lossy().to_string();
        let result = crate::Loader::load_entry(&path_str).map(|mut bundle| {
            let entry = bundle.entry;
            bundle.modules[entry].source = src.to_string();
            bundle.modules[entry].items = std::mem::take(&mut prog.items);
            bundle.modules[entry].block_spans = std::mem::take(&mut prog.block_spans);
            let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Check);
            (diags, bundle)
        });
        let _ = std::fs::remove_file(&tmp_path);
        match result {
            Ok(result) => result,
            Err(ds) => return Err(ds),
        }
    };
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
        .collect();
    if errors.is_empty() {
        let entry = bundle.entry;
        let mut core_imports = HashMap::new();
        update_core_imports_from_ledger(&bundle, &mut core_imports);
        Ok((
            current_stmts(&mut bundle.modules[entry].items, fn_name, current_len),
            core_imports,
        ))
    } else {
        Err(errors)
    }
}

fn current_stmts(items: &mut [Item], fn_name: &str, current_len: usize) -> Vec<Stmt> {
    let body = items
        .iter_mut()
        .find_map(|item| match item {
            Item::Func(f) if f.name == fn_name => Some(&mut f.body),
            _ => None,
        })
        .expect("REPL check function survives sema");
    body.split_off(body.len() - current_len)
}



/// Build a complete synthetic Jet source from accumulated declarations +
/// new item, then run sema. The successful rebuild retains this checked AST.
pub(crate) fn type_check_item(
    session: &Session,
    new_item_src: &str,
) -> Result<crate::AST::ProgramBundle, Vec<Diagnostic>> {
    let prog_src = format!(
        "{}{}{}\n{}\n",
        PRELOAD_SRC,
        session.import_src(),
        session.accumulated_src(),
        new_item_src,
    );
    checked_program(&prog_src)
}

fn checked_top_level_funcs(bundle: &crate::AST::ProgramBundle) -> HashMap<String, Func> {
    bundle.modules[bundle.entry]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Func(function) => Some((function.name.clone(), function.clone())),
            _ => None,
        })
        .collect()
}

fn checked_top_level_structs(bundle: &crate::AST::ProgramBundle) -> HashMap<String, StructDef> {
    bundle.modules[bundle.entry]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(s) => Some((s.name.clone(), s.clone())),
            _ => None,
        })
        .collect()
}

fn checked_top_level_enums(bundle: &crate::AST::ProgramBundle) -> HashMap<String, EnumDef> {
    bundle.modules[bundle.entry]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Enum(e) => Some((e.name.clone(), e.clone())),
            _ => None,
        })
        .collect()
}

/// Parse + sema-check `src`, returning the checked AST. Uses `Check` mode so
/// E0101 (no `run`) is not fired — a REPL session is a library.
fn checked_program(src: &str) -> Result<crate::AST::ProgramBundle, Vec<Diagnostic>> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags
            .into_iter()
            .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
            .collect());
    }
    let prog = match crate::Parser::parse_with_source(&toks, src) {
        Ok(p) => p,
        Err(ds) => return Err(ds),
    };
    // File imports need Loader resolution. Import-free programs use the same
    // canonical one-module bundle checker as every other compile path.
    let mut bundle = if prog.imports.is_empty() {
        program_bundle(src, prog, crate::Lexer::core_identifier_demand(&toks))
    } else {
        match write_unique_temp_file("check", src) {
            Err(_) => program_bundle(src, prog, crate::Lexer::core_identifier_demand(&toks)),
            Ok(tmp_path) => {
                let loaded = crate::Loader::load_entry(&tmp_path.to_string_lossy());
                let _ = std::fs::remove_file(&tmp_path);
                loaded?
            }
        }
    };
    let errors: Vec<_> = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Check)
        .into_iter()
        .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
        .collect();
    if errors.is_empty() {
        Ok(bundle)
    } else {
        Err(errors)
    }
}

fn program_bundle(
    src: &str,
    mut prog: crate::AST::Program,
    core_identifier_demand: crate::AST::CoreIdentifierDemand,
) -> crate::AST::ProgramBundle {
    let path = std::path::PathBuf::from("<repl>");
    crate::AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from("."),
        modules: vec![crate::AST::LoadedModule {
            path,
            display: "<repl>".to_string(),
            source: src.to_string(),
            core_identifier_demand,
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            script_body: std::mem::take(&mut prog.script_body),
            web_target_ceiling: prog.web_target_ceiling,
            no_prelude: prog.no_prelude,
            default_target: prog.default_target,
            html_path: prog.html_path,
            policy_declarations: prog.policy_declarations.clone(),
            user_policy_declarations: prog.user_policy_declarations.clone(),
            block_spans: prog.block_spans.clone(),
            rule_facts: std::mem::take(&mut prog.rule_facts),
        }],
        devtools_registry: crate::AST::DevtoolsRegistry::default(),
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        ffi_callback_fns: std::collections::HashSet::new(),
        cffi: crate::AST::CFfi::default(),
        comptime_inputs: Vec::new(),
        name_ledger: crate::AST::NameLedger::default(),
        layer_ceiling: None,
        inferred_layer: crate::Syntax::RuntimeLayer::Core,
        web_partitions: std::collections::HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
        package_guarantees: Default::default(),
        program_allocator: Default::default(),
        active_os: crate::Syntax::OSTarget::host(),
        build_facts: Default::default(),
        edition: crate::Manifest::latest_edition().to_string(),
    }
}

/// Rebuild the func_defs table from accumulated item sources.
fn rebuild_funcs(session: &mut Session) {
    let src = format!(
        "{}{}{}",
        PRELOAD_SRC,
        session.import_src(),
        session.accumulated_src()
    );
    if let Ok(bundle) = checked_program(&src) {
        session.func_defs = checked_top_level_funcs(&bundle);
        session.struct_defs = checked_top_level_structs(&bundle);
        session.enum_defs = checked_top_level_enums(&bundle);
        return;
    }
    session.func_defs.clear();
    session.struct_defs.clear();
    session.enum_defs.clear();
    let (toks, _) = crate::Lexer::lex(&src);
    if let Ok(prog) = crate::Parser::parse_with_source(&toks, &src) {
        for item in prog.items {
            match item {
                Item::Func(f) => {
                    session.func_defs.insert(f.name.clone(), f);
                }
                Item::Struct(s) => {
                    session.struct_defs.insert(s.name.clone(), s);
                }
                Item::Enum(e) => {
                    session.enum_defs.insert(e.name.clone(), e);
                }
                _ => {}
            }
        }
    }
}

// ── :load ─────────────────────────────────────────────────────────────────

/// `:load <file>` — load a Jet source file into the session. The one
/// file-into-session mechanism (D-REPL15=B).
///
/// `run_entry` says whether a `fn run` in the file is invoked on load.
/// Interactive `:load` passes `true` (its shipped behavior). The `jet repl
/// <file>.jet` preload passes `false` (#2038): naming a file on the command
/// line makes its definitions available, it does not run the program — that
/// is `jet run`.
fn cmd_load(
    path_str: &str,
    session: &mut Session,
    base_dir: &Path,
    run_entry: bool,
    authorizer: &mut ReplAuthorization<'_>,
    out_sink: &mut impl Write,
) {
    let src = match std::fs::read_to_string(path_str) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(out_sink, "error: couldn't read `{}`: {}", path_str, e);
            return;
        }
    };
    // Parse to count items and detect run.
    let (toks, lex_diags) = crate::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        let n = lex_diags.len();
        let _ = writeln!(out_sink, "{} parse error(s) in `{}`", n, path_str);
        return;
    }
    let prog = match crate::Parser::parse_with_source(&toks, &src) {
        Ok(p) => p,
        Err(ds) => {
            let _ = writeln!(out_sink, "{} parse error(s) in `{}`", ds.len(), path_str);
            return;
        }
    };
    let item_count = prog.items.len();
    let has_run = prog
        .items
        .iter()
        .any(|i| matches!(i, Item::Func(f) if f.name == "run"));

    // Add each item's source to the session (approximate: use the whole file).
    session.item_srcs.push(src.clone());
    rebuild_funcs(session);

    let _ = writeln!(out_sink, "loaded {} items from `{}`", item_count, path_str);

    // Run the file's `fn run` once, built and executed like `:run`.
    if has_run && run_entry {
        let output = run_session_program(session, "run()", base_dir, authorizer);
        let _ = write!(out_sink, "{}", output);
    }
}

// ── :type ─────────────────────────────────────────────────────────────────

fn cmd_type(name: &str, session: &Session, color: bool) {
    if session.scope.contains_key(name) {
        let ty = session
            .binding_types
            .get(name)
            .map(|t| t.name())
            .unwrap_or_else(|| "?".to_string());
        println!("{} : {}", name, ty);
    } else if session.func_defs.contains_key(name) {
        println!("{} : fn", name);
    } else {
        println!(
            "{}: `{}` isn't defined in this session",
            yellow("note", color),
            name
        );
    }
}

// ── diagnostic rendering ───────────────────────────────────────────────────

fn render_diags(file: &str, src: &str, diags: &[Diagnostic], color: bool) {
    eprint!(
        "{}",
        crate::Diagnostics::render_all_colored(file, src, diags, color)
    );
    let n = diags.len();
    eprintln!("{} problem{} found", n, if n == 1 { "" } else { "s" });
}
/// REPL wrappers add generated source before the user's input, but diagnostics
/// render against the input line alone. Shift every returned span back to that
/// source before rendering; checked subtraction preserves zero-width spans and
/// clamps only diagnostics that point into the generated wrapper.
fn reanchor_repl_diagnostic(diagnostic: &mut Diagnostic, input_start: usize) {
    let Some(span) = diagnostic.span else {
        return;
    };
    let start = span.start.checked_sub(input_start).unwrap_or(0);
    let end = span.end.checked_sub(input_start).unwrap_or(start);
    diagnostic.span = Some(crate::Diagnostics::Span::new(start, end.max(start)));
}

// ── main REPL loop ─────────────────────────────────────────────────────────

/// Run an interactive REPL session.
/// `jet repl` entry point. Prints the D-FE-REPL1=D banner unconditionally
/// (TTY and non-TTY alike — only its wording changed from the pre-redesign
/// banner), then hands off to the interactive raw-mode loop when stdin/stdout
/// are both a real terminal and `stty` is available, or the cooked
/// line-buffered loop otherwise (I6: `stty` shell-out, no line-editing
/// crate). The cooked loop is also the exact non-TTY floor `run_transcript`
/// mirrors — piped/redirected sessions keep the pre-redesign plain output.
pub fn run(project_dir: Option<&str>, flags: ReplFlags) -> i32 {
    use std::io::IsTerminal;
    let color = flags.color.resolve(io::stdout().is_terminal());
    let raw_guard = Terminal::RawGuard::enable();
    println!(
        "{}",
        Render::render_banner(env!("CARGO_PKG_VERSION"), color)
    );
    println!();
    eprintln!(
        "{}",
        Render::render_discovery_hint(raw_guard.is_some(), color)
    );

    match raw_guard {
        Some(guard) => Interactive::run_interactive(project_dir, color, guard, flags),
        None => run_cooked(project_dir, color, flags),
    }
}

/// Pre-redesign line-buffered loop: `read_line` per input, plain `user> `
/// prompt (no turn gutter — that's an interactive-only affordance), full
/// unfolded echoes. This is the non-TTY fallback and stays byte-identical to
/// the REPL's prior behavior (tests/cli's `no_args_repl_banner_golden` and
/// every `tests/repl.rs` transcript floor depend on it).
fn run_cooked(project_dir: Option<&str>, color: bool, flags: ReplFlags) -> i32 {
    let base_dir: std::path::PathBuf =
        project_dir
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });

    let mut session = Session::new();
    session.enable_persistent_history();
    let preload = flags.preload.clone();
    let mut policy = ReplPolicy::new(flags, &base_dir);

    // D-REPL10=A: --project loads the project's source items into the session
    // so functions/types are available without `use`.
    if let Some(dir) = project_dir {
        let mut stdout = io::stdout();
        load_project_items(Path::new(dir), &mut session, &mut stdout);
        session.preserve_project_baseline();
    }

    // #2038: `jet repl <file>.jet`. Same loader as `:load`, `fn run` not
    // invoked. Both may be given: `--project` seeds the package, the file
    // then loads on top.
    if let Some(file) = &preload {
        let mut stdout = io::stdout();
        let mut authorizer = policy.authorizer(None);
        cmd_load(file, &mut session, &base_dir, false, &mut authorizer, &mut stdout);
    }

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();

    loop {
        let prompt = Render::render_prompt(session.turns.len() + 1, color);
        let text = match read_input(&mut stdin_lock, color, &prompt) {
            Some(t) => t,
            None => {
                println!();
                break;
            }
        };

        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(arg) = trimmed.strip_prefix(":rerun ") {
            let mut authorizer = policy.authorizer(None);
            rerun_cooked(
                arg,
                &mut stdin_lock,
                &mut session,
                &base_dir,
                color,
                &mut authorizer,
            );
            continue;
        }

        let mut authorizer = policy.authorizer(None);
        if execute_line(
            trimmed,
            &mut session,
            &base_dir,
            color,
            false,
            false,
            &mut authorizer,
        ) {
            break;
        }
    }

    0
}

fn rerun_cooked(
    id_text: &str,
    input: &mut dyn BufRead,
    session: &mut Session,
    base_dir: &Path,
    color: bool,
    authorizer: &mut ReplAuthorization<'_>,
) {
    let id = match id_text.trim().parse::<usize>() {
        Ok(id) if session.turns.iter().any(|turn| turn.id == id) => id,
        _ => {
            eprintln!("turn #{} does not exist", id_text.trim());
            return;
        }
    };
    let original = session
        .turns
        .iter()
        .find(|turn| turn.id == id)
        .unwrap()
        .input
        .clone();
    print!("Edit turn {id} [{original}]: ");
    io::stdout().flush().ok();
    let mut edited = String::new();
    if input
        .read_line(&mut edited)
        .ok()
        .filter(|n| *n > 0)
        .is_none()
    {
        eprintln!("rerun cancelled at end of input");
        return;
    }
    let edited = edited.trim();
    let replacement = (!edited.is_empty() && edited != original).then_some(edited);
    let plan = match RerunPlan::build_replay_plan(&session.turns, id, replacement) {
        Ok(plan) => plan,
        Err(message) => {
            eprintln!("{message}");
            return;
        }
    };
    println!("{}", RerunPlan::render_replay_plan(&plan, color));
    let mut stale_from = None;
    for step in &plan.steps {
        if step.kind != RerunPlan::StepKind::ConfirmEffect {
            continue;
        }
        print!(
            "Replay effect turn {}? [y] replay  [s] skip and mark stale: ",
            step.turn_id
        );
        io::stdout().flush().ok();
        let mut answer = String::new();
        if input.read_line(&mut answer).is_err() || !answer.trim().eq_ignore_ascii_case("y") {
            stale_from = Some(step.turn_id);
            break;
        }
    }
    apply_replay_plan_with_stale(session, &plan, stale_from, base_dir, color, authorizer);
}

/// Classify + execute one already-trimmed input line, printing results to
/// stdout/stderr and recording the turn. Shared by `run_cooked` and
/// `Interactive::run_interactive` — the two live-terminal loops (the
/// transcript-buffer path, `run_transcript`, stays independent below so its
/// pinned floor output can never be perturbed by this shared path).
///
/// `fold_long_output` gates D-FE-REPL1=D auto-fold (`⋯ N rows folded …`) for
/// long `List` echoes — off for the cooked/plain floor, on in the
/// interactive TTY loop. `quiet` suppresses all printing while still
/// classifying/executing/recording the turn — used by
/// `apply_replay_plan` to silently rebuild session state for turns before
/// the one actually being rerun (D-FE-REPL-RERUN1=A).
///
/// Returns `true` when `:quit`/`:q`/`:exit` was requested.
pub(crate) fn execute_line(
    trimmed: &str,
    session: &mut Session,
    base_dir: &Path,
    color: bool,
    fold_long_output: bool,
    quiet: bool,
    authorizer: &mut ReplAuthorization<'_>,
) -> bool {
    session.step += 1;
    let step_src = format!("<repl:{}>", session.step);

    macro_rules! qprintln {
        ($($arg:tt)*) => {
            if !quiet { println!($($arg)*); }
        };
    }
    macro_rules! qprint {
        ($($arg:tt)*) => {
            if !quiet { print!($($arg)*); }
        };
    }
    macro_rules! qeprint {
        ($($arg:tt)*) => {
            if !quiet { eprint!($($arg)*); }
        };
    }

    // D-REPL-PRELOAD teaching note: on first use of print/eprint/input.
    if !session.shown_preload_note
        && (trimmed.contains("print(") || trimmed.contains("eprint(") || trimmed.contains("input("))
    {
        qprintln!(
            "{}",
            dim(
                "note: `print` is from `use core.term` — imported automatically in the REPL",
                color
            )
        );
        session.shown_preload_note = true;
    }

    let kind = match classify(trimmed, session.step) {
        Ok(k) => k,
        Err(ds) => {
            if !quiet {
                render_diags(&step_src, trimmed, &ds, color);
            }
            session.record_turn(
                trimmed,
                ReplTurnStatus::Error,
                ds.iter()
                    .map(|d| format!("{}: {}", d.code, d.what))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            return false;
        }
    };

    match kind {
        InputKind::Empty => {}

        InputKind::Meta(cmd, arg) => {
            if quiet {
                // Meta-commands don't create turns and aren't part of a
                // replay plan's own steps — a quiet prior-state replay never
                // reaches one (only Item/Import/Reject/Stmts turns are ever
                // recorded), but guard defensively rather than print anyway.
                return false;
            }
            return handle_meta(&cmd, arg.as_deref(), session, base_dir, color, authorizer);
        }

        InputKind::Item(src) => {
            let bundle = match type_check_item(session, &src) {
                Ok(bundle) => bundle,
                Err(errors) => {
                    if !quiet {
                        render_diags(&step_src, trimmed, &errors, color);
                    }
                    session.record_turn(
                        trimmed,
                        ReplTurnStatus::Error,
                        errors
                            .iter()
                            .map(|d| format!("{}: {}", d.code, d.what))
                            .collect::<Vec<_>>()
                            .join("; "),
                    );
                    return false;
                }
            };
            session.item_srcs.push(src);
            rebuild_funcs(session);
            update_core_imports_from_ledger(&bundle, &mut session.core_imports);
            qprintln!("{}", green("ok", color));
            session.record_turn(trimmed, ReplTurnStatus::Ok, "ok".to_string());
            if !quiet {
                session.remember_success(trimmed);
            }
        }

        InputKind::Import(src) => {
            // Try it against the accumulated session so a bad import
            // (unknown core module, etc.) reports before being kept.
            session.import_srcs.push(src.clone());
            let bundle = match type_check_item(session, "") {
                Ok(bundle) => bundle,
                Err(errors) => {
                    session.import_srcs.pop();
                    if !quiet {
                        render_diags(&step_src, trimmed, &errors, color);
                    }
                    session.record_turn(
                        trimmed,
                        ReplTurnStatus::Error,
                        errors
                            .iter()
                            .map(|d| format!("{}: {}", d.code, d.what))
                            .collect::<Vec<_>>()
                            .join("; "),
                    );
                    return false;
                }
            };
            rebuild_funcs(session);
            update_core_imports_from_ledger(&bundle, &mut session.core_imports);
            qprintln!("{}", green("ok", color));
            session.record_turn(trimmed, ReplTurnStatus::Ok, "ok".to_string());
            if !quiet {
                session.remember_success(trimmed);
            }
        }

        InputKind::Stmts(stmts, suppress, check_src) => {
            match run_stmts_turn(
                session, &stmts, suppress, &check_src, trimmed, base_dir, authorizer, false,
            ) {
                TurnResult::Done {
                    stdout,
                    stderr,
                    echo,
                    echo_ty,
                    bound_name,
                    had_effect,
                } => {
                    let mut summary = String::new();
                    if !stdout.is_empty() {
                        qprint!("{}", stdout);
                        summary.push_str(&stdout);
                    }
                    if !stderr.is_empty() {
                        qeprint!("{}", stderr);
                        summary.push_str(&stderr);
                    }
                    // D-FE-REPL1=D: long list echoes auto-fold in the
                    // interactive loop (`fold_long_output`); the cooked/plain
                    // floor and quiet replays always print the full value.
                    let mut pending_unfold: Option<String> = None;
                    if let Some(value) = echo {
                        let shown = echo_text(&value);
                        let fold = fold_long_output
                            .then(|| {
                                echo_ty
                                    .as_ref()
                                    .and_then(|ty| Render::fold_decision_for_value(ty, &value.shown))
                            })
                            .flatten();
                        match fold {
                            Some((count, elem_ty)) => {
                                qprintln!("{}", Render::render_fold_marker(count, &elem_ty, color));
                                summary.push_str(&Render::render_fold_marker(count, &elem_ty, false));
                                pending_unfold = Some(shown);
                            }
                            None => {
                                qprintln!("{}", shown);
                                summary.push_str(&shown);
                            }
                        }
                    }
                    session.record_turn_ex(trimmed, ReplTurnStatus::Ok, summary, had_effect, bound_name);
                    if !quiet {
                        session.remember_success(trimmed);
                    }
                    if let Some(full) = pending_unfold {
                        if let Some(t) = session.turns.last_mut() {
                            t.folded = true;
                            t.pending_unfold = Some(full);
                        }
                    }
                }
                TurnResult::Interrupted => {
                    qprintln!("Interrupted. External effects already performed were not rolled back.");
                    session.record_turn_ex(
                        trimmed,
                        ReplTurnStatus::Interrupted,
                        "Interrupted".to_string(),
                        true,
                        None,
                    );
                }
                TurnResult::Failed {
                    stdout,
                    stderr,
                    diagnostics,
                    report,
                } => {
                    qprint!("{}", stdout);
                    qeprint!("{}", stderr);
                    if !quiet {
                        render_diags(&step_src, trimmed, &diagnostics, color);
                    }
                    if let Some(report) = &report {
                        qeprint!("{}", report);
                    }
                    session.record_turn(trimmed, ReplTurnStatus::Error, turn_failure_summary(&diagnostics, report.as_deref()));
                }
            }
        }
    }

    false
}

/// Handle a `:command` meta-command (D-REPL15=B). Returns `true` when
/// `:quit`/`:q`/`:exit` was requested — the caller breaks its own loop and
/// returns normally instead of this function calling `std::process::exit`
/// directly, so a live `Terminal::RawGuard` (interactive TTY path) gets to
/// run its `Drop` and restore cooked terminal mode before the process exits
/// (`std::process::exit` skips destructors).
fn handle_meta(
    cmd: &str,
    arg: Option<&str>,
    session: &mut Session,
    base_dir: &Path,
    color: bool,
    authorizer: &mut ReplAuthorization<'_>,
) -> bool {
    match cmd {
        "quit" | "q" | "exit" => {
            println!("bye");
            return true;
        }
        "reset" => {
            session.reset();
            authorizer.reset_session();
            println!("session reset");
        }
        "load" => {
            let path = match arg {
                Some(p) => p,
                None => {
                    eprintln!("usage: :load <file.jet>");
                    return false;
                }
            };
            let mut stdout = io::stdout();
            cmd_load(path, session, base_dir, true, authorizer, &mut stdout);
        }
        "type" => {
            let name = match arg {
                Some(n) => n,
                None => {
                    eprintln!("usage: :type <name>");
                    return false;
                }
            };
            cmd_type(name, session, color);
        }
        "run" => {
            // The whole session as one built program (D-EXEC2).
            println!("{} {}", dim("compiling session…", color), dim("running…", color));
            print!("{}", cmd_run(session, base_dir, authorizer));
        }
        "turns" => render_turns(session, color),
        "fold" => {
            match parse_turn_id(arg).and_then(|id| set_turn_flag(session, id, "folded", true)) {
                Ok(()) => println!("turn folded"),
                Err(msg) => eprintln!("{msg}"),
            }
        }
        "unfold" => match parse_turn_id(arg).and_then(|id| unfold_turn(session, id)) {
            Ok(full) => {
                println!("turn unfolded");
                // D-FE-REPL1=D: a turn folded by the interactive auto-fold
                // (a long List echo) carries the full text it elided — show
                // it now rather than leaving `:unfold` a no-op note.
                if let Some(full) = full {
                    println!("{}", full);
                }
            }
            Err(msg) => eprintln!("{msg}"),
        },
        "pin" => match parse_turn_id(arg).and_then(|id| set_turn_flag(session, id, "pinned", true))
        {
            Ok(()) => println!("turn pinned"),
            Err(msg) => eprintln!("{msg}"),
        },
        "unpin" => {
            match parse_turn_id(arg).and_then(|id| set_turn_flag(session, id, "pinned", false)) {
                Ok(()) => println!("turn unpinned"),
                Err(msg) => eprintln!("{msg}"),
            }
        }
        "rerun" => match parse_turn_id(arg) {
            Ok(id) => {
                // D-FE-REPL-RERUN1=A: build + show the replay plan; unedited
                // `:rerun <id>` textual fallback applies it immediately when
                // no step needs a side-effect confirmation, mirroring `^R`'s
                // interactive flow without a prompt to answer in a script.
                match RerunPlan::build_replay_plan(&session.turns, id, None) {
                    Ok(plan) => {
                        if RerunPlan::plan_needs_confirmation(&plan) {
                            // Shows the `Apply? [y/N]` shape (D-FE-REPL-RERUN1=A)
                            // but this textual fallback has no key to read a
                            // reply from — point at `^R`, which does.
                            println!("{}", RerunPlan::render_replay_plan(&plan, color));
                            println!(
                                "note: this plan includes effectful turns — use the interactive `^R` to confirm/skip them"
                            );
                        } else {
                            apply_replay_plan(session, &plan, base_dir, color, authorizer);
                        }
                    }
                    Err(msg) => eprintln!("{msg}"),
                }
            }
            Err(msg) => eprintln!("{msg}"),
        },
        "history" => handle_history(arg, &mut session.history),
        "?" => match arg {
            Some(name) => match Docs::lookup(session, name) {
                Some(text) => print!("{}", text),
                None => println!(
                    "{}: `{}` isn't defined in this session",
                    yellow("note", color),
                    name
                ),
            },
            None => print_help(color),
        },
        "help" => print_help(color),
        _ => {
            eprintln!(
                "unknown meta-command `:{}`; type {} to see available commands",
                cmd,
                bold(":help", color)
            );
        }
    }
    false
}

/// D-FE-REPL-RERUN1=A: apply an already-built replay plan whose steps are all
/// `Auto` (or whose `ConfirmEffect` steps the caller already confirmed).
/// Rebuilds session state by clearing the session and replaying every turn
/// from turn 1 through the plan's last step in order — turns before
/// `plan.from_id` are known-good (their input/output already matched once)
/// so they replay silently; only the plan's own steps print.
fn apply_replay_plan(
    session: &mut Session,
    plan: &RerunPlan::ReplayPlan,
    base_dir: &Path,
    color: bool,
    authorizer: &mut ReplAuthorization<'_>,
) {
    apply_replay_plan_with_stale(session, plan, None, base_dir, color, authorizer);
}

/// Apply replay while conservatively retaining a skipped effect and every
/// downstream turn as stale notebook history. Those turns are not evaluated
/// and therefore cannot contribute bindings to the rebuilt live session.
pub(crate) fn apply_replay_plan_with_stale(
    session: &mut Session,
    plan: &RerunPlan::ReplayPlan,
    stale_from: Option<usize>,
    base_dir: &Path,
    color: bool,
    authorizer: &mut ReplAuthorization<'_>,
) {
    apply_replay_plan_with_stale_mode(session, plan, stale_from, base_dir, color, authorizer, true);
}

/// Notebook/headless callers need the same replay state transition without
/// human progress lines contaminating their JSONL output.
pub(crate) fn apply_replay_plan_with_stale_quiet(
    session: &mut Session,
    plan: &RerunPlan::ReplayPlan,
    stale_from: Option<usize>,
    base_dir: &Path,
    color: bool,
    authorizer: &mut ReplAuthorization<'_>,
) {
    apply_replay_plan_with_stale_mode(
        session, plan, stale_from, base_dir, color, authorizer, false,
    );
}

fn apply_replay_plan_with_stale_mode(
    session: &mut Session,
    plan: &RerunPlan::ReplayPlan,
    stale_from: Option<usize>,
    base_dir: &Path,
    color: bool,
    authorizer: &mut ReplAuthorization<'_>,
    announce: bool,
) {
    let prior: Vec<String> = session
        .turns
        .iter()
        .filter(|t| t.id < plan.from_id)
        .map(|t| t.input.clone())
        .collect();
    session.reset();
    // Turns before the edited one already matched once — replay them quietly
    // to rebuild state, so only the plan's own steps print.
    for input in &prior {
        execute_line(input, session, base_dir, color, false, true, authorizer);
    }
    for step in &plan.steps {
        if stale_from.is_some_and(|from| step.turn_id >= from) {
            session.turns.push(ReplTurn {
                id: session.turns.len() + 1,
                input: step.input.clone(),
                summary: "not replayed".to_string(),
                status: ReplTurnStatus::Ok,
                folded: false,
                pinned: false,
                stale: true,
                had_effect: step.kind == RerunPlan::StepKind::ConfirmEffect,
                bound_name: None,
                pending_unfold: None,
            });
            if announce {
                println!(
                    "{}",
                    dim(&format!("turn {} stale: not replayed", step.turn_id), color)
                );
            }
            continue;
        }
        if announce {
            println!(
                "{}",
                dim(&format!("rerun #{}: {}", step.turn_id, step.input), color)
            );
        }
        execute_line(
            &step.input,
            session,
            base_dir,
            color,
            false,
            false,
            authorizer,
        );
    }
}

fn help_text(color: bool) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(out, "{}", bold("REPL Meta-Commands", color)).unwrap();
    writeln!(out, "  {}          end the session", bold(":quit", color)).unwrap();
    writeln!(
        out,
        "  {}         clear all bindings and start fresh",
        bold(":reset", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}  load a Jet file into the session",
        bold(":load <file.jet>", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}      show the type of a binding",
        bold(":type <name>", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}           compile + run the session (bypasses fuel cap)",
        bold(":run", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}         list notebook turns, status, pins, and folds",
        bold(":turns", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {} search or erase successful submission history",
        bold(":history search <text> | clear", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}       rerun a prior turn as a preview",
        bold(":rerun <id>", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}        fold or unfold a turn's output summary",
        bold(":fold <id>", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}         pin or unpin a turn in the notebook rail",
        bold(":pin <id>", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}           show docs/type info for a name",
        bold("?name", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}         alias for {}",
        bold(":? <name>", color),
        bold("?name", color)
    )
    .unwrap();
    writeln!(out, "  {}          show this message", bold(":help", color)).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "{}", bold("Interactive terminal only", color)).unwrap();
    writeln!(
        out,
        "  {}             complete the current name",
        bold("Tab", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}              docs for the name under the cursor",
        bold("F1", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}              search successful submission history",
        bold("F3", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}              pin or unpin the latest turn",
        bold("^P", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}              fold or unfold the latest turn",
        bold("^F", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}              edit and rerun a prior turn",
        bold("^R", color)
    )
    .unwrap();
    writeln!(
        out,
        "  {}              open or close live bindings",
        bold("^B", color)
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "  Tip: end a line with {} to suppress echo of its value.",
        bold(";", color)
    )
    .unwrap();
    writeln!(
        out,
        "  Tip: {} is auto-imported — type `print(\"hello\")` to try it.",
        bold("core.term", color)
    )
    .unwrap();
    writeln!(
        out,
        "  Tip: a `{}` line is kept for the whole session — import once, use it on any later line.",
        bold("use core.math as math", color)
    )
    .unwrap();
    out
}

fn print_help(color: bool) {
    print!("{}", help_text(color));
}

fn handle_history(arg: Option<&str>, history: &mut History::History) {
    let Some(arg) = arg else {
        eprintln!("usage: :history search <text> | :history clear");
        return;
    };
    if arg == "clear" {
        match history.clear() {
            Ok(()) => println!("History cleared."),
            Err(error) => eprintln!("warning: history could not be cleared: {error}"),
        }
        return;
    }
    if let Some(needle) = arg.strip_prefix("search ") {
        let matches = history.search(needle);
        if matches.is_empty() {
            println!("No history matches.");
        } else {
            for entry in matches {
                println!("{entry}");
            }
        }
        return;
    }
    eprintln!("usage: :history search <text> | :history clear");
}

// ── one-shot command-line expression (#2068) ──────────────────────────────

/// What one command-line expression produced.
pub struct EvalOnce {
    /// The value the input evaluated to. `None` when the input only bound or
    /// declared something and so has no value to show.
    pub value: Option<EvalValue>,
    /// Anything the input printed, in program order.
    pub stdout: String,
    pub stderr: String,
}

/// One exported value: the checked type, the built program's Display text,
/// and its type-directed JSON (the `CtValue::to_json` shapes).
pub struct EvalValue {
    pub ty: Option<String>,
    pub shown: String,
    pub json: String,
}

/// Why a command-line expression produced no value.
pub struct EvalError {
    /// Refusals before the program ran (check, build, authority).
    pub diagnostics: Vec<Diagnostic>,
    /// The runtime stop report, when the program started and stopped.
    pub report: Option<String>,
    pub stdout: String,
    pub stderr: String,
}

impl From<Vec<Diagnostic>> for EvalError {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            diagnostics,
            report: None,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// `jet eval "<expression>"` (#2068).
///
/// One REPL turn against a fresh session through the one statement-turn
/// mechanism (`run_stmts_turn`): `classify` → `type_check_stmts` → built and
/// executed by the session executor. A command-line expression and a typed
/// REPL line therefore cannot disagree (I8/I9); this only marshals.
///
/// No `EffectPrompt` is attached, so a right no `--allow` covers is refused
/// with E1803 rather than blocking a non-interactive command on stdin —
/// `jet eval` is the pure/deterministic verb (S60 / D-PURE1).
pub fn eval_once(src: &str, base_dir: &Path, flags: ReplFlags) -> Result<EvalOnce, EvalError> {
    let mut policy = ReplPolicy::new(flags, base_dir);
    let mut session = Session::new();
    session.step = 1;
    let trimmed = src.trim();
    let (stmts, suppress, check_src) = match classify(trimmed, 1)? {
        InputKind::Stmts(stmts, suppress, check_src) => (stmts, suppress, check_src),
        // A declaration or import on its own is checked and accepted; it
        // simply has no value to show, exactly like a `run()` that returns
        // nothing. A `:meta` command never reaches here — the CLI rejects it
        // as a malformed argument before calling in.
        InputKind::Meta(..) | InputKind::Item(_) | InputKind::Import(_) | InputKind::Empty => {
            return Ok(EvalOnce {
                value: None,
                stdout: String::new(),
                stderr: String::new(),
            });
        }
    };
    let mut authorizer = policy.authorizer(None);
    match run_stmts_turn(
        &mut session,
        &stmts,
        suppress,
        &check_src,
        trimmed,
        base_dir,
        &mut authorizer,
        true,
    ) {
        TurnResult::Done {
            stdout,
            stderr,
            echo,
            ..
        } => Ok(EvalOnce {
            value: echo.map(|value| EvalValue {
                json: value.json.clone().unwrap_or_else(|| "null".to_string()),
                ty: value.ty,
                shown: value.shown,
            }),
            stdout,
            stderr,
        }),
        TurnResult::Interrupted => Err(EvalError {
            diagnostics: Vec::new(),
            report: Some("Interrupted.\n".to_string()),
            stdout: String::new(),
            stderr: String::new(),
        }),
        TurnResult::Failed {
            stdout,
            stderr,
            diagnostics,
            report,
        } => Err(EvalError {
            diagnostics,
            report,
            stdout,
            stderr,
        }),
    }
}

// ── transcript test harness (D-REPL20=A) ──────────────────────────────────

/// Run a REPL transcript test: feed `inputs` through the session, collect
/// the output lines. Used by `tests/repl.rs`. No color, no interactive I/O.
///
/// Each input should be a single REPL input (may be multi-line if the caller
/// joins them). The returned string is the combined stdout of the session.
pub fn run_transcript(inputs: &[&str], project_dir: Option<&str>) -> String {
    run_transcript_with_flags(inputs, project_dir, &[], &[])
}

/// Deterministic non-TTY transcript with explicit invocation policy. Used to
/// prove CLI flag behavior without inventing a test-only authorization path.
pub fn run_transcript_with_flags(
    inputs: &[&str],
    project_dir: Option<&str>,
    allow: &[&str],
    deny: &[&str],
) -> String {
    run_transcript_with(
        inputs,
        project_dir,
        ReplFlags {
            allow: allow.iter().map(|s| s.to_ascii_lowercase()).collect(),
            deny: deny.iter().map(|s| s.to_ascii_lowercase()).collect(),
            color: ColorChoice::Never,
            preload: None,
        },
    )
}

/// #2038: the `jet repl <file>.jet` floor. Same session engine, same loader —
/// the transcript just starts with the named file already loaded.
pub fn run_transcript_with_preload(
    inputs: &[&str],
    project_dir: Option<&str>,
    preload: &str,
) -> String {
    run_transcript_with(
        inputs,
        project_dir,
        ReplFlags {
            color: ColorChoice::Never,
            preload: Some(preload.to_string()),
            ..Default::default()
        },
    )
}

fn run_transcript_with(inputs: &[&str], project_dir: Option<&str>, flags: ReplFlags) -> String {
    jet_driver::boot_mir_eval();
    let base_dir: std::path::PathBuf = project_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let mut session = Session::new();
    let preload = flags.preload.clone();
    let mut policy = ReplPolicy::new(flags, &base_dir);
    let mut out = String::new();

    // D-REPL10=A: load project items when --project is set.
    if let Some(dir) = project_dir {
        let mut buf = Vec::new();
        load_project_items(Path::new(dir), &mut session, &mut buf);
        out.push_str(&String::from_utf8_lossy(&buf));
    }

    // #2038: the command-line file, through the same `cmd_load` the real
    // loops use — `fn run` is not invoked.
    if let Some(file) = &preload {
        let mut buf = Vec::new();
        let mut authorizer = policy.authorizer(None);
        cmd_load(file, &mut session, &base_dir, false, &mut authorizer, &mut buf);
        out.push_str(&String::from_utf8_lossy(&buf));
    }

    for &input in inputs {
        let normalized = normalize_repl_input(input);
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            continue;
        }

        session.step += 1;

        let kind = match classify(trimmed, session.step) {
            Ok(k) => k,
            Err(ds) => {
                for d in &ds {
                    out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                }
                session.record_turn(
                    trimmed,
                    ReplTurnStatus::Error,
                    ds.iter()
                        .map(|d| format!("{}: {}", d.code, d.what))
                        .collect::<Vec<_>>()
                        .join("; "),
                );
                continue;
            }
        };

        match kind {
            InputKind::Empty => {}

            InputKind::Meta(cmd, arg) => {
                match cmd.as_str() {
                    "quit" | "q" | "exit" => {
                        out.push_str("bye\n");
                        return out;
                    }
                    "reset" => {
                        session.reset();
                        policy.authorizer(None).reset_session();
                        out.push_str("session reset\n");
                    }
                    "load" => {
                        let path = match arg.as_deref() {
                            Some(p) => p.to_string(),
                            None => {
                                out.push_str("error: :load needs a file path\n");
                                continue;
                            }
                        };
                        let mut buf = Vec::new();
                        let mut authorizer = policy.authorizer(None);
                        cmd_load(&path, &mut session, &base_dir, true, &mut authorizer, &mut buf);
                        out.push_str(&String::from_utf8_lossy(&buf));
                    }
                    "type" => {
                        let name = match arg.as_deref() {
                            Some(n) => n,
                            None => {
                                out.push_str("error: :type needs a name\n");
                                continue;
                            }
                        };
                        if session.scope.contains_key(name) {
                            let ty = session
                                .binding_types
                                .get(name)
                                .map(|t| t.name())
                                .unwrap_or_else(|| "?".to_string());
                            out.push_str(&format!("{} : {}\n", name, ty));
                        } else if session.func_defs.contains_key(name) {
                            out.push_str(&format!("{} : fn\n", name));
                        } else {
                            out.push_str(&format!(
                                "note: `{}` isn't defined in this session\n",
                                name
                            ));
                        }
                    }
                    "run" => {
                        // The whole session as one built program (D-EXEC2).
                        let run_out = cmd_run(&session, &base_dir, &mut policy.authorizer(None));
                        out.push_str(&run_out);
                    }
                    "turns" => out.push_str(&render_turns_text(&session)),
                    "fold" => {
                        match parse_turn_id(arg.as_deref())
                            .and_then(|id| set_turn_flag(&mut session, id, "folded", true))
                        {
                            Ok(()) => out.push_str("turn folded\n"),
                            Err(msg) => out.push_str(&format!("{msg}\n")),
                        }
                    }
                    "unfold" => {
                        match parse_turn_id(arg.as_deref())
                            .and_then(|id| set_turn_flag(&mut session, id, "folded", false))
                        {
                            Ok(()) => out.push_str("turn unfolded\n"),
                            Err(msg) => out.push_str(&format!("{msg}\n")),
                        }
                    }
                    "pin" => {
                        match parse_turn_id(arg.as_deref())
                            .and_then(|id| set_turn_flag(&mut session, id, "pinned", true))
                        {
                            Ok(()) => out.push_str("turn pinned\n"),
                            Err(msg) => out.push_str(&format!("{msg}\n")),
                        }
                    }
                    "unpin" => {
                        match parse_turn_id(arg.as_deref())
                            .and_then(|id| set_turn_flag(&mut session, id, "pinned", false))
                        {
                            Ok(()) => out.push_str("turn unpinned\n"),
                            Err(msg) => out.push_str(&format!("{msg}\n")),
                        }
                    }
                    "rerun" => match parse_turn_id(arg.as_deref()) {
                        Ok(id) => {
                            if let Some(input) = session
                                .turns
                                .iter()
                                .find(|t| t.id == id)
                                .map(|t| t.input.clone())
                            {
                                out.push_str(&format!("rerun #{id}: {input}\n"));
                                out.push_str(&run_transcript(&[input.as_str()], project_dir));
                            } else {
                                out.push_str(&format!("turn #{id} does not exist\n"));
                            }
                        }
                        Err(msg) => out.push_str(&format!("{msg}\n")),
                    },
                    "history" => match arg.as_deref() {
                        Some("clear") => {
                            session.history.clear().ok();
                            out.push_str("History cleared.\n");
                        }
                        Some(arg) if arg.starts_with("search ") => {
                            let needle = &arg[7..];
                            let matches = session.history.search(needle);
                            if matches.is_empty() {
                                out.push_str("No history matches.\n");
                            } else {
                                for entry in matches {
                                    out.push_str(entry);
                                    out.push('\n');
                                }
                            }
                        }
                        _ => out.push_str("usage: :history search <text> | :history clear\n"),
                    },
                    "?" => {
                        // D-FE-REPL-DOCS1=B: same shared-docs-index lookup the
                        // cooked/interactive loops use (`Docs::lookup`), so
                        // `?name`/`:? name` behave identically everywhere.
                        if let Some(name) = arg.as_deref() {
                            match Docs::lookup(&session, name) {
                                Some(text) => out.push_str(&text),
                                None => out.push_str(&format!(
                                    "note: `{}` isn't defined in this session\n",
                                    name
                                )),
                            }
                        } else {
                            out.push_str("REPL Meta-Commands\n");
                        }
                    }
                    "help" => out.push_str("REPL Meta-Commands\n"),
                    _ => out.push_str(&format!("unknown meta-command `:{}`\n", cmd)),
                }
            }

            InputKind::Item(src) => {
                let bundle = match type_check_item(&session, &src) {
                    Ok(bundle) => bundle,
                    Err(errors) => {
                        for d in &errors {
                            out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                        }
                        session.record_turn(
                            trimmed,
                            ReplTurnStatus::Error,
                            errors
                                .iter()
                                .map(|d| format!("{}: {}", d.code, d.what))
                                .collect::<Vec<_>>()
                                .join("; "),
                        );
                        continue;
                    }
                };
                session.item_srcs.push(src);
                rebuild_funcs(&mut session);
                update_core_imports_from_ledger(&bundle, &mut session.core_imports);
                out.push_str("ok\n");
                session.record_turn(trimmed, ReplTurnStatus::Ok, "ok".to_string());
                session.remember_success(trimmed);
            }

            InputKind::Import(src) => {
                session.import_srcs.push(src.clone());
                let bundle = match type_check_item(&session, "") {
                    Ok(bundle) => bundle,
                    Err(errors) => {
                        session.import_srcs.pop();
                        for d in &errors {
                            out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                        }
                        session.record_turn(
                            trimmed,
                            ReplTurnStatus::Error,
                            errors
                                .iter()
                                .map(|d| format!("{}: {}", d.code, d.what))
                                .collect::<Vec<_>>()
                                .join("; "),
                        );
                        continue;
                    }
                };
                rebuild_funcs(&mut session);
                update_core_imports_from_ledger(&bundle, &mut session.core_imports);
                out.push_str("ok\n");
                session.record_turn(trimmed, ReplTurnStatus::Ok, "ok".to_string());
                session.remember_success(trimmed);
            }

            InputKind::Stmts(stmts, suppress, check_src) => {
                let mut authorizer = policy.authorizer(None);
                match run_stmts_turn(
                    &mut session,
                    &stmts,
                    suppress,
                    &check_src,
                    trimmed,
                    &base_dir,
                    &mut authorizer,
                    false,
                ) {
                    TurnResult::Done {
                        stdout,
                        stderr,
                        echo,
                        bound_name,
                        had_effect,
                        ..
                    } => {
                        let mut summary = format!("{stdout}{stderr}");
                        out.push_str(&summary);
                        if let Some(value) = echo {
                            let shown = echo_text(&value);
                            out.push_str(&format!("{shown}\n"));
                            summary.push_str(&shown);
                        }
                        session.record_turn_ex(
                            trimmed,
                            ReplTurnStatus::Ok,
                            summary,
                            had_effect,
                            bound_name,
                        );
                        session.remember_success(trimmed);
                    }
                    TurnResult::Interrupted => {
                        out.push_str("Interrupted. External effects already performed were not rolled back.\n");
                        session.record_turn(trimmed, ReplTurnStatus::Interrupted, "Interrupted".to_string());
                    }
                    TurnResult::Failed {
                        stdout,
                        stderr,
                        diagnostics,
                        report,
                    } => {
                        out.push_str(&stdout);
                        out.push_str(&stderr);
                        out.push_str(&failure_text(&diagnostics, report.as_deref()));
                        session.record_turn(
                            trimmed,
                            ReplTurnStatus::Error,
                            turn_failure_summary(&diagnostics, report.as_deref()),
                        );
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingPrompt(usize, PromptChoice);

    impl EffectPrompt for CountingPrompt {
        fn choose(&mut self, _: &ReplEffectRequest, _: bool) -> PromptChoice {
            self.0 += 1;
            self.1
        }
    }

    #[test]
    fn undecided_right_without_prompt_is_e1803_and_never_prompts() {
        let mut policy = ReplPolicy::new(ReplFlags::default(), Path::new("."));
        let mut authorization = policy.authorizer(None);
        let error = authorization
            .decide(&["FS.Write".to_string()], "write_file(\"x\", \"y\")")
            .expect_err("no prompt means refusal");
        assert_eq!(error.code, "E1803");
        assert!(error.fix.contains("--allow=FS.Write"), "{}", error.fix);
    }

    #[test]
    fn explicit_deny_wins_before_any_prompt() {
        let flags = ReplFlags::new(&[], &["FS".to_string()]);
        let mut policy = ReplPolicy::new(flags, Path::new("."));
        let mut prompt = CountingPrompt(0, PromptChoice::Once);
        let error = policy
            .authorizer(Some(&mut prompt))
            .decide(&["FS.Write".to_string()], "turn")
            .expect_err("deny wins");
        assert_eq!(error.code, "E1803");
        assert_eq!(prompt.0, 0, "a denied right must not prompt");
    }

    #[test]
    fn session_approval_is_forwarded_to_later_turns() {
        let mut policy = ReplPolicy::new(ReplFlags::default(), Path::new("."));
        let mut prompt = CountingPrompt(0, PromptChoice::Session);
        let approved = policy
            .authorizer(Some(&mut prompt))
            .decide(&["FS.Read".to_string()], "turn")
            .expect("approved");
        assert_eq!(approved, vec!["FS.Read".to_string()]);
        let rights = policy.authorizer(None).rights(&[]);
        assert_eq!(rights.allow, vec!["FS.Read".to_string()]);
        policy.authorizer(None).reset_session();
        assert!(policy.authorizer(None).rights(&[]).allow.is_empty());
    }

    #[test]
    fn authority_answer_matches_compile_build_session_and_repl() {
        let written = "IO";
        let external_spelling = "io";
        let expected = jet_foundation::Authority::parse_right(written).expect("known right");
        assert_eq!(expected, "IO");

        let compile =
            crate::Sema::Effects::parse_effect_name(external_spelling).expect("compile right");
        assert_eq!(compile, expected);
        assert!(crate::Sema::Effects::effect_covers(written, &expected));

        let session = answer(
            &Holds::from([external_spelling.to_string()]),
            &Holds::new(),
            written,
        );
        assert_eq!(session, Verdict::Allowed);

        let flags = ReplFlags::new(&[external_spelling.to_string()], &[]);
        assert!(flags.allow.contains(&expected));
        let mut policy = ReplPolicy::new(flags, Path::new("."));
        assert_eq!(policy.authorizer(None).rights(&[]).allow, vec![expected]);
    }

    #[cfg(unix)]
    #[test]
    fn repl_temp_writer_rejects_existing_symlink() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("jet-repl-temp-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("outside.txt");
        let link = root.join("session.jet");
        std::fs::write(&target, "must survive\n").unwrap();
        symlink(&target, &link).unwrap();

        assert!(write_temp_file_at(&link, "attacker\n").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "must survive\n");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn repl_nofollow_flag_uses_linux_family_value() {
        assert_eq!(REPL_O_NOFOLLOW, 0o400000);
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn repl_nofollow_flag_uses_macos_family_value() {
        assert_eq!(REPL_O_NOFOLLOW, 0x0100);
    }

    #[cfg(all(
        unix,
        not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))
    ))]
    #[test]
    fn repl_temp_writer_rejects_unsupported_unix() {
        let path =
            std::env::temp_dir().join(format!("jet-repl-unsupported-{}.jet", std::process::id()));
        let error = write_temp_file_at(&path, "must not be written\n")
            .err()
            .expect("unsupported Unix target must fail closed");
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    }

    #[test]
    fn sigil_binding_is_classified_as_statement() {
        let kind = classify("t :: s", 1).expect("classify");
        match kind {
            InputKind::Stmts(stmts, suppress, check_src) => {
                assert!(!suppress);
                assert_eq!(check_src, "t :: s;");
                assert!(matches!(stmts.as_slice(), [Stmt::Val(_)]));
            }
            _ => panic!("expected statement input"),
        }
    }

    #[test]
    fn sigil_binding_from_string_session_name_marks_move() {
        let kind = classify("t :: s", 1).expect("classify");
        let InputKind::Stmts(stmts, _, _) = kind else {
            panic!("expected statement input");
        };
        let mut names = HashSet::new();
        names.insert("s".to_string());
        let mut scope = HashMap::new();
        scope.insert("s".to_string(), Type::String);
        let moved = collect_moved_names(&stmts, &names, &scope);
        assert!(moved.contains("s"), "moved names: {:?}", moved);
    }

    #[test]
    fn bare_binding_is_preserved_as_an_exact_statement() {
        let kind = classify("s :: \"\"", 1).expect("classify");
        let InputKind::Stmts(stmts, _, check_src) = kind else {
            panic!("expected statement input");
        };
        assert_eq!(check_src, "s :: \"\";");
        assert!(matches!(stmts.as_slice(), [Stmt::Val(_)]));
    }

    #[test]
    fn notebook_item_classifier_uses_the_parser_item_plane() {
        assert!(is_item_input("@answer :: 42"));
        assert!(is_item_input("#Test(\"answer\") { assert(true) }"));
        assert!(!is_item_input("answer :: 42"));
    }

    #[test]
    fn raw_multiline_completeness_comes_from_repl_parser_shapes() {
        assert!(raw_input_is_complete("40 + 2"));
        assert!(raw_input_is_complete("answer :: = 42"));
        assert!(!raw_input_is_complete("40 +"));
        assert!(!raw_input_is_complete("answer ::"));
        assert!(!raw_input_is_complete("fn total(xs: [Int]) -> Int {"));
        assert!(!raw_input_is_complete("if true {\n    1"));
        assert!(!raw_input_is_complete("/* unfinished"));
        assert!(!raw_input_is_complete("\"\"\"\nunfinished"));
        assert!(raw_input_is_complete("\"single-line text\ncan't continue"));
    }
}
