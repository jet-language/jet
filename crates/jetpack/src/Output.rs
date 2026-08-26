//! Jetpack terminal output: quiet, aligned, colored, TTY-aware.
//!
//! Mirrors the `nh`/ansi aesthetic from `examples/jetos/lib/ansi.jet`. A single
//! `Theme` carries one `color` flag so `--no-color` / `NO_COLOR` flips every
//! line through one value instead of scattered globals. Everything Jetpack says
//! to the user goes through here (D-JPK14 beauty requirement).

use super::RefSpec::RefError;
use crate::Syntax;
use jet_foundation::Diagnostics::Diagnostic;
use jet_foundation::Terminal::{ColorChoice, Theme as SharedTheme};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
pub struct Theme {
    pub color: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteProgressSnapshot {
    /// `None` means the counter overflowed and must not be rendered as a
    /// guessed number.
    pub transferred: Option<u64>,
    /// `None` means no fetched object has disclosed a trusted byte total yet.
    pub total: Option<u64>,
}

#[derive(Debug, Default)]
struct ByteProgressState {
    transferred: Option<u64>,
    total: Option<u64>,
    total_overflowed: bool,
    phase: String,
    package_done: usize,
    package_total: usize,
    object_done: usize,
    object_total: usize,
    object_base_done: usize,
    object_base_total: usize,
    last_rendered_phase: String,
    last_rendered_package_done: usize,
    last_rendered_package_total: usize,
    last_rendered_object_done: usize,
    last_rendered_object_total: usize,
    last_rendered_transferred: Option<u64>,
    last_rendered_total: Option<u64>,
}

#[derive(Debug, Clone)]
struct AggregateProgressSnapshot {
    phase: String,
    package_done: usize,
    package_total: usize,
    object_done: usize,
    object_total: usize,
    transferred: Option<u64>,
    total: Option<u64>,
}

#[derive(Clone, Copy)]
struct AggregateProgressRenderer {
    theme: Theme,
    tty: bool,
    drawn: usize,
}

/// Thread-safe byte ledger for a realization's closure. Narinfo discovery and
/// NAR reads happen on parallel workers, so updates must be additive and
/// separately represent an unknown denominator.
pub(crate) struct ByteProgress {
    state: Mutex<ByteProgressState>,
    renderer: Mutex<Option<AggregateProgressRenderer>>,
}

impl Default for ByteProgress {
    fn default() -> Self {
        Self {
            state: Mutex::new(ByteProgressState {
                transferred: Some(0),
                ..ByteProgressState::default()
            }),
            renderer: Mutex::new(None),
        }
    }
}

impl ByteProgress {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(crate) fn snapshot(&self) -> ByteProgressSnapshot {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        ByteProgressSnapshot {
            transferred: state.transferred,
            total: state.total,
        }
    }

    pub(crate) fn has_facts(&self) -> bool {
        let snapshot = self.snapshot();
        snapshot.total.is_some() || snapshot.transferred != Some(0)
    }

    pub(crate) fn render(&self) -> String {
        let snapshot = self.snapshot();
        let transferred = snapshot
            .transferred
            .map(human_size)
            .unwrap_or_else(|| "?".to_string());
        let total = snapshot
            .total
            .map(human_size)
            .unwrap_or_else(|| "?".to_string());
        format!("{transferred} / {total}")
    }

    fn discovered_bytes(&self, bytes: u64) {
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.total_overflowed {
                return;
            }
            state.total = match state.total {
                Some(total) => match total.checked_add(bytes) {
                    Some(total) => Some(total),
                    None => {
                        state.total_overflowed = true;
                        None
                    }
                },
                None => Some(bytes),
            };
        }
        self.render_if_needed(false);
    }

    fn transferred_bytes(&self, bytes: u64) {
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.transferred = state
                .transferred
                .and_then(|transferred| transferred.checked_add(bytes));
        }
        self.render_if_needed(false);
    }

    fn activate_renderer(&self, theme: Theme, tty: bool) {
        let mut renderer = self
            .renderer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        renderer.get_or_insert(AggregateProgressRenderer {
            theme,
            tty,
            drawn: 0,
        });
    }

    fn clear_renderer(&self) {
        let mut renderer = self
            .renderer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(renderer) = renderer.as_mut() else {
            return;
        };
        if renderer.tty && renderer.drawn > 0 {
            eprint!("\x1b[{}F\x1b[0J", renderer.drawn);
        }
        renderer.drawn = 0;
    }

    fn phase(&self, phase: &str) {
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.phase = phase.to_string();
        }
        self.render_if_needed(false);
    }

    fn object_progress(&self, done: usize, total: usize) {
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.object_done = state.object_base_done.saturating_add(done);
            state.object_total = state.object_base_total.saturating_add(total);
        }
        self.render_if_needed(false);
    }

    fn render_if_needed(&self, force: bool) {
        const BYTE_REDRAW_THRESHOLD: u64 = 1_000_000;
        let (snapshot, structural) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let structural = force
                || state.phase != state.last_rendered_phase
                || state.package_done != state.last_rendered_package_done
                || state.package_total != state.last_rendered_package_total
                || state.object_done != state.last_rendered_object_done
                || state.object_total != state.last_rendered_object_total
                || state.total != state.last_rendered_total;
            let byte_delta = match (state.transferred, state.last_rendered_transferred) {
                (Some(current), Some(previous)) => current.saturating_sub(previous),
                (Some(_), None) | (None, Some(_)) => BYTE_REDRAW_THRESHOLD,
                (None, None) => 0,
            };
            let byte_redraw = byte_delta >= BYTE_REDRAW_THRESHOLD
                || state.total == state.transferred && state.transferred != Some(0);
            if !(structural || byte_redraw) {
                return;
            }
            state.last_rendered_phase = state.phase.clone();
            state.last_rendered_package_done = state.package_done;
            state.last_rendered_package_total = state.package_total;
            state.last_rendered_object_done = state.object_done;
            state.last_rendered_object_total = state.object_total;
            state.last_rendered_transferred = state.transferred;
            state.last_rendered_total = state.total;
            (
                AggregateProgressSnapshot {
                    phase: state.phase.clone(),
                    package_done: state.package_done,
                    package_total: state.package_total,
                    object_done: state.object_done,
                    object_total: state.object_total,
                    transferred: state.transferred,
                    total: state.total,
                },
                structural,
            )
        };
        let mut renderer = self
            .renderer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(renderer) = renderer.as_mut() else {
            return;
        };
        if !renderer.tty && !structural {
            return;
        }
        let line = renderer.theme.render_aggregate_progress(
            &snapshot.phase,
            snapshot.package_done,
            snapshot.package_total,
            snapshot.object_done,
            snapshot.object_total,
            snapshot.transferred,
            snapshot.total,
        );
        if renderer.tty {
            if renderer.drawn > 0 {
                eprint!("\x1b[{}F\x1b[0J", renderer.drawn);
            }
            eprintln!("{}{}", LiveRegion::pad(), line);
            renderer.drawn = 1;
        } else {
            eprintln!("{}{}", LiveRegion::pad(), line);
        }
    }
}

