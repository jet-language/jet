//! `jet explain <CODE>` — offline terminal essays for every diagnostic code.
//!
//! The index is built from the typed compile-time diagnostic rows, so
//! `explain` works with no network and no files on disk. Every row gets an
//! entry by construction (invariant I4: no code without an explain), and any
//! row that has a detailed *what/why/fix* template gets the richer essay.

use jet_foundation::Terminal::Theme;
use std::collections::BTreeMap;

/// One explainable diagnostic code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    pub code: String,
    /// Pipeline stage (`jet` / `parse` / `sema` / …), from the registry table.
    pub stage: String,
    /// One-line meaning from the registry table (always present).
    pub meaning: String,
    /// Plain-language "what happened", from a detailed row (when present).
    pub what: Option<String>,
    /// The rule behind it, from a detailed row (when present).
    pub why: Option<String>,
    /// A concrete next step, from a detailed row (when present).
    pub fix: Option<String>,
    /// True when the registry marks the code as retired (kept for history).
    pub retired: bool,
}

/// Build the full code → explanation index from the embedded spec.
pub fn index() -> BTreeMap<String, Explanation> {
    let mut out: BTreeMap<String, Explanation> = BTreeMap::new();
    for row in jet_foundation::Registry::diagnostic_rows() {
        out.insert(
            row.code.to_string(),
            Explanation {
                code: row.code.to_string(),
                stage: row.stage.to_string(),
                meaning: row.meaning.to_string(),
                what: row.detail.then(|| row.what.to_string()),
                why: row.detail.then(|| row.why.to_string()),
                fix: row.detail.then(|| row.fix.to_string()),
                retired: row.status == jet_foundation::Registry::DiagnosticStatus::Retired,
            },
        );
    }

    out
}

/// Live codes (non-retired). Used to verify I4: every registered code
/// resolves through `jet explain`.
pub fn live_codes() -> Vec<String> {
    index()
        .into_values()
        .filter(|e| !e.retired)
        .map(|e| e.code)
        .collect()
}

/// Render the generated diagnostic-row reference. The committed Markdown is a
/// checked artifact; this function is its only producer.
pub fn diagnostics_reference_markdown() -> String {
    let mut out = String::from(
        "# Typed diagnostic rows\n\nGenerated from `crates/jet-codegen/src/Prelude/Diagnostics.jet`.\n\n| Code | Stage | Severity | Moment | Status | Meaning | What | Why | Fix |\n|---|---|---|---|---|---|---|---|---|\n",
    );
    for row in jet_foundation::Registry::diagnostic_rows() {
        let severity = match row.severity {
            jet_foundation::Diagnostics::Severity::Error => "error",
            jet_foundation::Diagnostics::Severity::Lint => "lint",
        };
        let cells = [
            row.code,
            row.stage,
            severity,
            row.moment.as_str(),
            row.status.name(),
            row.meaning,
            row.what,
            row.why,
            row.fix,
        ];
        out.push('|');
        for cell in cells {
            out.push(' ');
            out.push_str(&escape_markdown_cell(cell));
            out.push(' ');
            out.push('|');
        }
        out.push('\n');
    }
    out
}

fn escape_markdown_cell(value: &str) -> String {
    value.replace('\\', "\\\\").replace('|', "\\|")
}

