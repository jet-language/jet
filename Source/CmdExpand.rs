//! D-EXPANDCLI1=A (card #183): `jet inspect expand` — the transparency command.
//! Prints the facts sema already proved for one lens (`--facts <lens>`), or
//! every lens grouped (bare `jet inspect expand <file>`).
//!
//! I2/I3: this never runs a second analysis and never asks rustc. Every fact
//! comes straight off the checked `ProgramBundle` (`Func::is_inline` /
//! `is_inline_always` — present on a bundle that compiled at all means the
//! `#InlineAlways` promise already held, E0917/E0918/E0919 would have fired
//! otherwise).

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet::Sema::SemIndexEffectFacts;
use jet::AST::{Item, ProgramBundle};

/// One registered lens: name, one-line description for `--facts <unknown>`
/// listings and the bare-form group header, and the renderer that turns a
/// checked bundle + its facts into output lines. A table, not a match
/// cascade — adding a lens later (effects/layout/derive expansion) is one
/// row here (D-EXPANDCLI1: "other ratified surfaces add lenses under the
/// same flag, never new commands").
struct Lens {
    name: &'static str,
    summary: &'static str,
    render: fn(&ProgramBundle, &SemIndexEffectFacts) -> Vec<String>,
}

const LENSES: &[Lens] = &[
    Lens {
        name: "inline",
        summary: "#Inline / #InlineAlways contracts (D-METHODMACRO1)",
        render: render_inline,
    },
    Lens {
        name: "memory",
        summary: "transitive no_alloc / zero_rc / arena_bounded facts (D-MEM-FACTS1)",
        render: render_memory,
    },
];

pub(crate) fn run_expand(args: &[String], _json: bool) {
    let mut lens_name: Option<String> = None;
    let mut positional: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix("--facts=") {
            lens_name = Some(v.to_string());
        } else if a == "--facts" {
            match it.next() {
                Some(v) => lens_name = Some(v.clone()),
                None => {
                    eprintln!("error: `--facts` needs a lens name");
                    print_available_lenses();
                    exit(ExitCodes::USER_ERROR);
                }
            }
        } else if !a.starts_with('-') {
            positional.push(a.as_str());
        }
    }

    let Some(path) = positional.first().copied() else {
        eprintln!("error: `jet inspect expand` needs an entry file");
        eprintln!(" Fix: jet inspect expand examples/features/basics/hello.jet");
        eprintln!(" Fix: jet inspect expand --facts inline examples/features/basics/hello.jet");
        exit(ExitCodes::USER_ERROR);
    };

    let selected: Vec<&Lens> = match &lens_name {
        Some(name) => match LENSES.iter().find(|l| l.name == name) {
            Some(l) => vec![l],
            None => {
                eprintln!("error: unknown lens `{}`", name);
                print_available_lenses();
                exit(ExitCodes::USER_ERROR);
            }
        },
        None => LENSES.iter().collect(),
    };

    let abs = absolutize(path);
    let entry = abs.display().to_string();
    let (diags, bundle, facts) = jet::Driver::check_file_with_effect_facts(&entry, None, false);
    let has_errors = diags
        .iter()
        .any(|d| d.severity == jet::Diagnostics::Severity::Error);
    let Some(bundle) = (if has_errors { None } else { bundle }) else {
        for d in &diags {
            eprintln!(
                "{}",
                jet::render_diagnostics(&entry, "", std::slice::from_ref(d))
            );
        }
        exit(ExitCodes::USER_ERROR);
    };

    // Bare form (`lens_name` is `None`): magic default, every lens, grouped
    // under a header, empty lenses skipped entirely so the output stays
    // readable on a program that only uses one of the mechanisms.
    let bare = lens_name.is_none();
    let mut printed_any = false;
    for lens in &selected {
        let lines = (lens.render)(&bundle, &facts);
        if bare && lines.is_empty() {
            continue;
        }
        if printed_any {
            println!();
        }
        println!(
            "{} — {} ({} fact{})",
            lens.name,
            lens.summary,
            lines.len(),
            if lines.len() == 1 { "" } else { "s" }
        );
        if lines.is_empty() {
            println!("  (none in this program)");
        } else {
            for l in &lines {
                println!("  {}", l);
            }
        }
        printed_any = true;
    }
    if !printed_any {
        println!("no facts for any lens in this program");
    }
    exit(ExitCodes::OK);
}