impl crate::Store::ProgressSink for ByteProgress {
    fn discovered_bytes(&self, bytes: u64) {
        ByteProgress::discovered_bytes(self, bytes);
    }

    fn transferred_bytes(&self, bytes: u64) {
        ByteProgress::transferred_bytes(self, bytes);
    }

    fn phase(&self, phase: &str) {
        ByteProgress::phase(self, phase);
    }

    fn object_progress(&self, done: usize, total: usize) {
        ByteProgress::object_progress(self, done, total);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanMark {
    Add,
    Remove,
    Change,
}

impl PlanMark {
    fn symbol(self) -> &'static str {
        match self {
            PlanMark::Add => "+",
            PlanMark::Remove => "-",
            PlanMark::Change => "~",
        }
    }
}

impl Theme {
    /// Compatibility entry point: `true` is the historical no-color flag.
    pub fn resolve(no_color: bool) -> Theme {
        let choice = if no_color {
            ColorChoice::Never
        } else {
            ColorChoice::Auto
        };
        Self::resolve_choice(choice)
    }

    /// Resolve the shared explicit > NO_COLOR > FORCE_COLOR > TTY policy.
    pub fn resolve_choice(choice: ColorChoice) -> Theme {
        Self::resolve_for(choice, std::io::stderr().is_terminal())
    }

    /// Resolve against the stream that will actually receive the bytes.
    pub fn resolve_for(choice: ColorChoice, is_terminal: bool) -> Theme {
        Theme {
            color: choice.resolve(is_terminal),
        }
    }

    fn shared(&self) -> SharedTheme {
        SharedTheme::new(self.color)
    }

    pub fn bold(&self, t: &str) -> String {
        self.shared().bold(t)
    }
    pub fn green(&self, t: &str) -> String {
        self.shared().success(t)
    }
    pub fn yellow(&self, t: &str) -> String {
        self.shared().warn(t)
    }
    pub fn red(&self, t: &str) -> String {
        self.shared().error(t)
    }
    pub fn cyan(&self, t: &str) -> String {
        self.shared().accent(t)
    }
    pub fn gray(&self, t: &str) -> String {
        self.shared().dim(t)
    }
    pub fn border(&self, t: &str) -> String {
        self.shared().border(t)
    }

    /// The aligned `jet` gutter every status line shares.
    fn gutter(&self) -> String {
        self.cyan(Syntax::JETPACK_PROMPT_LABEL)
    }

    /// A primary status line: `  jet  <msg>`.
    pub fn status(&self, msg: &str) {
        eprintln!("  {}  {}", self.gutter(), msg);
    }

    /// A secondary, indented detail line: `           ▸ <msg>`.
    pub fn detail(&self, msg: &str) {
        let pad = " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4);
        eprintln!("{pad}{} {}", self.gray("▸"), msg);
    }

    pub fn graph_identity(&self, identity: &str) {
        self.detail(&format!("package graph identity: {identity}"));
    }

    /// A success line ending in a green check.
    pub fn ok(&self, msg: &str) {
        eprintln!("  {}  {} {}", self.gutter(), msg, self.green("✓"));
    }

    /// A plain note (no gutter), e.g. the trust-prompt preamble.
    pub fn note(&self, msg: &str) {
        eprintln!("\n  {}\n", self.gray(msg));
    }

    /// An aligned ledger row for one realized package:
    /// `✓ <name> <version> <state>`. `name_w` pads the name column so a run of
    /// rows reads as a table; an unknown version renders as `—` so the state
    /// column stays put. Pure so the tier-2 live region (`LiveRegion::finish`)
    /// can compute the same text without printing it itself.
    pub fn render_row(&self, name: &str, name_w: usize, version: &str, state: &str) -> String {
        let v = if version.is_empty() { "—" } else { version };
        format!(
            "{} {}  {}  {}",
            self.green("✓"),
            self.bold(&format!("{name:<name_w$}")),
            self.gray(&format!("{v:<8}")),
            self.gray(state),
        )
    }

    pub fn row(&self, name: &str, name_w: usize, version: &str, state: &str) {
        let pad = " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4);
        eprintln!("{pad}{}", self.render_row(name, name_w, version, state));
    }