/// Look up one code (case-insensitive).
pub fn lookup(code: &str) -> Option<Explanation> {
    let want = normalize(code);
    index()
        .into_iter()
        .find_map(|(k, v)| if normalize(&k) == want { Some(v) } else { None })
        .or_else(|| {
            let key = jet_foundation::Policy::PolicyKey::parse(code.trim())?;
            Some(Explanation {
                code: key.name().to_string(),
                stage: "policy".to_string(),
                meaning: format!("compiler-owned scoped policy `{}`", key.name()),
                what: Some(format!("`{}` participates in the package → module → function → block policy ladder.", key.name())),
                why: Some("one registry owns applicability, inheritance, conflicts, and provenance".to_string()),
                fix: Some("inspect the semantic index at the target site for the effective value and full declaration chain".to_string()),
                retired: false,
            })
        })
        .or_else(|| {
            // D-META-REG1=A: one lookup over the one registration table. A
            // marker, a knowledge plane, a right, and a build fact are rows of
            // the same table, so `jet explain` has one path, not one per kind.
            let registered = jet_foundation::Registry::row(code.trim())?;
            let Some(row) = registered.rule else {
                return Some(explain_fact_row(registered));
            };
            let (retired, replacement) = match row.status {
                jet_foundation::Policy::RuleStatus::Active => (false, None),
                jet_foundation::Policy::RuleStatus::Retired { replacement } => {
                    (true, Some(replacement))
                }
            };
            Some(Explanation {
                code: row.name.to_string(),
                stage: "rule applicability".to_string(),
                meaning: format!("`#{}{}`", row.name, row.signature.render()),
                // D-MARK-FORM1=A: there is no written-form column. The row's
                // own signature says whether parentheses may and must appear.
                what: Some(format!(
                    "arguments: {}; repeatable: {}; status: {:?}; attachment sites: {:?}.{}",
                    if row.signature.arguments_required() {
                        "required"
                    } else if row.signature.accepts_arguments() {
                        "optional"
                    } else {
                        "none"
                    },
                    row.repeatable,
                    row.status,
                    row.sites,
                    marker_argument_declarations(row)
                )),
                why: Some(format!("resolution is {:?}; site-bound authority never becomes ambient policy", row.resolution)),
                fix: Some(replacement.map_or_else(
                    || "move the rule to one of its registered sites".to_string(),
                    |replacement| format!("replace it with `{replacement}`"),
                )),
                retired,
            })
        })
}

/// D-FACT-LAW1=B: a row that is not a rule on written code answers with the one
/// law it obeys — which way its facts tighten for free, and the written words
/// that loosen them. D-FACT-OWN1=A: a row a prover publishes is read-only, so it
/// states no direction and names no gate.
fn explain_fact_row(row: &jet_foundation::Registry::RegistryRow) -> Explanation {
    use jet_foundation::Registry::{RowTarget, SafeDirection};

    let attaches_to = match row.target {
        RowTarget::Code(_) => "written code",
        RowTarget::Value => "a value",
        RowTarget::Scope => "a scope",
        RowTarget::Build => "the build",
        RowTarget::Corpus => "the compiler's own source",
        RowTarget::Report => "the diagnostic report",
    };
    // D-ONCE-LAW1=A: a corpus truth answers with its home, its renderers, and
    // the guard that proves there is no second copy.
    if let (Some(home), Some(guard)) = (row.home, row.guard) {
        return Explanation {
            code: row.name.to_string(),
            stage: "registration table".to_string(),
            meaning: format!(
                "`{}` — a truth stated once, in {home} ({})",
                row.name, row.decision
            ),
            what: Some(format!("rendered from there by: {}.", row.renderers.join(", "))),
            why: Some(format!(
                "`{}` in {} proves there is no second copy; it {}",
                guard.test,
                guard.file,
                guard.proof.name()
            )),
            fix: Some(format!(
                "change the meaning in {home}; a second copy anywhere else fails the guard"
            )),
            retired: false,
        };
    }
    Explanation {
        code: row.name.to_string(),
        stage: "registration table".to_string(),
        meaning: format!(
            "`{}` — a registered {} on {} ({})",
            row.name,
            row.kind().name(),
            attaches_to,
            row.decision
        ),
        what: Some(match row.published_by {
            Some(prover) => format!(
                "the {prover} prover publishes this row; it is read-only and carries no plane algebra."
            ),
            None => format!("attaches to {attaches_to}."),
        }),
        why: Some(match row.safe_direction {
            SafeDirection::None => {
                "this row holds no fact that moves, so it states no safe direction".to_string()
            }
            direction => format!(
                "facts tighten silently in the `{}` direction; loosening one is always written",
                direction.name()
            ),
        }),
        fix: Some(if row.gates.is_empty() {
            "nothing loosens this row; read it and act on what it says".to_string()
        } else {
            format!("to loosen it, write one of: {}", row.gates.join(", "))
        }),
        retired: false,
    }
}

