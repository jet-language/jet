//! E2-M18 — `jet repl`: interactive REPL session.
//!
//! All decisions ratified 2026-06-16/17:
//!   D-REPL4=A   interpreter only (reuses comptime tree-walker, I2)
//!   D-REPL5=A   expressions + top-level statements; no FFI/tasks
//!   D-REPL7=C   one accumulating module (default)
//!   D-REPL8=A   real move semantics across inputs
//!   D-REPL9=A   brace/paren/bracket balance → `...` secondary prompt
//!   D-REPL11=A  std-only; no external line-editing crate (I6)
//!   D-REPL15=B  meta-commands: `:quit`, `:reset`, `:load`, `:type`, `:help`
//!   D-REPL16=B  `x : T = v` echo for last expression; `;` suppresses
//!   D-REPL17=A  diagnostics byte-identical to batch compiler (I4)
//!   D-REPL-FUEL=A   ~10M steps/input; E1801 on overshoot
//!   D-REPL-BANNER=A banner + `:help` hint on startup
//!   D-REPL-COLOR=A  respect `NO_COLOR`/`CLICOLOR`
//!   D-REPL-PRELOAD=A auto-import `core.io`; teaching note on first use
//!
//! Error codes (E18xx):
//!   E1801  fuel cap hit — snippet ran too long
//!   E1802  hard-reject: feature not available in the REPL interpreter

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::Comptime::{CtValue, DevSink, REPL_FUEL_BUDGET};
use crate::Diagnostics::Diagnostic;
use crate::AST::{AccessConvention, CallArg, Expr, Func, Item, Stmt};

// ── color helpers ──────────────────────────────────────────────────────────

/// Return true if color output is desired on stdout.
/// Respects `NO_COLOR` and `CLICOLOR` (D-REPL-COLOR=A).
pub fn color_on() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Ok(v) = std::env::var("CLICOLOR") {
        if v == "0" {
            return false;
        }
    }
    use std::io::IsTerminal;
    io::stdout().is_terminal()
}