    pub fn render_plan_row(
        &self,
        mark: PlanMark,
        name: &str,
        name_w: usize,
        from: &str,
        to: &str,
    ) -> String {
        let symbol = match mark {
            PlanMark::Add => self.green(mark.symbol()),
            PlanMark::Remove => self.red(mark.symbol()),
            PlanMark::Change => self.yellow(mark.symbol()),
        };
        format!(
            "{} {}  {} -> {}",
            symbol,
            self.bold(&format!("{name:<name_w$}")),
            self.gray(from),
            self.gray(to),
        )
    }

    /// A mutation plan row. Color carries the mark in a TTY; plain output keeps
    /// the same + / - / ~ symbols for deterministic logs and review.
    pub fn plan_row(&self, mark: PlanMark, name: &str, name_w: usize, from: &str, to: &str) {
        let pad = " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4);
        eprintln!(
            "{pad}{}",
            self.render_plan_row(mark, name, name_w, from, to)
        );
    }

    pub fn render_progress_chain(
        &self,
        phase: &str,
        done: usize,
        total: usize,
        node: &str,
        edge: &str,
    ) -> String {
        let count = if total == 0 {
            String::new()
        } else {
            format!(" {done}/{total}")
        };
        let edge = if edge.is_empty() {
            String::new()
        } else {
            format!(" -> {edge}")
        };
        format!(
            "{} {}{} {}{}",
            self.gray("▸"),
            phase,
            self.gray(&count),
            self.bold(node),
            self.gray(&edge),
        )
    }

    /// One truthful projection of the work graph edge currently being
    /// realized. `source -> node` is dependency direction (the provider/source
    /// supplies the package), while `completed/total` counts only nodes that
    /// finished before this current edge. A failing first node therefore stays
    /// at `completed 0/N` rather than claiming work that never completed.
    /// State is an actual phase (`Resolving`, `Building`, `Substituting`), not
    /// an animation label inferred after the fact.
    pub fn render_dependency_status(
        &self,
        phase: &str,
        completed: usize,
        total: usize,
        source: &str,
        node: &str,
        state: &str,
    ) -> String {
        let edge = if source.is_empty() {
            node.to_string()
        } else {
            format!("{} -> {}", self.gray(source), self.bold(node))
        };
        format!(
            "{phase} completed {completed}/{total} {} current: {edge} {} {}",
            self.gray("·"),
            self.gray("·"),
            self.gray(state),
        )
    }

    /// One aggregate realization line. Package counts describe resolution;
    /// object counts describe substitution and admission. A missing byte
    /// denominator remains `?` until signed metadata discloses it.
    pub fn render_aggregate_progress(
        &self,
        phase: &str,
        package_done: usize,
        package_total: usize,
        object_done: usize,
        object_total: usize,
        transferred: Option<u64>,
        total_bytes: Option<u64>,
    ) -> String {
        let (done, total, noun) = if phase == "Resolving" {
            (package_done, package_total, "packages")
        } else {
            (object_done, object_total, "objects")
        };
        let total = if total == 0 {
            "?".to_string()
        } else {
            total.to_string()
        };
        let transferred = transferred
            .map(human_size)
            .unwrap_or_else(|| "?".to_string());
        let total_bytes = total_bytes
            .map(human_size)
            .unwrap_or_else(|| "?".to_string());
        format!("{phase:<12} {done}/{total} {noun}  {transferred} / {total_bytes}")
    }

    /// Dependency-chain progress for long realization/build phases. TTY output
    /// can later pin/redraw this same line; non-TTY appends it as stable ledger.
    pub fn progress_chain(&self, phase: &str, done: usize, total: usize, node: &str, edge: &str) {
        let pad = " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4);
        eprintln!(
            "{pad}{}",
            self.render_progress_chain(phase, done, total, node, edge)
        );
    }

    // -- Tier 1: trivial ops (add/remove/env resolve). One aligned `✓ name
    // version` row per package, no state column — the state-column ledger
    // row (`row`, above) stays for tier-2 builds where "how it was
    // satisfied" is the useful signal (D-JPK-CACHE1). Pure `render_*`
    // functions are unit-testable without a TTY; the `eprintln!` wrappers
    // are the only impure part. --

    /// `✓ <name>  <version>` — no state/duration suffix.
    pub fn render_ready_row(&self, name: &str, name_w: usize, version: &str) -> String {
        let v = if version.is_empty() { "—" } else { version };
        format!(
            "{} {}  {}",
            self.green("✓"),
            self.bold(&format!("{name:<name_w$}")),
            self.gray(v),
        )
    }

    pub fn ready_row(&self, name: &str, name_w: usize, version: &str) {
        let pad = " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4);
        eprintln!("{pad}{}", self.render_ready_row(name, name_w, version));
    }

    /// `N Packages Ready ✓` / `1 Package Ready ✓`.
    pub fn render_ready_summary(&self, count: usize) -> String {
        let noun = if count == 1 { "Package" } else { "Packages" };
        format!("{count} {noun} Ready {}", self.green("✓"))
    }

    pub fn ready_summary(&self, count: usize) {
        eprintln!("  {}  {}", self.gutter(), self.render_ready_summary(count));
    }

    // -- Tier 2: long builds/downloads. A pinned live region shows
    // `building K/N · <current>` plus a progress bar; finished packages
    // promote UP out of it as permanent ledger rows. Non-TTY never draws the
    // live region — callers fall back to `progress_chain`/`row` plain lines,
    // which stay the CI-safe floor. --