fn marker_argument_declarations(row: &jet_foundation::Policy::AppliedRule) -> String {
    let mut declarations = row
        .signature
        .params
        .iter()
        .filter_map(|param| {
            jet_foundation::Policy::rule_arg_declaration(param.source_type)
                .map(|_| format!(" `core.lang.{}`", param.source_type))
        })
        .collect::<Vec<_>>();
    if let Some(source_type) = row.signature.variadic_source_type {
        if jet_foundation::Policy::rule_arg_declaration(source_type).is_some() {
            declarations.push(format!(" `core.lang.{source_type}`"));
        }
    }
    declarations.sort();
    declarations.dedup();
    if declarations.is_empty() {
        String::new()
    } else {
        format!(" Argument declarations: {}.", declarations.join(","))
    }
}

/// Existing `jet explain` rendering for one effective policy at a concrete site.
pub fn lookup_policy(key: jet_foundation::Policy::PolicyKey, declarations: impl IntoIterator<Item = jet_foundation::Policy::PolicyDeclaration>) -> Option<Explanation> {
    let effective = jet_foundation::Policy::resolve(key, declarations).ok()??;
    Some(Explanation {
        code: key.name().to_string(),
        stage: "policy".to_string(),
        meaning: format!("effective scoped policy `{}`", key.name()),
        what: Some(jet_foundation::Policy::explain(&effective)),
        why: Some("the nearest applicable declaration wins subject to the registry's tightening and conflict rules".to_string()),
        fix: Some("change the nearest declaration, or remove it to inherit the next outer value".to_string()),
        retired: false,
    })
}

/// Render the offline essay for a code. Uses readable, beginner-friendly
/// headers. `color` bolds the code line on a TTY.
pub fn render(ex: &Explanation, color: bool) -> String {
    let mut out = String::new();
    let theme = Theme::new(color);
    out.push_str(&format!("{}\n\n", theme.accent(&ex.code)));
    if ex.retired {
        out.push_str("This code is retired: it is no longer produced by the current\n");
        out.push_str("compiler, and is kept here only so old build logs stay readable.\n");
        return out;
    }
    let what = ex.what.as_deref().or(Some(ex.meaning.as_str()));
    if let Some(w) = what {
        out.push_str(&format!("{}\n  {}\n\n", theme.bold("What this means:"), w));
    }
    if let Some(why) = &ex.why {
        out.push_str(&format!(
            "{}\n  {}\n\n",
            theme.bold(&format!("Why {} enforces it:", crate::Syntax::LANG_NAME)),
            why
        ));
    }
    if let Some(fix) = &ex.fix {
        out.push_str(&format!("{}\n  {}\n\n", theme.bold("How to fix it:"), fix));
    }
    if ex.what.is_none() {
        // No detailed block yet: show stage so the entry is still useful.
        if !ex.stage.is_empty() {
            out.push_str(&format!("Stage: {}\n\n", ex.stage));
        }
        out.push_str(
            "A longer explanation will land with the detailed typed row.\n\n",
        );
    }
    out.push_str(&format!(
        "This explanation comes from {}'s diagnostics reference.\n",
        crate::Syntax::BINARY_NAME
    ));
    out
}

/// The teaching pointer appended after a rendered error (one dim line).
/// Suppressed in `--json` (the code is already structured there).
pub fn pointer_line(code: &str, color: bool) -> String {
    let body = format!(
        "run `{} explain {}` to learn more",
        crate::Syntax::BINARY_NAME,
        code
    );
    Theme::new(color).dim(&body)
}

fn normalize(code: &str) -> String {
    code.trim().to_uppercase()
}

pub fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() == 5 && b[1..].iter().all(|c| c.is_ascii_digit()) {
        return matches!(b[0], b'E' | b'L')
            || (b[0] == b'W' && b[1..] == *b"0410");
    }
    let Some(rest) = s.strip_prefix("E-").or_else(|| s.strip_prefix("L-")) else {
        return false;
    };
    let mut words = rest.split('-');
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    };
    words.next().is_some_and(valid)
        && words.next().is_some_and(valid)
        && words.all(valid)
}

