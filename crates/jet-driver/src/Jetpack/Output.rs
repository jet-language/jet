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
    /// column stays put.
    pub fn row(&self, name: &str, name_w: usize, version: &str, state: &str) {
        let pad = " ".repeat(Syntax::JETPACK_PROMPT_LABEL.len() + 4);
        let v = if version.is_empty() { "—" } else { version };
        eprintln!(
            "{pad}{} {}  {}  {}",
            self.green("✓"),
            self.bold(&format!("{name:<name_w$}")),
            self.gray(&format!("{v:<8}")),
            self.gray(state),
        );
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