    /// A `done/total` progress bar: solid blocks for done, light blocks for
    /// remaining. `width` is the bar's character count.
    pub fn render_progress_bar(done: usize, total: usize, width: usize) -> String {
        if total == 0 || width == 0 {
            return "░".repeat(width);
        }
        let filled = ((done as f64 / total as f64) * width as f64).round() as usize;
        let filled = filled.min(width);
        format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
    }

    /// `building K/N · <current>` — the live region's header line.
    pub fn render_live_header(
        &self,
        verb: &str,
        done: usize,
        total: usize,
        current: &str,
    ) -> String {
        format!("{verb} {done}/{total} {} {}", self.gray("·"), current)
    }

    /// A live region for one tier-2 phase. TTY: redraws its status lines in
    /// place (ANSI cursor-up + clear-to-end) and lets finished rows scroll
    /// permanently above it. Non-TTY: `finish` is the only thing that prints
    /// (plain sequential ledger lines); `set_status`/`collapse` are inert so
    /// output stays deterministic (the CI-safe floor).
    pub fn live_region(&self) -> LiveRegion<'_> {
        LiveRegion {
            theme: self,
            tty: self.color && std::io::stderr().is_terminal(),
            drawn: 0,
            progress: ByteProgress::new(),
        }
    }

    // -- Tier 3: mutations (jetos switch/apply, jetpack update/remove). A
    // `Plan  gen N → N+1` header, `+ / ~ / -` rows (existing `plan_row`),
    // an optional `Download <size>` line, then the existing `confirm_apply`
    // gate. On yes, the plan region becomes the applied ledger. --

    pub fn render_plan_gen_header(from_gen: usize, to_gen: usize) -> String {
        format!("Plan  gen {from_gen} → {to_gen}")
    }

    pub fn plan_gen_header(&self, from_gen: usize, to_gen: usize) {
        self.status(&Self::render_plan_gen_header(from_gen, to_gen));
    }

    /// `Download <human size>`, e.g. `Download 240 MB`.
    pub fn render_download_line(bytes: u64) -> String {
        format!("Download {}", human_size(bytes))
    }

    pub fn download_line(&self, bytes: u64) {
        self.status(&Self::render_download_line(bytes));
    }

    /// The applied ledger: printed once the mutation gate is accepted,
    /// replacing the plan region with the same rows as resolved fact.
    pub fn applied_header(&self, to_gen: usize) {
        self.status(&format!("Apply  gen {to_gen}"));
    }