/// D-ONCE-LAW1=A: `jet inspect facts` reads the one registration table. Every
/// registered row is listed, and a corpus truth also shows its home, everything
/// that renders it, and the guard that proves there is no second copy.
pub fn facts_report_text() -> String {
    use jet_foundation::Registry::{self, RowKind};

    let mut out = String::new();
    for kind in [
        RowKind::Truth,
        RowKind::Plane,
        RowKind::Right,
        RowKind::Fact,
        RowKind::Marker,
        RowKind::Diagnostic,
    ] {
        let rows: Vec<_> = Registry::rows()
            .iter()
            .filter(|row| row.kind() == kind)
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("{}s ({}):\n", kind.name(), rows.len()));
        for row in rows {
            out.push_str(&format!("  {:<20} {}\n", row.name, row.decision));
            if let Some(home) = row.home {
                out.push_str(&format!("    home      {home}\n"));
                out.push_str(&format!("    renders   {}\n", row.renderers.join(", ")));
            }
            if let Some(guard) = row.guard {
                out.push_str(&format!(
                    "    guard     {} in {} ({})\n",
                    guard.test,
                    guard.file,
                    guard.proof.name()
                ));
            }
        }
        out.push('\n');
    }
    out
}

/// `--json` render of `jet inspect facts`.
pub fn facts_report_json() -> String {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let rows: Vec<String> = jet_foundation::Registry::rows()
        .iter()
        .map(|row| {
            let mut fields = vec![
                format!("\"name\":\"{}\"", esc(row.name)),
                format!("\"kind\":\"{}\"", row.kind().name()),
                format!("\"decision\":\"{}\"", esc(row.decision)),
                format!("\"safe_direction\":\"{}\"", row.safe_direction.name()),
            ];
            if let Some(home) = row.home {
                fields.push(format!("\"home\":\"{}\"", esc(home)));
                let renderers: Vec<String> = row
                    .renderers
                    .iter()
                    .map(|r| format!("\"{}\"", esc(r)))
                    .collect();
                fields.push(format!("\"renderers\":[{}]", renderers.join(",")));
            }
            if let Some(guard) = row.guard {
                fields.push(format!(
                    "\"guard\":{{\"test\":\"{}\",\"file\":\"{}\",\"proof\":\"{}\"}}",
                    esc(guard.test),
                    esc(guard.file),
                    guard.proof.name()
                ));
            }
            format!("{{{}}}", fields.join(","))
        })
        .collect();
    format!(
        "{{\"schema_version\":1,\"rows\":[{}]}}",
        rows.join(",")
    )
}

#[cfg(test)]
mod marker_registry_tests {
    /// D-ONCE-LAW1=A: the report lists every registered truth, with the guard
    /// that proves it. A truth missing from the report is a truth nobody can
    /// audit.
    #[test]
    fn inspect_facts_lists_every_registered_truth() {
        let text = super::facts_report_text();
        let json = super::facts_report_json();
        let mut truths = 0;
        for row in jet_foundation::Registry::truths() {
            truths += 1;
            let home = row.home.expect("a truth names a home");
            let guard = row.guard.expect("a truth names a guard");
            assert!(text.contains(row.name), "`{}` is missing from the report", row.name);
            assert!(text.contains(home), "`{}` does not show its home", row.name);
            assert!(text.contains(guard.test), "`{}` does not show its guard", row.name);
            assert!(json.contains(guard.test), "`{}` is missing from --json", row.name);
        }
        assert!(truths >= 7, "the registry is born non-empty");
        // Every other kind is listed too, so one report reads the whole table.
        assert!(text.contains("Exactness") && text.contains("Rights"));
    }

    /// A corpus truth explains itself with its home and its guard.
    #[test]
    fn explain_reads_a_corpus_truth() {
        let ice = super::lookup("IceReport").expect("IceReport is a registered truth");
        assert!(ice.meaning.contains("crates/jet-foundation/src/Diagnostics.rs"));
        assert!(ice
            .why
            .as_deref()
            .is_some_and(|why| why.contains("no_hand_typed_ice_banner_outside_the_one_home")));
    }

    #[test]
    fn marker_explain_reports_typed_signature_and_retirement() {
        let inline = super::lookup("Inline").expect("Inline registry explanation");
        assert_eq!(inline.meaning, "`#Inline(mode: InlineMode = .Hint)`");
        assert!(!inline.retired);

        let pure = super::lookup("Pure").expect("Pure retirement explanation");
        assert!(pure.retired);
        assert_eq!(pure.fix.as_deref(), Some("replace it with `=[]=>`"));
    }
}