fn bold(s: &str, color: bool) -> String {
    if color {
        format!("\x1b[1m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

fn dim(s: &str, color: bool) -> String {
    if color {
        format!("\x1b[2m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

fn green(s: &str, color: bool) -> String {
    if color {
        format!("\x1b[32m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

fn yellow(s: &str, color: bool) -> String {
    if color {
        format!("\x1b[33m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

// ── REPL diagnostics (E18xx) ───────────────────────────────────────────────

/// E1801: per-input fuel cap hit (D-REPL-FUEL=A).
pub fn e1801(steps: u64) -> Diagnostic {
    Diagnostic::error(
        "E1801",
        format!(
            "this snippet ran more than {} interpreter steps without finishing",
            steps
        ),
        "the REPL interpreter caps each input to avoid hanging your session; \
         this almost always means a loop that never ends"
            .to_string(),
        "check any loops for a condition that never becomes false; \
         use `:run` to allow unbounded execution (compiles and runs instead of interpreting)"
            .to_string(),
        None,
    )
}

/// E1802: feature hard-rejected by the REPL interpreter (D-REPL6=A).
pub fn e1802(feature: &str) -> Diagnostic {
    Diagnostic::error(
        "E1802",
        format!("the REPL interpreter can't run {}", feature),
        "the REPL is an interpreter for learning Jet; some features — \
         FFI, tasks/channels, `#Unsafe`, and OS-level APIs — require the real compiler"
            .to_string(),
        "run `jet run <file.jet>` or `jet build <file.jet>` to use the full compiler".to_string(),
        None,
    )
}

// ── session state ─────────────────────────────────────────────────────────

/// A single accumulated REPL session. All state is in memory (REPL-I4/I5).
///
/// Session stores item declarations as raw source text; it re-parses them
/// on each type-check step to build a fresh `Program` (avoids needing
/// `Clone` on every AST type). The interpreter scope is a live
/// `HashMap<name, CtValue>` that survives across inputs (D-REPL7).
pub struct Session {
    /// Raw source text for each accumulated top-level item declaration.
    /// Re-parsed on each sema check step so the full item list is visible.
    pub item_srcs: Vec<String>,
    /// D-REPL10/S16: accumulated `use …;` import lines. Prepended to every
    /// synthetic program so an import typed in one input (e.g.
    /// `use core.math as math`) keeps resolving in later inputs.
    pub import_srcs: Vec<String>,
    /// Accumulated `val` binding sources: `val x: Int = 0;` style lines.
    /// These are added after each successful `val` step so sema sees them.
    pub binding_srcs: Vec<String>,
    /// The interpreter's function table: all `fn` items by name.
    /// Rebuilt when a new function is added.
    pub func_defs: HashMap<String, Func>,
    /// Live interpreter scope: accumulated `val`/`var` bindings (D-REPL7).
    pub scope: HashMap<String, CtValue>,
    /// Whether the teaching note for `print` (D-REPL-PRELOAD) has been shown.
    pub shown_preload_note: bool,
    /// Input counter — used in synthetic spans (diagnostics say `<repl:N>`).
    pub step: usize,
    /// D-REPL8=A: bindings moved/consumed in a prior input. Excluded from
    /// binding stubs so sema sees them as undefined in later inputs.
    pub moved_names: HashSet<String>,
    /// Accumulated raw statement source text for each successfully executed
    /// statement input. Used by `:run` to materialize a `run()` body.
    pub stmt_srcs: Vec<String>,
    /// D-CTCORE1: alias → Core module path (e.g. `"math"` → `"core.math"`),
    /// derived from successfully accepted `use` declarations. Passed to
    /// `run_repl_step` so the comptime interpreter can execute whitelisted
    /// pure Core calls (e.g. `math.sqrt(16.0)`) inline instead of E0956.
    pub core_imports: HashMap<String, String>,
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
            binding_srcs: Vec::new(),
            func_defs: HashMap::new(),
            scope: HashMap::new(),
            shown_preload_note: false,
            step: 0,
            moved_names: HashSet::new(),
            stmt_srcs: Vec::new(),
            core_imports: HashMap::new(),
        }
    }

    pub fn reset(&mut self) {
        self.item_srcs.clear();
        self.import_srcs.clear();
        self.binding_srcs.clear();
        self.func_defs.clear();
        self.scope.clear();
        self.step = 0;
        self.moved_names.clear();
        self.stmt_srcs.clear();
        self.core_imports.clear();
        // Keep shown_preload_note — no need to repeat the teaching note.
    }

    /// Build the accumulated item declarations source text (functions, structs…).
    /// This is inserted at program top-level, so `val` bindings are NOT here.
    fn accumulated_src(&self) -> String {
        self.item_srcs.join("\n")
    }

    /// Accumulated `use …;` import lines, one per line, with a trailing newline
    /// when non-empty so they sit cleanly above the items in a synthetic program.
    fn import_src(&self) -> String {
        if self.import_srcs.is_empty() {
            String::new()
        } else {
            format!("{}\n", self.import_srcs.join("\n"))
        }
    }

    /// Build the accumulated val-binding stubs for insertion INSIDE a function body.
    /// Bindings moved in prior inputs are re-declared then immediately consumed
    /// by a synthetic assignment, so sema reports E0121 ("was given away") if the
    /// current input tries to use them (D-REPL8=A).
    fn binding_stub_name(stub: &str) -> &str {
        let before_marker = stub
            .split_once("::")
            .map(|(name, _)| name)
            .or_else(|| stub.split_once(":=").map(|(name, _)| name))
            .unwrap_or(stub);
        before_marker
            .split_once(':')
            .map(|(name, _)| name)
            .unwrap_or(before_marker)
            .trim()
    }

    fn binding_stubs_src(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for stub in &self.binding_srcs {
            let name = Self::binding_stub_name(stub);
            if self.moved_names.contains(name) {
                // Re-declare, then synthetically consume so sema sees it moved.
                lines.push(stub.clone());
                // `__moved_<name>__ :: name` — a binding whose init is `name`
                // (a non-scalar Ident). Sema's `note_move_if_direct_ident` marks
                // `name` as moved immediately after this declaration.
                lines.push(format!("__moved_{}__ :: {};", name, name));
            } else {
                lines.push(stub.clone());
            }
        }
        lines.join("\n")
    }

    /// Register a val binding for sema visibility after it was evaluated.
    /// `mutable` must match the original `::` / `:=` sigil (D-BIND4).
    pub fn record_binding(&mut self, name: &str, v: &CtValue, mutable: bool) {
        let sigil = if mutable {
            crate::Syntax::SIGIL_BIND_MUT
        } else {
            crate::Syntax::SIGIL_BIND_IMMUT
        };
        // Synthetic `name: Type {sigil} zero_val` so later inputs type-check.
        let type_and_val = match v {
            CtValue::Int(_) => format!(": Int {} 0", sigil),
            CtValue::Float(_) => format!(": Float {} 0.0", sigil),
            CtValue::Bool(_) => format!(": Bool {} false", sigil),
            CtValue::Char(_) => format!(": Char {} 'a'", sigil),
            CtValue::Str(_) => format!(": String {} \"\"", sigil),
            CtValue::Bytes(_) => format!(": [U8] {} []", sigil),
            CtValue::List(_) => format!(": List<Int> {} []", sigil),
            CtValue::Map(_) => format!(": Map<String, Int> {} [:]", sigil),
            CtValue::Some(_) | CtValue::None(_) => return, // skip Option for now
            CtValue::ResOk(_) | CtValue::ResErr(_) => return, // skip Result for now
            CtValue::Struct { .. } => {
                // We can't easily construct a dummy struct; skip sema pre-check
                // for struct bindings. The interpreter scope still has the value.
                return;
            }
            CtValue::Enum { .. } => return,
            CtValue::Unit => return,
            // c139: no sema pre-check stub for a closure value either — same
            // reasoning as `Struct`/`Enum` above.
            CtValue::Closure(_) => return,
        };
        self.binding_srcs.push(format!("{}{}", name, type_and_val));
    }
}

// ── move detection (D-REPL8=A) ─────────────────────────────────────────────

/// Whether a `Stmt::Val` in this step introduced `name` as mutable (`:=`).
fn val_binding_mutable(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|stmt| {
        matches!(
            stmt,
            Stmt::Val(b) if b.name == name && b.mutable
        )
    })
}

/// Scan the successfully-parsed stmts for names that were moved from the
/// session's current bindings. A move occurs when:
///
/// 1. `val t = s` (or `t :: s`) — the init is a bare identifier that names a
///    session binding of non-scalar type. Sema's `note_move_if_direct_ident`
///    marks non-scalar bindings moved at this point.
/// 2. A call argument uses `take name` convention — `CallArg::convention == Move`
///    with an `Ident` expr naming a session binding.
///
/// `scope` is consulted to determine if a binding is scalar (Int/Float/Bool/Char)
/// — scalars are copy, not moved.
///
/// We only scan depth-1 (the stmt list passed to the REPL step). Nested moves
/// inside `fn` bodies are not relevant — they stay within the item's own scope.
fn collect_moved_names(
    stmts: &[Stmt],
    session_bindings: &HashSet<String>,
    scope: &HashMap<String, CtValue>,
) -> HashSet<String> {
    let mut moved = HashSet::new();
    for stmt in stmts {
        collect_moved_in_stmt(stmt, session_bindings, scope, &mut moved);
    }
    moved
}

/// Return true if a CtValue is scalar (copy semantics, not moved).
fn ct_value_is_scalar(v: &CtValue) -> bool {
    matches!(
        v,
        CtValue::Int(_) | CtValue::Float(_) | CtValue::Bool(_) | CtValue::Char(_)
    )
}

fn collect_moved_in_stmt(
    stmt: &Stmt,
    session_bindings: &HashSet<String>,
    scope: &HashMap<String, CtValue>,
    moved: &mut HashSet<String>,
) {
    match stmt {
        Stmt::Val(b) => {
            // `val t = s` — if init is a bare Ident naming a non-scalar session
            // binding, that's a move (sema marks it via note_move_if_direct_ident).
            if let Expr::Ident(name, _) = &b.init {
                if session_bindings.contains(name.as_str()) {
                    // Only non-scalars are moved.
                    let is_scalar = scope
                        .get(name.as_str())
                        .map(ct_value_is_scalar)
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
        Stmt::Expr(e) => {
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
    scope: &HashMap<String, CtValue>,
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
    scope: &HashMap<String, CtValue>,
    moved: &mut HashSet<String>,
) {
    if arg.convention == AccessConvention::Move {
        if let Expr::Ident(name, _) = &arg.expr {
            if session_bindings.contains(name.as_str()) {
                let is_scalar = scope
                    .get(name.as_str())
                    .map(ct_value_is_scalar)
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
fn unique_temp_name(tag: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!("__jet_repl_{}_{}_{}.jet", tag, std::process::id(), n)
}

// ── :run ───────────────────────────────────────────────────────────────────

/// Materialize the session + stmt_srcs to a temp `.jet` file and run it
/// natively via `jet run` (D-REPL-FUEL=A). Bypasses the interpreter fuel cap.
/// When the session is empty, reports a no-op note.
fn cmd_run_native(session: &Session, color: bool, out_sink: &mut impl Write) {
    if session.stmt_srcs.is_empty() && session.item_srcs.is_empty() {
        let _ = writeln!(out_sink, "note: session is empty — nothing to run");
        return;
    }

    // Materialize: imports + items + a run() that replays statement inputs.
    let import_src = session.import_src();
    let item_src = session.item_srcs.join("\n");
    let stmt_body = session.stmt_srcs.join("\n");
    let jet_src = if stmt_body.trim().is_empty() {
        format!("{}{}\nfn run() {{}}\n", import_src, item_src)
    } else {
        format!(
            "{}{}\nfn run() {{\n{}\n}}\n",
            import_src, item_src, stmt_body
        )
    };

    // Write to a temp file.
    let tmp_path = std::env::temp_dir().join(unique_temp_name("run"));
    if let Err(e) = std::fs::write(&tmp_path, &jet_src) {
        let _ = writeln!(out_sink, "error: couldn't write temp file: {}", e);
        return;
    }

    let _ = write!(out_sink, "{}", dim("compiling session…", color));
    let _ = writeln!(out_sink, " {}", dim("running…", color));

    // Spawn `jet run <temp>` using the current executable. This reuses the
    // full compile+rustc pipeline without duplicating it in the library crate.
    let jet_bin = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("jet"));

    let result = std::process::Command::new(&jet_bin)
        .arg("run")
        .arg(&tmp_path)
        .output();

    let _ = std::fs::remove_file(&tmp_path);

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let _ = write!(out_sink, "{}", stdout);
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.is_empty() {
                    let _ = write!(out_sink, "{}", stderr);
                }
            }
        }
        Err(e) => {
            let _ = writeln!(out_sink, "error: couldn't invoke jet run: {}", e);
        }
    }
}

/// Transcript-test variant of `:run`: materializes the session and interprets
/// the materialized `run()` with unlimited fuel (no cap). Used in tests so
/// we can verify `:run` semantics without invoking rustc.
fn cmd_run_transcript(session: &Session) -> String {
    if session.stmt_srcs.is_empty() && session.item_srcs.is_empty() {
        return "note: session is empty — nothing to run\n".to_string();
    }

    let import_src = session.import_src();
    let item_src = session.item_srcs.join("\n");
    let stmt_body = session.stmt_srcs.join("\n");
    let jet_src = if stmt_body.trim().is_empty() {
        format!("{}{}\nfn run() {{}}\n", import_src, item_src)
    } else {
        format!(
            "{}{}\nfn run() {{\n{}\n}}\n",
            import_src, item_src, stmt_body
        )
    };

    // Parse and interpret the materialized source.
    let (toks, lex_diags) = crate::Lexer::lex(&jet_src);
    if !lex_diags.is_empty() {
        return format!(
            "error: materialization parse failed: {:?}\n",
            lex_diags.first().map(|d| &d.what)
        );
    }
    let prog = match crate::Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => {
            return format!(
                "error: could not build this value: {}\n",
                ds.first().map(|d| d.what.as_str()).unwrap_or("?")
            )
        }
    };
    // Find run() and run it through the interpreter with DEV_FUEL_BUDGET.
    let mut func_defs: HashMap<String, Func> = HashMap::new();
    for item in &prog.items {
        if let Item::Func(f) = item {
            func_defs.insert(f.name.clone(), f.clone());
        }
    }
    let funcs: HashMap<String, &Func> = func_defs.iter().map(|(k, v)| (k.clone(), v)).collect();
    let Some(main) = func_defs.get("run") else {
        return "note: no run in materialized session\n".to_string();
    };
    let main_clone = main.clone();
    let base_dir = std::path::PathBuf::from(".");
    let mut sink = DevSink::new();
    // Use a very large fuel budget (but still finite) for :run.
    const RUN_FUEL: u64 = 1_000_000_000;
    match crate::Comptime::run_repl_main_with_fuel(
        &main_clone,
        &funcs,
        &base_dir,
        &mut sink,
        RUN_FUEL,
        &session.core_imports,
    ) {
        Ok(()) => sink.stdout,
        Err(d) => format!("error [{}]: {}\n", d.code, d.what),
    }
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
            match crate::Parser::parse(&toks) {
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
    /// Hard-reject with an E1802 message naming the feature.
    Reject(String),
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
    print!("{}", bold(prompt, color));
    io::stdout().flush().ok();
    let mut buf = String::new();
    if stdin.read_line(&mut buf).ok()? == 0 {
        return None; // EOF
    }
    let trimmed = buf
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .to_string();

    // D-REPL9: keep reading if brackets are unbalanced.
    let mut bal = bracket_balance(&trimmed);
    let mut full = trimmed;
    while bal > 0 {
        print!("{}", dim("...  ", color));
        io::stdout().flush().ok();
        let mut cont = String::new();
        if stdin.read_line(&mut cont).ok()? == 0 {
            break;
        }
        let line = cont.trim_end_matches('\n').trim_end_matches('\r');
        bal += bracket_balance(line);
        full.push('\n');
        full.push_str(line);
    }
    Some(full)
}

/// Detect whether the text starts with a keyword that only makes sense as a
/// top-level item declaration (not a statement).
fn looks_like_item(text: &str) -> bool {
    let t = text.trim_start();
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

/// D-CTCORE1: parse one `use …;` source line and add any core module alias →
/// path entries to `map`. Called after a `use` import is accepted into the
/// session so `run_repl_step` can resolve whitelisted pure Core calls inline.
fn update_core_imports(import_src: &str, map: &mut HashMap<String, String>) {
    let (toks, _) = crate::Lexer::lex(import_src);
    if let Ok(prog) = crate::Parser::parse(&toks) {
        for imp in &prog.imports {
            if let Some(module) = crate::Loader::core_module_path(imp) {
                let alias = crate::Loader::import_alias(imp);
                map.insert(alias, module);
            }
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

/// Detect hard-reject features (D-REPL6=A).
fn reject_feature(text: &str) -> Option<&'static str> {
    let t = text.trim();
    if t.contains("#Unsafe") {
        return Some("`#Unsafe`");
    }
    if t.contains("extern rust") {
        return Some("`extern rust`");
    }
    if t.contains("#Extern") || t.contains("#Bindgen") {
        return Some("C-FFI (`#Extern`/`#Bindgen`)");
    }
    if t.contains("core.tasks") || t.contains("core.channels") {
        return Some("tasks/channels (`core.tasks`)");
    }
    if t.contains("core.mem") {
        return Some("`core.mem` (low-level memory tier)");
    }
    if t.contains("core.http") || t.contains("jet.http") {
        return Some("the HTTP client/server (`core.http`)");
    }
    if t.contains("core.db") || t.contains("jet.db") {
        return Some("`core.db` (SQLite)");
    }
    if t.contains("core.net") {
        return Some("network sockets (`core.net`)");
    }
    if t.contains("core.archive") || t.contains("jet.archive") {
        return Some("`core.archive`");
    }
    if t.contains("core.reactive") || t.contains("jet.reactive") {
        return Some("`core.reactive`");
    }
    if t.contains("core.crypto") || t.contains("jet.crypto") {
        return Some("`core.crypto`");
    }
    if t.contains("jet.log") {
        return Some("`core.log`");
    }
    if t.contains("core.term") || t.contains("live {") || t.contains("live{") {
        return Some("`core.term` / `live` blocks (terminal direct-input)");
    }
    None
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

    // Hard rejects (D-REPL6=A) — these fire before imports so e.g.
    // `use core.fs as fs` reports E1802 (the module needs the real compiler).
    if let Some(feature) = reject_feature(trimmed) {
        return Ok(InputKind::Reject(feature.to_string()));
    }

    // `use …` import line (S16). Carry it across the session so a later input
    // can resolve through its alias (D-REPL10).
    if looks_like_import(trimmed) {
        // The parser wants a trailing `;`; REPL inputs may omit it.
        let normalized = {
            let t = trimmed.trim_end();
            if t.ends_with(';') {
                t.to_string()
            } else {
                format!("{};", t)
            }
        };
        let full = format!("// repl:{}\n{}\n", step, normalized);
        let (toks, lex_diags) = crate::Lexer::lex(&full);
        if !lex_diags.is_empty() {
            return Err(lex_diags);
        }
        match crate::Parser::parse(&toks) {
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
        if let Err(ds) = crate::Parser::parse(&toks) {
            return Err(ds);
        }
        return Ok(InputKind::Item(src));
    }

    // For everything (statements AND bare expressions), try wrapping as:
    //   fn __repl__() { val __repl_echo__ = <input>; }
    // first. If it parses, the last statement is a Val with the magic name.
    // In run_repl_step we detect `__repl_echo__` and echo its value (D-REPL16=B).
    // If input already ends in `;`, suppress the echo.
    //
    // For inputs that are already statements (val/var/print/if/while/…),
    // the `val __repl_echo__ = stmt` form won't parse; we fall through to
    // plain statement wrapping.
    // Strategy: if the input isn't a statement keyword, try wrapping it as
    // `val __repl_echo__ = <input>;`. This lets bare expressions like `1 + 2`
    // parse as a `Val` binding with the magic sentinel name. `run_repl_step`
    // then echoes the value without adding it to the session scope.
    //
    // For statements (val/var/print/if/…) or suppressed inputs, try the plain
    // statement-wrapping form first; if that fails, try the echo sentinel.
    let try_stmt_first = starts_with_stmt_keyword(trimmed) || suppress;

    // Plain statement wrapping: add `;` if the input doesn't end in `;` or `}`.
    // Jet requires explicit semicolons after statements. REPL inputs omit them
    // for brevity, so we inject them.
    let plain_input = {
        let t = trimmed.trim_end();
        if t.ends_with(';') || t.ends_with('}') {
            t.to_string()
        } else {
            format!("{};", t)
        }
    };
    let plain_src = format!("// repl:{}\nfn __repl__() {{\n{}\n}}\n", step, plain_input);
    // Echo-sentinel wrapping (D-BIND4: `::` sigil binding).
    let echo_stmt = format!("__repl_echo__ :: {}", trimmed);
    let echo_src = format!("// repl:{}\nfn __repl__() {{\n{}\n}}\n", step, echo_stmt);

    if try_stmt_first {
        // Try plain statement form; if it succeeds, return it.
        // The check_src is the full statement content (with `;` for sema).
        let (toks, lex_diags) = crate::Lexer::lex(&plain_src);
        if lex_diags.is_empty() {
            if let Ok(prog) = crate::Parser::parse(&toks) {
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
        let (toks, lex_diags) = crate::Lexer::lex(&echo_src);
        if lex_diags.is_empty() {
            if let Ok(prog) = crate::Parser::parse(&toks) {
                if let Some(Item::Func(f)) = prog.items.into_iter().next() {
                    if !f.body.is_empty() {
                        // check_src must end in `;` for type_check_stmts.
                        let echo_check_src = format!("{};", echo_stmt);
                        return Ok(InputKind::Stmts(f.body, suppress, echo_check_src));
                    }
                }
            }
        }
    }

    // Fallback: plain statement form (returns parse error if any).
    let (toks, lex_diags) = crate::Lexer::lex(&plain_src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    match crate::Parser::parse(&toks) {
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
/// the `use core.io` line is included for teaching purposes (the REPL shows a
/// note about it on first use) but the import itself is a no-op for the checker.
const PRELOAD_SRC: &str = "";

/// Build a complete synthetic Jet source from accumulated declarations +
/// a new check function, then run sema. `check_src` is the statement(s) text
/// as produced by `classify` — it already ends in `;` (or `}`). Returns errors only.
fn type_check_stmts(session: &Session, check_src: &str, step: usize) -> Vec<Diagnostic> {
    // Binding stubs go inside the check function so sema sees prior `val` bindings.
    let binding_stubs = session.binding_stubs_src();
    let body = if binding_stubs.is_empty() {
        check_src.to_string()
    } else {
        format!("{}\n{}", binding_stubs, check_src)
    };
    let prog_src = format!(
        "{}{}{}\nfn __repl_check_{}__() {{\n{}\n}}\n",
        PRELOAD_SRC,
        session.import_src(),
        session.accumulated_src(),
        step,
        body,
    );
    run_sema(&prog_src)
}

/// Build a complete synthetic Jet source from accumulated declarations +
/// new item, then run sema. Returns errors only.
fn type_check_item(session: &Session, new_item_src: &str) -> Vec<Diagnostic> {
    let prog_src = format!(
        "{}{}{}\n{}\n",
        PRELOAD_SRC,
        session.import_src(),
        session.accumulated_src(),
        new_item_src,
    );
    run_sema(&prog_src)
}

/// Parse + sema-check `src`, return errors only.
/// Uses `Check` mode so E0101 (no `run`) is not fired — the REPL never has
/// a `run` function in its synthetic programs (D-REPL: session is a library).
fn run_sema(src: &str) -> Vec<Diagnostic> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return lex_diags
            .into_iter()
            .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
            .collect();
    }
    let mut prog = match crate::Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => return ds,
    };
    // When the program has imports (a `use …` carried across the session),
    // the single-file checker can't resolve core-module aliases — only the
    // Loader-driven bundle path registers `core_imports`. Route through it via
    // a temp file so e.g. `use core.math as math` makes `math.sqrt(…)` resolve
    // (S16/D-REPL10). The common import-free path keeps the cheaper checker so
    // existing transcript spans are unchanged.
    if !prog.imports.is_empty() {
        return run_sema_bundle(src);
    }
    // CompileMode::Check: type-check without requiring `run`.
    let all = crate::Sema::check_with_mode(&mut prog, crate::Sema::CompileMode::Check);
    all.into_iter()
        .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
        .collect()
}

/// Sema-check a synthetic program that contains imports, via the Loader bundle
/// path (the only path that registers core-module aliases). Writes `src` to a
/// temp `.jet` file and runs `Check` mode so no `run` is required.
fn run_sema_bundle(src: &str) -> Vec<Diagnostic> {
    let tmp_path = std::env::temp_dir().join(unique_temp_name("check"));
    if std::fs::write(&tmp_path, src).is_err() {
        // Fall back to the single-file checker if the temp write fails.
        let (toks, _) = crate::Lexer::lex(src);
        if let Ok(mut prog) = crate::Parser::parse(&toks) {
            return crate::Sema::check_with_mode(&mut prog, crate::Sema::CompileMode::Check)
                .into_iter()
                .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
                .collect();
        }
        return Vec::new();
    }
    let path_str = tmp_path.to_string_lossy().to_string();
    let diags = match crate::Loader::load_entry(&path_str) {
        Ok(mut bundle) => crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Check),
        Err(ds) => ds,
    };
    let _ = std::fs::remove_file(&tmp_path);
    diags
        .into_iter()
        .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
        .collect()
}

/// Rebuild the func_defs table from accumulated item sources.
fn rebuild_funcs(session: &mut Session) {
    session.func_defs.clear();
    let src = format!("{}{}", PRELOAD_SRC, session.accumulated_src());
    let (toks, _) = crate::Lexer::lex(&src);
    if let Ok(prog) = crate::Parser::parse(&toks) {
        for item in prog.items {
            if let Item::Func(f) = item {
                session.func_defs.insert(f.name.clone(), f);
            }
        }
    }
}

// ── value display (D-REPL16=B) ─────────────────────────────────────────────

fn display_value(v: &CtValue) -> String {
    let val_str = v.jet_show();
    let ty = type_name(v);
    if matches!(v, CtValue::Str(_)) {
        format!("\"{}\" : {}", val_str, ty)
    } else {
        format!("{} : {}", val_str, ty)
    }
}

fn type_name(v: &CtValue) -> &'static str {
    match v {
        CtValue::Int(_) => "Int",
        CtValue::Float(_) => "Float",
        CtValue::Bool(_) => "Bool",
        CtValue::Char(_) => "Char",
        CtValue::Str(_) => "String",
        CtValue::Bytes(_) => "[U8]",
        CtValue::List(_) => "List",
        CtValue::Map(_) => "Map",
        CtValue::Struct { .. } => "Struct",
        CtValue::Enum { .. } => "Enum",
        CtValue::Some(_) | CtValue::None(_) => "Option",
        CtValue::ResOk(_) | CtValue::ResErr(_) => "Result",
        CtValue::Unit => "()",
        CtValue::Closure(_) => "Fn",
    }
}

// ── :load ─────────────────────────────────────────────────────────────────

/// `:load <file>` — load a Jet source file into the session.
fn cmd_load(
    path_str: &str,
    session: &mut Session,
    base_dir: &Path,
    _color: bool,
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
    let prog = match crate::Parser::parse(&toks) {
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

    // Run run() if present.
    if has_run {
        if let Some(main) = session.func_defs.get("run") {
            let main_clone = main.clone();
            let funcs: HashMap<String, &Func> = session
                .func_defs
                .iter()
                .map(|(k, v)| (k.clone(), v))
                .collect();
            let mut sink = DevSink::new();
            let _ = crate::Comptime::run_main_with_fuel(
                &main_clone,
                &funcs,
                base_dir,
                &mut sink,
                REPL_FUEL_BUDGET,
            );
            if !sink.stdout.is_empty() {
                let _ = write!(out_sink, "{}", sink.stdout);
            }
        }
    }
}

// ── :type ─────────────────────────────────────────────────────────────────

fn cmd_type(name: &str, session: &Session, color: bool) {
    if let Some(v) = session.scope.get(name) {
        println!("{} : {}", name, type_name(v));
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
    eprint!("{}", crate::render_all_colored(file, src, diags, color));
    let n = diags.len();
    eprintln!("{} problem{} found", n, if n == 1 { "" } else { "s" });
}

// ── main REPL loop ─────────────────────────────────────────────────────────

/// Run an interactive REPL session.
pub fn run(project_dir: Option<&str>) -> i32 {
    let color = color_on();
    let base_dir: std::path::PathBuf =
        project_dir
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });

    // D-REPL-BANNER=A
    println!(
        "Jet {} — interactive REPL  (type {} to exit, {} for commands)",
        env!("CARGO_PKG_VERSION"),
        bold(":quit", color),
        bold(":help", color)
    );
    println!();

    let mut session = Session::new();

    // D-REPL10=A: --project loads the project's source items into the session
    // so functions/types are available without `use`.
    if let Some(dir) = project_dir {
        let mut stdout = io::stdout();
        load_project_items(Path::new(dir), &mut session, &mut stdout);
    }

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();

    loop {
        let text = match read_input(&mut stdin_lock, color, "user> ") {
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

        session.step += 1;
        let step_src = format!("<repl:{}>", session.step);

        // D-REPL-PRELOAD teaching note: on first use of print/eprint/input.
        if !session.shown_preload_note
            && (trimmed.contains("print(")
                || trimmed.contains("eprint(")
                || trimmed.contains("input("))
        {
            println!(
                "{}",
                dim(
                    "note: `print` is from `use core.io` — imported automatically in the REPL",
                    color
                )
            );
            session.shown_preload_note = true;
        }

        let kind = match classify(trimmed, session.step) {
            Ok(k) => k,
            Err(ds) => {
                render_diags(&step_src, trimmed, &ds, color);
                continue;
            }
        };

        match kind {
            InputKind::Empty => {}

            InputKind::Meta(cmd, arg) => {
                handle_meta(&cmd, arg.as_deref(), &mut session, &base_dir, color);
            }

            InputKind::Reject(feature) => {
                let d = e1802(&feature);
                render_diags(&step_src, trimmed, &[d], color);
            }

            InputKind::Item(src) => {
                let errors = type_check_item(&session, &src);
                if !errors.is_empty() {
                    render_diags(&step_src, trimmed, &errors, color);
                    continue;
                }
                session.item_srcs.push(src);
                rebuild_funcs(&mut session);
                println!("{}", green("ok", color));
            }

            InputKind::Import(src) => {
                // Try it against the accumulated session so a bad import
                // (unknown core module, etc.) reports before being kept.
                session.import_srcs.push(src.clone());
                let errors = type_check_item(&session, "");
                if !errors.is_empty() {
                    session.import_srcs.pop();
                    render_diags(&step_src, trimmed, &errors, color);
                    continue;
                }
                // D-CTCORE1: register any core alias → module path so the
                // comptime interpreter can execute whitelisted pure Core calls.
                update_core_imports(&src, &mut session.core_imports);
                println!("{}", green("ok", color));
            }

            InputKind::Stmts(stmts, suppress, check_src) => {
                let errors = type_check_stmts(&session, &check_src, session.step);
                if !errors.is_empty() {
                    render_diags(&step_src, trimmed, &errors, color);
                    continue;
                }

                // D-REPL8=A: detect which session bindings are moved by this input.
                let session_binding_names: HashSet<String> =
                    session.scope.keys().cloned().collect();
                let newly_moved =
                    collect_moved_names(&stmts, &session_binding_names, &session.scope);

                // Snapshot keys before execution to detect new bindings.
                let before_keys: HashSet<String> = session.scope.keys().cloned().collect();

                let funcs: HashMap<String, &Func> = session
                    .func_defs
                    .iter()
                    .map(|(k, v)| (k.clone(), v))
                    .collect();
                let mut sink = DevSink::new();
                match crate::Comptime::run_repl_step(
                    &stmts,
                    &funcs,
                    &base_dir,
                    &mut sink,
                    &mut session.scope,
                    REPL_FUEL_BUDGET,
                    suppress,
                    &session.core_imports,
                ) {
                    Ok(echo_val) => {
                        // Track moves: bindings consumed in this input are gone.
                        session.moved_names.extend(newly_moved);
                        // Record stmt source for :run materialization.
                        let raw = trimmed.trim_end_matches(';').trim().to_string();
                        if !raw.is_empty() && !raw.starts_with("__repl_echo__") {
                            session.stmt_srcs.push(format!("{};", raw));
                        }
                        // Register any new bindings for sema visibility.
                        let new_names: Vec<String> = session
                            .scope
                            .keys()
                            .filter(|k| !before_keys.contains(*k) && *k != "__repl_echo__")
                            .cloned()
                            .collect();
                        for name in &new_names {
                            if let Some(v) = session.scope.get(name) {
                                let v = v.clone();
                                let mutable = val_binding_mutable(&stmts, name);
                                session.record_binding(name, &v, mutable);
                            }
                        }
                        if !sink.stdout.is_empty() {
                            print!("{}", sink.stdout);
                        }
                        if !sink.stderr.is_empty() {
                            eprint!("{}", sink.stderr);
                        }
                        if let Some(v) = echo_val {
                            if !matches!(v, CtValue::Unit) {
                                println!("{}", display_value(&v));
                            }
                        }
                    }
                    Err(d) => {
                        let d = if d.code == "E2202" {
                            e1801(REPL_FUEL_BUDGET)
                        } else {
                            d
                        };
                        render_diags(&step_src, trimmed, &[d], color);
                    }
                }
            }
        }
    }

    0
}

/// Handle a `:command` meta-command (D-REPL15=B).
fn handle_meta(cmd: &str, arg: Option<&str>, session: &mut Session, base_dir: &Path, color: bool) {
    match cmd {
        "quit" | "q" | "exit" => {
            println!("bye");
            std::process::exit(0);
        }
        "reset" => {
            session.reset();
            println!("session reset");
        }
        "load" => {
            let path = match arg {
                Some(p) => p,
                None => {
                    eprintln!("usage: :load <file.jet>");
                    return;
                }
            };
            let mut stdout = io::stdout();
            cmd_load(path, session, base_dir, color, &mut stdout);
        }
        "type" => {
            let name = match arg {
                Some(n) => n,
                None => {
                    eprintln!("usage: :type <name>");
                    return;
                }
            };
            cmd_type(name, session, color);
        }
        "run" => {
            // D-REPL-FUEL=A: compile the session to a temp file and run it natively.
            let mut stdout = io::stdout();
            cmd_run_native(session, color, &mut stdout);
        }
        "help" => print_help(color),
        _ => {
            eprintln!(
                "unknown meta-command `:{}`; type {} to see available commands",
                cmd,
                bold(":help", color)
            );
        }
    }
}

fn print_help(color: bool) {
    println!("{}", bold("REPL meta-commands", color));
    println!("  {}          end the session", bold(":quit", color));
    println!(
        "  {}         clear all bindings and start fresh",
        bold(":reset", color)
    );
    println!(
        "  {}  load a Jet file into the session",
        bold(":load <file.jet>", color)
    );
    println!(
        "  {}      show the type of a binding",
        bold(":type <name>", color)
    );
    println!(
        "  {}           compile + run the session (bypasses fuel cap)",
        bold(":run", color)
    );
    println!("  {}          show this message", bold(":help", color));
    println!();
    println!(
        "  Tip: end a line with {} to suppress echo of its value.",
        bold(";", color)
    );
    println!(
        "  Tip: {} is auto-imported — type `print(\"hello\")` to try it.",
        bold("core.io", color)
    );
    println!(
        "  Tip: a `{}` line is kept for the whole session — import once, use it on any later line.",
        bold("use core.math as math", color)
    );
}

// ── transcript test harness (D-REPL20=A) ──────────────────────────────────

/// Run a REPL transcript test: feed `inputs` through the session, collect
/// the output lines. Used by `tests/repl.rs`. No color, no interactive I/O.
///
/// Each input should be a single REPL input (may be multi-line if the caller
/// joins them). The returned string is the combined stdout of the session.
pub fn run_transcript(inputs: &[&str], project_dir: Option<&str>) -> String {
    let base_dir: std::path::PathBuf = project_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let mut session = Session::new();
    let mut out = String::new();

    // D-REPL10=A: load project items when --project is set.
    if let Some(dir) = project_dir {
        let mut buf = Vec::new();
        load_project_items(Path::new(dir), &mut session, &mut buf);
        out.push_str(&String::from_utf8_lossy(&buf));
    }

    for &input in inputs {
        let trimmed = input.trim();
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
                        cmd_load(&path, &mut session, &base_dir, false, &mut buf);
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
                        if let Some(v) = session.scope.get(name) {
                            out.push_str(&format!("{} : {}\n", name, type_name(v)));
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
                        // D-REPL-FUEL=A: transcript path uses interpreter-based
                        // materialization (same semantics, no rustc dependency).
                        let run_out = cmd_run_transcript(&session);
                        out.push_str(&run_out);
                    }
                    "help" => out.push_str("REPL meta-commands\n"),
                    _ => out.push_str(&format!("unknown meta-command `:{}`\n", cmd)),
                }
            }

            InputKind::Reject(feature) => {
                let d = e1802(&feature);
                out.push_str(&format!("error [E1802]: {}\n", d.what));
            }

            InputKind::Item(src) => {
                let errors = type_check_item(&session, &src);
                if !errors.is_empty() {
                    for d in &errors {
                        out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                    }
                    continue;
                }
                session.item_srcs.push(src);
                rebuild_funcs(&mut session);
                out.push_str("ok\n");
            }

            InputKind::Import(src) => {
                session.import_srcs.push(src.clone());
                let errors = type_check_item(&session, "");
                if !errors.is_empty() {
                    session.import_srcs.pop();
                    for d in &errors {
                        out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                    }
                    continue;
                }
                // D-CTCORE1: register any core alias → module path so the
                // comptime interpreter can execute whitelisted pure Core calls.
                update_core_imports(&src, &mut session.core_imports);
                out.push_str("ok\n");
            }

            InputKind::Stmts(stmts, suppress, check_src) => {
                let errors = type_check_stmts(&session, &check_src, session.step);
                if !errors.is_empty() {
                    for d in &errors {
                        out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                    }
                    continue;
                }

                // D-REPL8=A: detect which session bindings are moved by this input.
                let session_binding_names: HashSet<String> =
                    session.scope.keys().cloned().collect();
                let newly_moved =
                    collect_moved_names(&stmts, &session_binding_names, &session.scope);

                let before_keys: HashSet<String> = session.scope.keys().cloned().collect();

                let funcs: HashMap<String, &Func> = session
                    .func_defs
                    .iter()
                    .map(|(k, v)| (k.clone(), v))
                    .collect();
                let mut sink = DevSink::new();
                match crate::Comptime::run_repl_step(
                    &stmts,
                    &funcs,
                    &base_dir,
                    &mut sink,
                    &mut session.scope,
                    REPL_FUEL_BUDGET,
                    suppress,
                    &session.core_imports,
                ) {
                    Ok(echo_val) => {
                        // Track moves for cross-input detection (D-REPL8=A).
                        session.moved_names.extend(newly_moved);
                        // Record stmt source for :run materialization.
                        let raw = trimmed.trim_end_matches(';').trim().to_string();
                        if !raw.is_empty() && !raw.starts_with("__repl_echo__") {
                            session.stmt_srcs.push(format!("{};", raw));
                        }
                        // Register new bindings for sema visibility.
                        let new_names: Vec<String> = session
                            .scope
                            .keys()
                            .filter(|k| !before_keys.contains(*k) && *k != "__repl_echo__")
                            .cloned()
                            .collect();
                        for name in &new_names {
                            if let Some(v) = session.scope.get(name) {
                                let v = v.clone();
                                let mutable = val_binding_mutable(&stmts, name);
                                session.record_binding(name, &v, mutable);
                            }
                        }
                        if !sink.stdout.is_empty() {
                            out.push_str(&sink.stdout);
                        }
                        if !sink.stderr.is_empty() {
                            out.push_str(&sink.stderr);
                        }
                        if let Some(v) = echo_val {
                            if !matches!(v, CtValue::Unit) {
                                out.push_str(&format!("{}\n", display_value(&v)));
                            }
                        }
                    }
                    Err(d) => {
                        let d = if d.code == "E2202" {
                            e1801(REPL_FUEL_BUDGET)
                        } else {
                            d
                        };
                        out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
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
        scope.insert("s".to_string(), CtValue::Str("hello".to_string()));
        let moved = collect_moved_names(&stmts, &names, &scope);
        assert!(moved.contains("s"), "moved names: {:?}", moved);
    }

    #[test]
    fn typed_binding_stub_name_matches_moved_name() {
        assert_eq!(Session::binding_stub_name("s: String :: \"\""), "s");
    }
}