    pub fn confirm_apply(&self, assume_yes: bool) -> bool {
        if assume_yes {
            self.status("applying plan (--yes)");
            return true;
        }
        if !std::io::stdin().is_terminal() {
            self.status("plan only; pass -y or --yes to apply in a non-interactive shell.");
            return false;
        }
        eprint!("  {}  Apply? [y/N] ", self.gutter());
        use std::io::Write;
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            self.status("plan cancelled.");
            return false;
        }
        let answer = answer.trim();
        let apply = answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes");
        if !apply {
            self.status("plan cancelled.");
        }
        apply
    }

    /// Confirm the first acquisition for an environment. The closure has
    /// already been resolved, so this gate reports its package count and
    /// trusted byte total before any payload is fetched.
    pub fn confirm_download(
        &self,
        label: &str,
        packages: usize,
        bytes: Option<u64>,
        assume_yes: bool,
    ) -> bool {
        let noun = if packages == 1 { "package" } else { "packages" };
        let size = bytes
            .map(human_size)
            .unwrap_or_else(|| "size unknown".to_string());
        let summary = format!("{label} needs {packages} {noun}, {size}");
        if assume_yes {
            self.status(&summary);
            return true;
        }
        if !std::io::stdin().is_terminal() {
            self.error_coded(
                "E1340",
                &format!("{summary} and cannot prompt here"),
                "the environment needs package downloads and stdin is not a terminal to ask for confirmation",
                "pass -y to accept, or pre-warm with `jetpack env --prep -y`",
            );
            return false;
        }
        eprint!("  {}  {} - Continue? [Y/n] ", self.gutter(), summary);
        use std::io::Write;
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            self.status("download cancelled.");
            return false;
        }
        let answer = answer.trim();
        let apply = answer.is_empty()
            || answer.eq_ignore_ascii_case("y")
            || answer.eq_ignore_ascii_case("yes");
        if !apply {
            self.status("download cancelled.");
        }
        apply
    }

    /// Choose the explicit local-unofficial catalog used to bootstrap a project
    /// before an official signed registry exists. A remembered choice remains
    /// explicit because this prompt is the authority grant.
    pub fn choose_local_catalog(&self, detected: Option<&str>, assume_yes: bool) -> Option<String> {
        self.status("package catalog setup");
        self.detail("no official signed Jetpack registry is configured yet");
        self.detail("local catalogs leave package-name mappings unverified; downloaded Nix cache bytes still require valid signatures");

        if let Some(path) = detected {
            self.detail(&format!("found local catalog: {}", self.bold(path)));
            if assume_yes {
                self.status("using and remembering the detected local catalog (--yes)");
                return Some(path.to_string());
            }
            if !std::io::stdin().is_terminal() {
                self.error_coded(
                    "E1340",
                    "the detected local catalog needs approval and cannot prompt here",
                    "local-unofficial catalogs require an explicit first-use choice",
                    "rerun in a terminal, pass -y, or pass --local-nix-catalog <dir>",
                );
                return None;
            }
            eprint!(
                "  {}  Use and remember this catalog for this project? [Y/n] ",
                self.gutter()
            );
            use std::io::Write;
            let _ = std::io::stderr().flush();
            let mut answer = String::new();
            if std::io::stdin().read_line(&mut answer).is_err() {
                self.status("catalog setup cancelled.");
                return None;
            }
            let answer = answer.trim();
            if answer.is_empty()
                || answer.eq_ignore_ascii_case("y")
                || answer.eq_ignore_ascii_case("yes")
            {
                return Some(path.to_string());
            }
            self.status("catalog setup cancelled.");
            return None;
        }

        if assume_yes || !std::io::stdin().is_terminal() {
            self.error_coded(
                "E1340",
                "this environment needs a package catalog",
                "no official signed registry or local catalog was found",
                "run in a terminal to choose a local catalog, or pass --local-nix-catalog <dir>",
            );
            return None;
        }
        eprint!("  {}  Local catalog directory: ", self.gutter());
        use std::io::Write;
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() || answer.trim().is_empty() {
            self.status("catalog setup cancelled.");
            return None;
        }
        Some(answer.trim().to_string())
    }

    /// The threshold rule — the one visual signature of entering/leaving a
    /// jetpack environment. A single horizontal line carrying the env label:
    ///
    /// `  ── myproj ─ temporary shell · exit to leave ──────────────`
    ///
    /// The first segment (the label) is bold cyan; the rest is quiet gray.
    /// Reads identically without color, so the threshold survives `NO_COLOR`.
    pub fn rule(&self, segments: &[&str]) {
        let width = std::env::var("COLUMNS")
            .ok()
            .and_then(|c| c.parse::<usize>().ok())
            .map(|c| c.saturating_sub(4).clamp(40, 96))
            .unwrap_or(64);
        let (label, rest) = match segments.split_first() {
            Some(s) => s,
            None => return,
        };
        let tail = rest.join(" · ");
        let plain_len = 3 + label.len() + if tail.is_empty() { 0 } else { 3 + tail.len() } + 1;
        let fill = "─".repeat(width.saturating_sub(plain_len).max(2));
        let mut line = format!("{} {} ", self.border("──"), self.cyan(label));
        if !tail.is_empty() {
            line.push_str(&format!("{} {} ", self.border("─"), self.gray(&tail)));
        }
        line.push_str(&self.border(&fill));
        eprintln!("\n  {line}\n");
    }

    /// A live spinner while a package realizes. Only spins when color is on
    /// (a TTY); otherwise it is inert and the caller's plain status line
    /// stands. Dropping it stops the thread and clears the line, so the final
    /// ledger row prints over a clean slate.
    pub fn spinner(&self, msg: &str) -> Spinner {
        if !self.color {
            return Spinner {
                stop: None,
                handle: None,
            };
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let pad = " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4);
        let msg = msg.to_string();
        let accent = SharedTheme::ACCENT_SGR;
        let handle = std::thread::spawn(move || {
            const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i = 0usize;
            while !flag.load(Ordering::Relaxed) {
                eprint!(
                    "\r{pad}\x1b[{accent}m{}\x1b[0m {msg}",
                    FRAMES[i % FRAMES.len()]
                );
                use std::io::Write;
                let _ = std::io::stderr().flush();
                i += 1;
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            eprint!("\r\x1b[2K");
            use std::io::Write;
            let _ = std::io::stderr().flush();
        });
        Spinner {
            stop: Some(stop),
            handle: Some(handle),
        }
    }

    /// Render an operational Jet failure through the registered E1340 row.
    pub fn error(&self, headline: &str, why: &str, fix: &str) {
        let why = if why.is_empty() {
            "Jet cannot finish this command without the input or valid operation named above"
        } else {
            why
        };
        let fix = if fix.is_empty() {
            "correct the named input or operation and run the command again"
        } else {
            fix
        };
        self.error_coded("E1340", headline, why, fix);
    }

    pub fn render_error_coded(&self, code: &str, what: &str, why: &str, fix: &str) -> String {
        Diagnostic::error(
            code,
            what.to_string(),
            why.to_string(),
            fix.to_string(),
            None,
        )
        .render_colored("", "", self.color)
    }

    /// Render a coded Jetpack failure through the shared terminal renderer.
    pub fn error_coded(&self, code: &str, headline: &str, why: &str, fix: &str) {
        eprint!("{}", self.render_error_coded(code, headline, why, fix));
    }

    /// Render a coded Jetpack warning through the shared terminal renderer.
    pub fn warning_coded(&self, code: &str, headline: &str, why: &str, fix: &str) {
        let diagnostic = Diagnostic::lint(
            code,
            headline.to_string(),
            why.to_string(),
            fix.to_string(),
            None,
        );
        eprint!("{}", diagnostic.render_colored("", "", self.color));
    }
}

/// A tier-2 live region: `set_status` redraws its lines in place on a TTY;
/// `finish` promotes one permanent row above it (scrolls up, the way a
/// finished download/build leaves the pinned area in `nh`/`cargo`-style
/// output). Non-TTY only ever prints through `finish` — `set_status` and
/// `collapse`'s redraw are no-ops there, so CI/piped output stays plain
/// sequential lines.
pub struct LiveRegion<'a> {
    theme: &'a Theme,
    tty: bool,
    /// Number of lines currently drawn by the live region, so the next
    /// redraw can erase exactly that many before writing new ones.
    drawn: usize,
    progress: Arc<ByteProgress>,
}

impl<'a> LiveRegion<'a> {
    fn pad() -> String {
        " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4)
    }

