//! Jetpack terminal output: quiet, aligned, colored, TTY-aware.
//!
//! Mirrors the `nh`/ansi aesthetic from `examples/jetos/lib/ansi.jet`. A single
//! `Theme` carries one `color` flag so `--no-color` / `NO_COLOR` flips every
//! line through one value instead of scattered globals. Everything Jetpack says
//! to the user goes through here (D-JPK14 beauty requirement).

use super::RefSpec::RefError;
use crate::Syntax;
use std::io::IsTerminal;

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

    /// A plain note (no gutter), e.g. the "entering a temporary shell" banner.
    pub fn note(&self, msg: &str) {
        eprintln!("\n  {}\n", self.gray(msg));
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
    }
}