fn print_available_lenses() {
    eprintln!(" available lenses:");
    for l in LENSES {
        eprintln!("   {:<8} {}", l.name, l.summary);
    }
}

fn render_memory(_bundle: &ProgramBundle, facts: &SemIndexEffectFacts) -> Vec<String> {
    let mut lines = facts
        .memory_declarations
        .iter()
        .flat_map(|declaration| {
            declaration.roots.iter().map(|root| {
                let status = match facts
                    .memory_projections
                    .get(&(root.clone(), declaration.fact))
                {
                    Some(jet::Sema::MemoryProjection::Proven) => "proven".to_string(),
                    Some(jet::Sema::MemoryProjection::Violated { call_path, operation }) => {
                        format!("violated by {operation} through {}", call_path.join(" -> "))
                    }
                    Some(jet::Sema::MemoryProjection::OpenWorld { call_path, reason }) => {
                        format!("open through {}: {reason}", call_path.join(" -> "))
                    }
                    None => "not projected".to_string(),
                };
                format!(
                    "{}: {} — {} ({})",
                    root,
                    declaration.fact.display(),
                    status,
                    declaration.provenance
                )
            })
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

/// D-METHODMACRO1=A: every `#Inline`/`#InlineAlways` fn or method in the
/// bundle, and the Rust attribute codegen emits for it. Functions with
/// neither marker produce no line (the ballot: don't dump everything).
fn render_inline(bundle: &ProgramBundle, _facts: &SemIndexEffectFacts) -> Vec<String> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        let mut in_module = collect_inline_facts(&module.items, None);
        in_module.sort_by_key(|(_, span, _, _)| span.start);
        for (qualified, span, contract, attr) in in_module {
            let (line, col) = jet::Diagnostics::span_line_col(&module.source, span.start);
            out.push(format!(
                "{}:{}:{}   {}   {}   -> {}",
                module.display, line, col, qualified, contract, attr
            ));
        }
    }
    out
}

/// Walk one item list for `#Inline`/`#InlineAlways` functions and methods,
/// recursing into struct/enum trait-impl blocks and inline code modules
/// (D-MOD2) so nothing declared inside one is missed.
type InlineRow = (String, jet::Diagnostics::Span, &'static str, &'static str);

/// One `#Inline`/`#InlineAlways` fn or method: its qualified name, marker
/// span, contract label, and the Rust attribute codegen emits. `None` for
/// neither marker (the ballot: don't dump every function, only the ones
/// carrying a contract).
fn inline_row(f: &jet::AST::Func, owner: Option<&str>) -> Option<InlineRow> {
    let (contract, attr) = if f.is_inline_always {
        ("#InlineAlways", "#[inline(always)]")
    } else if f.is_inline {
        ("#Inline", "#[inline]")
    } else {
        return None;
    };
    let qualified = match owner {
        Some(t) => format!("{}.{}", t, f.name),
        None => f.name.clone(),
    };
    let span = f.inline_span.unwrap_or(f.name_span);
    Some((qualified, span, contract, attr))
}

fn collect_inline_facts(items: &[Item], owner_type: Option<&str>) -> Vec<InlineRow> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Item::Func(f) => out.extend(inline_row(f, owner_type)),
            Item::Struct(s) => {
                for m in &s.methods {
                    out.extend(inline_row(m, Some(&s.name)));
                }
                for ti in &s.trait_impls {
                    for m in &ti.methods {
                        out.extend(inline_row(m, Some(&s.name)));
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    out.extend(inline_row(m, Some(&e.name)));
                }
                for ti in &e.trait_impls {
                    for m in &ti.methods {
                        out.extend(inline_row(m, Some(&e.name)));
                    }
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    out.extend(inline_row(m, Some(&i.type_name)));
                }
            }
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    out.extend(collect_inline_facts(body, owner_type));
                }
            }
            _ => {}
        }
    }
    out
}

fn absolutize(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}