    /// Erase the live region's currently drawn status lines. `pub(crate)`:
    /// callers outside `Output` (the tier-2 realize loop) need to force a
    /// clear right before any diagnostic print, so stale progress-bar text
    /// never sits above/below an error.
    pub(crate) fn clear(&mut self) {
        self.progress.clear_renderer();
        if !self.tty || self.drawn == 0 {
            self.drawn = 0;
            return;
        }
        // Move the cursor up `drawn` lines (to column 1) and clear from
        // there to the end of the screen.
        eprint!("\x1b[{}F\x1b[0J", self.drawn);
        self.drawn = 0;
    }

    /// Promote one finished row: erase the live region, print the row
    /// permanently (it now scrolls with normal output), then the caller is
    /// expected to call `set_status` again to redraw the live region below
    /// it. On non-TTY this is the only thing that ever prints — the plain,
    /// deterministic ledger line.
    pub fn finish(&mut self, line: &str) {
        self.clear();
        eprintln!("{}{}", Self::pad(), line);
    }

    /// Redraw the live region's status lines in place. A no-op on non-TTY
    /// (finished lines from `finish` are the whole non-TTY output).
    pub fn set_status(&mut self, lines: &[String]) {
        if !self.tty {
            return;
        }
        self.clear();
        for line in lines {
            eprintln!("{}{}", Self::pad(), line);
        }
        self.drawn = lines.len();
    }

    /// Start or advance the aggregate realization line. Byte and object
    /// updates arrive through the shared progress sink while admission runs;
    /// this call supplies the package-level count and the current resolve
    /// phase before the provider begins work.
    pub(crate) fn set_aggregate_status(
        &mut self,
        phase: &str,
        completed_packages: usize,
        total_packages: usize,
    ) {
        {
            let mut state = self
                .progress
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.object_base_done = state.object_done;
            state.object_base_total = state.object_total;
            state.package_done = completed_packages;
            state.package_total = total_packages;
            state.phase = phase.to_string();
        }
        self.progress.activate_renderer(*self.theme, self.tty);
        self.progress.render_if_needed(true);
    }

    /// Project one dependency edge. TTY redraws a two-line pinned region;
    /// non-TTY appends exactly one stable chain line per node so CI retains
    /// progress without cursor control, timing noise, or duplicate bars.
    pub fn set_dependency_status(
        &mut self,
        phase: &str,
        completed: usize,
        total: usize,
        source: &str,
        node: &str,
        state: &str,
    ) {
        let base_line = self
            .theme
            .render_dependency_status(phase, completed, total, source, node, state);
        let line = if self.progress.has_facts() {
            format!("{base_line}  {}", self.theme.gray(&self.progress.render()))
        } else {
            base_line
        };
        if !self.tty {
            eprintln!("{}{}", Self::pad(), line);
            return;
        }
        self.set_status(&[
            line,
            format!(
                "{}  {}",
                Theme::render_progress_bar(completed, total, 14),
                self.theme.gray(state),
            ),
        ]);
    }

    /// Close the live region out, collapsing it to one final summary line
    /// (tier 2's `<tool> build ready ✓`).
    pub fn collapse(&mut self, summary: &str) {
        self.clear();
        eprintln!("  {}  {}", self.theme.gutter(), summary);
    }

    pub(crate) fn progress_handle(&self) -> crate::Store::ProgressHandle {
        self.progress.clone()
    }
}

