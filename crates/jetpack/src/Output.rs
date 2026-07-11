//! Jetpack terminal output: quiet, aligned, colored, TTY-aware.
//!
//! Mirrors the `nh`/ansi aesthetic from `examples/jetos/lib/ansi.jet`. A single
//! `Theme` carries one `color` flag so `--no-color` / `NO_COLOR` flips every
//! line through one value instead of scattered globals. Everything Jetpack says
//! to the user goes through here (D-JPK14 beauty requirement).

use super::RefSpec::RefError;
use crate::Syntax;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Theme {
    pub color: bool,
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

    fn sgr(self) -> &'static str {
        match self {
            PlanMark::Add => "32",
            PlanMark::Remove => "31",
            PlanMark::Change => "33",
        }
    }
}

impl Theme {
    /// Resolve color from flags + environment. Precedence: explicit
    /// `--no-color` wins, then `NO_COLOR` (any value), then TTY detection.
    pub fn resolve(no_color_flag: bool) -> Theme {
        if no_color_flag || std::env::var_os("NO_COLOR").is_some() {
            return Theme { color: false };
        }
        Theme {
            color: std::io::stderr().is_terminal(),
        }
    }

    fn paint(&self, sgr: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{sgr}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(&self, t: &str) -> String {
        self.paint("1", t)
    }
    pub fn green(&self, t: &str) -> String {
        self.paint("32", t)
    }
    pub fn yellow(&self, t: &str) -> String {
        self.paint("33", t)
    }
    pub fn red(&self, t: &str) -> String {
        self.paint("31", t)
    }
    pub fn cyan(&self, t: &str) -> String {
        self.paint("36", t)
    }
    pub fn gray(&self, t: &str) -> String {
        self.paint("90", t)
    }

    /// The aligned `jetpack` gutter every status line shares.
    fn gutter(&self) -> String {
        self.cyan(Syntax::JETPACK_PROMPT_LABEL)
    }

    /// A primary status line: `  jetpack  <msg>`.
    pub fn status(&self, msg: &str) {
        eprintln!("  {}  {}", self.gutter(), msg);
    }

    /// A secondary, indented detail line: `           ▸ <msg>`.
    pub fn detail(&self, msg: &str) {
        let pad = " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4);
        eprintln!("{pad}{} {}", self.gray("▸"), msg);
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
        let symbol = self.paint(mark.sgr(), mark.symbol());
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
        eprintln!("{pad}{}", self.render_plan_row(mark, name, name_w, from, to));
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
    /// supplies the package), while `done/total` counts completed nodes.
    /// State is an actual phase (`resolving`, `building`, `substituting`), not
    /// an animation label inferred after the fact.
    pub fn render_dependency_status(
        &self,
        phase: &str,
        done: usize,
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
            "{phase} {done}/{total} {} {edge} {} {}",
            self.gray("·"),
            self.gray("·"),
            self.gray(state),
        )
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

    /// `N packages ready ✓` / `1 package ready ✓`.
    pub fn render_ready_summary(&self, count: usize) -> String {
        let noun = if count == 1 { "package" } else { "packages" };
        format!("{count} {noun} ready {}", self.green("✓"))
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
    pub fn render_live_header(&self, verb: &str, done: usize, total: usize, current: &str) -> String {
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
        let mut line = format!("{} {} ", self.gray("──"), self.paint("1;36", label));
        if !tail.is_empty() {
            line.push_str(&format!("{} {} ", self.gray("─"), self.gray(&tail)));
        }
        line.push_str(&self.gray(&fill));
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
        let handle = std::thread::spawn(move || {
            const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i = 0usize;
            while !flag.load(Ordering::Relaxed) {
                eprint!("\r{pad}\x1b[36m{}\x1b[0m {msg}", FRAMES[i % FRAMES.len()]);
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

    /// A `jet explain`-style error block: a red headline, a why line, and a
    /// fix pointer. Beginner-first voice (docs/spec/diagnostics.md diagnostics).
    pub fn error(&self, headline: &str, why: &str, fix: &str) {
        eprintln!();
        eprintln!("  {} {}", self.red("error:"), self.bold(headline));
        eprintln!("    {}", why);
        if !fix.is_empty() {
            eprintln!("    {} {}", self.gray("fix:"), fix);
        }
        eprintln!();
    }

    /// A coded error block: `error[E1230]: <headline>`, matching the compiler's
    /// `error[Exxxx]:` house style so `jet explain <code>` has a referent.
    pub fn error_coded(&self, code: &str, headline: &str, why: &str, fix: &str) {
        eprintln!();
        eprintln!(
            "  {} {}",
            self.red(&format!("error[{code}]:")),
            self.bold(headline)
        );
        eprintln!("    {}", why);
        if !fix.is_empty() {
            eprintln!("    {} {}", self.gray("fix:"), fix);
        }
        eprintln!();
    }

    /// A coded warning block: `warning[L0205]: <headline>`.
    pub fn warning_coded(&self, code: &str, headline: &str, why: &str, fix: &str) {
        eprintln!();
        eprintln!(
            "  {} {}",
            self.yellow(&format!("warning[{code}]:")),
            self.bold(headline)
        );
        eprintln!("    {}", why);
        if !fix.is_empty() {
            eprintln!("    {} {}", self.gray("fix:"), fix);
        }
        eprintln!();
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

    /// Project one dependency edge. TTY redraws a two-line pinned region;
    /// non-TTY appends exactly one stable chain line per node so CI retains
    /// progress without cursor control, timing noise, or duplicate bars.
    pub fn set_dependency_status(
        &mut self,
        phase: &str,
        done: usize,
        total: usize,
        source: &str,
        node: &str,
        state: &str,
    ) {
        let line = self
            .theme
            .render_dependency_status(phase, done, total, source, node, state);
        if !self.tty {
            eprintln!("{}{}", Self::pad(), line);
            return;
        }
        self.set_status(&[
            line,
            format!(
                "{}  {}",
                Theme::render_progress_bar(done, total, 14),
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
    let example = "nixpkgs:fastfetch";
    match err {
        RefError::MissingSeparator(raw) => theme.error(
            &format!("`{raw}` is missing a source"),
            "Jetpack refs are written `<source>:<package>`.",
            &format!("try `{example}` or `github:owner/repo` — no `#`."),
        ),
        RefError::EmptyHalf(raw) => theme.error(
            &format!("`{raw}` has an empty source or package"),
            "Both halves of `<source>:<package>` must be filled in.",
            &format!("try `{example}`."),
        ),
        RefError::UnknownSource {
            source, declared, ..
        } => {
            let known = if declared.is_empty() {
                "Sources are the built-ins `nixpkgs`, `github`, and `path`, or names you \
                 declare in env.jet with `pkg.source(...)`."
                    .to_string()
            } else {
                format!(
                    "Sources are the built-ins `nixpkgs`, `github`, `path`, or names declared \
                     in env.jet. This env declares: {}.",
                    declared.join(", ")
                )
            };
            theme.error(
                &format!("`{source}` is not a known source"),
                &known,
                &format!("add `pkg.source(\"{source}\", \"<upstream>\")`, or use a known name."),
            )
        }
        // D-MONOREF1=A: bare name with no separator that didn't match a workspace member.
        RefError::AmbiguousBare(raw) => theme.error(
            &format!("`{raw}` is ambiguous — no workspace member matches"),
            "A bare package name (no `source:` prefix) resolves against the workspace \
             member list. Either this name isn't a member, or there are multiple matches.",
            "use `source:package` or `source.package` to be explicit.",
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
            "address one member by its path (e.g. `packages/logging`), or use `source:package`.",
        ),
        // E1231: a bare/path ref matched no workspace member.
        RefError::UnknownMember { query, suggestions } => {
            let did_you_mean = if suggestions.is_empty() {
                "check `workspace.jet` — that name isn't in the member index.".to_string()
            } else {
                format!("did you mean: {}?", suggestions.join(", "))
            };
            theme.error_coded(
                "E1231",
                &format!("`{query}` is not a workspace member"),
                "A bare or path-form ref (no `source:` prefix) must name a member listed in the \
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
    fn progress_chain_has_deterministic_plain_fallback() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_progress_chain("realize", 2, 5, "ripgrep", "nixpkgs"),
            "▸ realize 2/5 ripgrep -> nixpkgs"
        );
    }

    #[test]
    fn dependency_status_uses_real_edge_direction_and_stable_plain_text() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_dependency_status(
                "building",
                2,
                5,
                "nixpkgs",
                "ripgrep",
                "resolving",
            ),
            "building 2/5 · nixpkgs -> ripgrep · resolving"
        );
    }

    #[test]
    fn dependency_status_handles_first_party_node_without_invented_parent() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_dependency_status("building", 1, 1, "", "local-app", "building"),
            "building 1/1 · local-app · building"
        );
    }

    // -- Tier 1: ready rows / summary --

    #[test]
    fn ready_row_has_no_state_column() {
        let theme = Theme { color: false };
        assert_eq!(
            theme.render_ready_row("ripgrep", 8, "14.1.0"),
            "✓ ripgrep   14.1.0"
        );
        assert_eq!(theme.render_ready_row("fd", 8, "9.0.0"), "✓ fd        9.0.0");
    }

    #[test]
    fn ready_row_unknown_version_is_a_dash() {
        let theme = Theme { color: false };
        assert_eq!(theme.render_ready_row("mystery", 8, ""), "✓ mystery   —");
    }

    #[test]
    fn ready_summary_pluralizes() {
        let theme = Theme { color: false };
        assert_eq!(theme.render_ready_summary(1), "1 package ready ✓");
        assert_eq!(theme.render_ready_summary(2), "2 packages ready ✓");
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
            theme.render_live_header("building", 31, 42, "linux"),
            "building 31/42 · linux"
        );
    }

    // -- Tier 3: plan gen header / download line --

    #[test]
    fn plan_gen_header_reads_gen_arrow_gen() {
        assert_eq!(
            Theme::render_plan_gen_header(42, 43),
            "Plan  gen 42 → 43"
        );
    }

    #[test]
    fn download_line_uses_human_size() {
        assert_eq!(Theme::render_download_line(240_000_000), "Download 240 MB");
        assert_eq!(Theme::render_download_line(0), "Download 0 B");
        assert_eq!(Theme::render_download_line(512), "Download 512 B");
        assert_eq!(Theme::render_download_line(1_300_000_000), "Download 1.3 GB");
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