/// `240 MB` / `1.3 GB` / `512 KB` — binary-ish decimal human size for the
/// tier-3 `Download` line. Whole numbers below 1000 print without a
/// fraction; everything else keeps one decimal place.
fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// Handle for a running spinner; dropping stops and clears it.
pub struct Spinner {
    stop: Option<Arc<AtomicBool>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// A short human duration for the ledger's `built 42s` / `built 2m 03s` tag.
pub fn human_duration(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

/// Render a ref-classification failure as a friendly diagnostic.
pub fn ref_error(theme: &Theme, err: &RefError) {
    let example = "fastfetch@jetpack";
    match err {
        RefError::MissingSeparator(raw) => theme.error(
            &format!("`{raw}` is missing a source"),
            "Jet refs are written `package@source#version`; `#version` is optional.",
            &format!("try `{example}` or `owner/repo@github`."),
        ),
        RefError::EmptyHalf(raw) => theme.error(
            &format!("`{raw}` has an empty source or package"),
            "Both halves of `package@source` must be filled in.",
            &format!("try `{example}`."),
        ),
        RefError::ProviderFirst { raw, replacement } => theme.error_coded(
            "E1317",
            &format!("`{raw}` puts the provider first"),
            "D-JPK-REF1 puts the package or target before `@` and the source after it.",
            &format!("put the package or target first: write `{replacement}`."),
        ),
        RefError::NonCanonical { raw, replacement } => theme.error_coded(
            "E1317",
            &format!("`{raw}` uses a retired package-ref order"),
            "D-VERDICT-2190-1 puts the version or policy after `@source`.",
            &format!("write `{replacement}`."),
        ),
        RefError::RetiredNixpkgs { raw, replacement } => theme.error_coded(
            "E1317",
            &format!("`{raw}` uses the retired `@nixpkgs` source spelling"),
            "D-JPK-SNIXREUSE1 makes Jetpack the public package source; nixpkgs remains provenance in locks and receipts.",
            &format!("write `{replacement}`."),
        ),
        RefError::PathProviderRetired { raw, path } => theme.error_coded(
            "E1317",
            &format!("`{raw}` uses the retired `path` provider word"),
            "Local `./`, `../`, and `/` paths are bare refs.",
            &format!("write `{path}`."),
        ),
        RefError::UnknownSource {
            source, declared, ..
        } => {
            let known = if declared.is_empty() {
                "Sources are built-ins such as `jetpack` and `github`, or names declared \
                 in env.jet `sources:`."
                    .to_string()
            } else {
                format!(
                    "Sources are built-ins such as `jetpack` and `github`, or names declared \
                     in env.jet. This env declares: {}.",
                    declared.join(", ")
                )
            };
            theme.error(
                &format!("`{source}` is not a known source"),
                &known,
                &format!("declare `{source}` in `sources:`, or use a known source name."),
            )
        }
        // D-MONOREF1=A: bare name with no separator that didn't match a workspace member.
        RefError::AmbiguousBare(raw) => theme.error(
            &format!("`{raw}` is ambiguous — no workspace member matches"),
            "A bare package name (no `@source` suffix) resolves against the workspace \
             member list. Either this name isn't a member, or there are multiple matches.",
            "use `package@source` to be explicit.",
        ),
        // E1230: a bare/path ref matched more than one workspace member.
        RefError::AmbiguousMember { query, candidates } => theme.error_coded(
            "E1230",
            &format!("`{query}` matches more than one workspace member"),
            &format!(
                "A bare or path-form ref resolves against the workspace member index, and \
                 this one matches several members: {}.",
                candidates.join(", ")
            ),
            "address one member by its path (e.g. `packages/logging`), or use `package@source`.",
        ),
        // E1231: a bare/path ref matched no workspace member.
        RefError::UnknownMember { query, suggestions } => {
            // No typed edit: this is a rendered package-reference error, not a
            // source diagnostic, and it carries no report file/span.
            let did_you_mean = if suggestions.is_empty() {
                "check `workspace.jet` — that name isn't in the member index.".to_string()
            } else {
                format!("did you mean: {}?", suggestions.join(", "))
            };
            theme.error_coded(
                "E1231",
                &format!("`{query}` is not a workspace member"),
                "A bare or path-form ref (no `@source` suffix) must name a member listed in the \
                 workspace index (`workspace.jet` `members:`).",
                &did_you_mean,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_rows_keep_symbols_without_color() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_plan_row(PlanMark::Add, "ripgrep", 8, "-", "14.1.0"),
            "+ ripgrep   - -> 14.1.0"
        );
        assert_eq!(
            theme.render_plan_row(PlanMark::Remove, "old", 8, "1.0.0", "-"),
            "- old       1.0.0 -> -"
        );
        assert_eq!(
            theme.render_plan_row(PlanMark::Change, "linux", 8, "6.9", "6.10"),
            "~ linux     6.9 -> 6.10"
        );
    }

    #[test]
    fn plan_rows_paint_symbols_when_colored() {
        let theme = Theme { color: true };
        let add = theme.render_plan_row(PlanMark::Add, "ripgrep", 8, "-", "14.1.0");
        let remove = theme.render_plan_row(PlanMark::Remove, "old", 8, "1.0.0", "-");
        let change = theme.render_plan_row(PlanMark::Change, "linux", 8, "6.9", "6.10");
        assert!(add.starts_with("\x1b[32m+\x1b[0m"), "add: {add}");
        assert!(remove.starts_with("\x1b[31m-\x1b[0m"), "remove: {remove}");
        assert!(change.starts_with("\x1b[33m~\x1b[0m"), "change: {change}");
        assert!(add.contains("ripgrep"), "add: {add}");
        assert!(remove.contains("old"), "remove: {remove}");
        assert!(change.contains("linux"), "change: {change}");
    }

    #[test]
    fn theme_resolve_honors_no_color_flag() {
        // Explicit `--no-color` is the deterministic floor (also covers the
        // NO_COLOR path's `color: false` branch without mutating process env
        // under parallel cargo test).
        let theme = Theme::resolve_choice(ColorChoice::Never);
        assert!(!theme.color, "--no-color flag must force color=false");
    }

    #[test]
    fn confirm_apply_yes_bypasses_gate() {
        let theme = Theme { color: false };
        assert!(theme.confirm_apply(true));
    }

    #[test]
    fn confirm_apply_non_tty_without_yes_is_plan_only() {
        // Cargo tests pipe stdin; without `-y`/`--yes` the gate must refuse
        // apply and never hang waiting for a prompt (D-FE-CLI1 non-TTY rule).
        let theme = Theme { color: false };
        if std::io::stdin().is_terminal() {
            // Host matrix may attach a real TTY; skip rather than hang.
            return;
        }
        assert!(!theme.confirm_apply(false));
    }

    #[test]
    fn live_region_is_non_tty_when_color_off() {
        // Color-off is the CI/NO_COLOR floor: `set_status` must be inert so
        // only `finish`/`set_dependency_status` append plain ledger lines.
        let theme = Theme { color: false };
        let live = theme.live_region();
        assert!(!live.tty);
    }

    #[test]
    fn live_region_plain_fallback_is_deterministic_when_no_color_and_piped() {
        const CHILD: &str = "JETPACK_OUTPUT_PLAIN_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let theme = Theme::resolve_choice(ColorChoice::Auto);
            assert!(!theme.color, "NO_COLOR must disable color");
            let mut live = theme.live_region();
            assert!(!live.tty, "piped stderr must not use live cursor control");
            live.set_dependency_status("Resolving", 0, 2, "jetpack", "ripgrep", "Fetching");
            live.finish(&theme.render_row("ripgrep", 8, "15.2.0", "Substituted"));
            live.set_dependency_status("Admitting", 1, 2, "jetpack", "jq", "Writing");
            live.finish(&theme.render_row("jq", 8, "1.8.2", "Substituted"));
            live.collapse("Build Ready ✓");
            return;
        }

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "Output::tests::live_region_plain_fallback_is_deterministic_when_no_color_and_piped",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("NO_COLOR", "")
            .env_remove("FORCE_COLOR")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .unwrap();
        assert!(output.status.success());

        let theme = Theme { color: false };
        let pad = LiveRegion::pad();
        let expected = [
            format!(
                "{pad}{}",
                theme.render_dependency_status("Resolving", 0, 2, "jetpack", "ripgrep", "Fetching")
            ),
            format!(
                "{pad}{}",
                theme.render_row("ripgrep", 8, "15.2.0", "Substituted")
            ),
            format!(
                "{pad}{}",
                theme.render_dependency_status("Admitting", 1, 2, "jetpack", "jq", "Writing")
            ),
            format!("{pad}{}", theme.render_row("jq", 8, "1.8.2", "Substituted")),
            format!("  {}  Build Ready ✓", theme.gutter()),
        ]
        .join("\n")
            + "\n";
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(stderr, expected);
        assert!(
            !stderr.as_bytes().contains(&0x1b),
            "plain output: {stderr:?}"
        );
    }

    #[test]
    fn progress_chain_has_deterministic_plain_fallback() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_progress_chain("Realize", 2, 5, "ripgrep", "nixpkgs"),
            "▸ Realize 2/5 ripgrep -> nixpkgs"
        );
    }

    #[test]
    fn dependency_status_uses_real_edge_direction_and_stable_plain_text() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_dependency_status("Building", 2, 5, "nixpkgs", "ripgrep", "Resolving",),
            "Building completed 2/5 · current: nixpkgs -> ripgrep · Resolving"
        );
    }

    #[test]
    fn dependency_status_handles_first_party_node_without_invented_parent() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_dependency_status("Building", 0, 1, "", "local-app", "Building"),
            "Building completed 0/1 · current: local-app · Building"
        );
    }

    #[test]
    fn byte_progress_grows_known_total_and_marks_unknown_honestly() {
        let progress = ByteProgress::default();
        assert_eq!(progress.render(), "0 B / ?");

        progress.discovered_bytes(100);
        progress.transferred_bytes(40);
        assert_eq!(progress.render(), "40 B / 100 B");

        progress.discovered_bytes(200);
        assert_eq!(progress.render(), "40 B / 300 B");

        progress.discovered_bytes(u64::MAX);
        progress.discovered_bytes(1);
        assert_eq!(progress.snapshot().total, None);
        assert_eq!(progress.render(), "40 B / ?");
    }

    #[test]
    fn aggregate_progress_renders_phase_counts_and_truthful_bytes() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_aggregate_progress(
                "Admitting",
                2,
                3,
                31,
                48,
                Some(340_000_000),
                Some(1_200_000_000),
            ),
            "Admitting    31/48 objects  340 MB / 1.2 GB"
        );
        assert!(theme
            .render_aggregate_progress("Resolving", 0, 2, 0, 0, Some(0), None)
            .ends_with("0 B / ?"));
    }

    // -- Tier 1: ready rows / summary --

    #[test]
    fn ready_row_has_no_state_column() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_ready_row("ripgrep", 8, "14.1.0"),
            "✓ ripgrep   14.1.0"
        );
        assert_eq!(
            theme.render_ready_row("fd", 8, "9.0.0"),
            "✓ fd        9.0.0"
        );
    }

    #[test]
    fn ready_row_unknown_version_is_a_dash() {
        let theme = Theme { color: false };
        assert_eq!(theme.render_ready_row("mystery", 8, ""), "✓ mystery   —");
    }

    #[test]
    fn ready_summary_pluralizes() {
        let theme = Theme { color: false };
        assert_eq!(theme.render_ready_summary(1), "1 Package Ready ✓");
        assert_eq!(theme.render_ready_summary(2), "2 Packages Ready ✓");
    }

    // -- Tier 2: progress bar / live header --

    #[test]
    fn progress_bar_fills_proportionally() {
        assert_eq!(Theme::render_progress_bar(0, 4, 4), "░░░░");
        assert_eq!(Theme::render_progress_bar(1, 4, 4), "█░░░");
        assert_eq!(Theme::render_progress_bar(2, 4, 4), "██░░");
        assert_eq!(Theme::render_progress_bar(4, 4, 4), "████");
    }

    #[test]
    fn progress_bar_zero_total_is_empty() {
        assert_eq!(Theme::render_progress_bar(0, 0, 4), "░░░░");
    }

    #[test]
    fn progress_bar_never_overfills_past_width() {
        // done > total (shouldn't happen, but must not panic or overshoot).
        assert_eq!(Theme::render_progress_bar(9, 4, 4), "████");
    }

    #[test]
    fn live_header_reads_as_building_k_of_n() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_live_header("Building", 31, 42, "linux"),
            "Building 31/42 · linux"
        );
    }

    // -- Tier 3: plan gen header / download line --

    #[test]
    fn plan_gen_header_reads_gen_arrow_gen() {
        assert_eq!(Theme::render_plan_gen_header(42, 43), "Plan  gen 42 → 43");
    }

    #[test]
    fn download_line_uses_human_size() {
        assert_eq!(Theme::render_download_line(240_000_000), "Download 240 MB");
        assert_eq!(Theme::render_download_line(0), "Download 0 B");
        assert_eq!(Theme::render_download_line(512), "Download 512 B");
        assert_eq!(
            Theme::render_download_line(1_300_000_000),
            "Download 1.3 GB"
        );
    }

    // -- human_size --

    #[test]
    fn human_size_picks_the_right_unit() {
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1_000), "1.0 KB");
        assert_eq!(human_size(240_000_000), "240 MB");
        assert_eq!(human_size(1_500_000_000), "1.5 GB");
    }
}
