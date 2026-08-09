//! D-EXPANDCLI1=A (card #183): `jet inspect expand` — the transparency command.
//! Prints the facts sema already proved for one lens (`--facts <lens>`), or
//! every lens grouped (bare `jet inspect expand <file>`).
//!
//! I2/I3: this never runs a second analysis and never asks rustc. Every fact
//! comes straight off the checked `ProgramBundle` (`Func::is_inline` /
//! `is_inline_always` — present on a bundle that compiled at all means the
//! `#Inline(Always)` promise already held, E0917/E0918/E0919 would have fired
//! otherwise).

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet::Sema::SemIndexEffectFacts;
use jet::AST::{Item, ProgramBundle};
use jet_semindex::{ExpandLens, ExpandProjection, ExpandValue};

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
    render_json: fn(&ProgramBundle, &SemIndexEffectFacts) -> Vec<ExpandValue>,
}

const LENSES: &[Lens] = &[
    Lens {
        name: "inline",
        summary: "#Inline / #Inline(Always) contracts (D-METHODMACRO1)",
        render: render_inline,
        render_json: render_inline_json,
    },
    Lens {
        name: "memory",
        summary: "transitive no_alloc / zero_rc / arena_bounded facts (D-MEM-FACTS1)",
        render: render_memory,
        render_json: render_memory_json,
    },
    Lens {
        name: "web",
        summary: "D-WEBAPP1 application graph facts (routes/actions/policy)",
        render: render_web,
        render_json: render_web_json,
    },
    Lens {
        name: "layout",
        summary: "D-LAYOUT-FACTS1 compiler-owned type layout facts",
        render: render_layout,
        render_json: render_layout_json,
    },
];

pub(crate) fn run_expand(args: &[String], json: bool) {
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
                    if json {
                        print_json_cli_error(
                            "E2104",
                            "`--facts` needs a lens name",
                            "the expand command must know which registered lens to project",
                            "pass `--facts inline`, `--facts memory`, `--facts web`, or `--facts layout`",
                        );
                    }
                    if !json {
                        crate::cli_error!("E2104", "`--facts` needs a lens name");
                        print_available_lenses();
                    }
                    exit(ExitCodes::USER_ERROR);
                }
            }
        } else if !a.starts_with('-') {
            positional.push(a.as_str());
        }
    }

    let Some(path) = positional.first().copied() else {
        if json {
            print_json_cli_error(
                "E2104",
                "`jet inspect expand` needs an entry file",
                "expand facts come from one checked Jet entry file",
                "run `jet inspect expand --facts inline examples/features/basics/hello.jet`",
            );
        }
        if !json {
            crate::cli_error!("E2104", "`jet inspect expand` needs an entry file");
        }
        exit(ExitCodes::USER_ERROR);
    };

    let selected: Vec<&Lens> = match &lens_name {
        Some(name) => match LENSES.iter().find(|l| l.name == name) {
            Some(l) => vec![l],
            None => {
                if json {
                    print_json_cli_error(
                        "E2941",
                        &format!("unknown expand lens `{name}`"),
                        "only registered lenses have checked semantic facts",
                        "use `inline`, `memory`, `web`, or `layout`",
                    );
                }
                if !json {
                    crate::cli_error!("E2941", "unknown expand lens `{}`", name);
                    print_available_lenses();
                }
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
        if json {
            let source = std::fs::read_to_string(&abs).unwrap_or_default();
            print_json_frontend_diagnostics(&entry, &source, &diags);
        }
        for d in &diags {
            eprintln!(
                "{}",
                jet::render_diagnostics(&entry, "", std::slice::from_ref(d))
            );
        }
        exit(ExitCodes::USER_ERROR);
    };

    // JSON is the canonical semantic-index document with one additive
    // `expand` projection. The checked bundle and effect facts above are the
    // only inputs: this path never re-checks or asks rustc for an answer.
    if json {
        let index = jet_semindex::from_checked(&bundle, &facts);
        let selection = lens_name.as_deref().unwrap_or("all");
        let lenses = selected
            .iter()
            .map(|lens| ExpandLens {
                name: lens.name.to_string(),
                summary: lens.summary.to_string(),
                facts: (lens.render_json)(&bundle, &facts),
            })
            .collect();
        let expand = ExpandProjection {
            selection: selection.to_string(),
            lenses,
        };
        println!("{}", index.to_json_with_expand(&expand));
        exit(ExitCodes::OK);
    }

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

fn print_json_cli_error(code: &str, what: &str, why: &str, fix: &str) -> ! {
    crate::emit_cli_report(code, what.to_string(), why.to_string(), fix.to_string(), true);
    exit(ExitCodes::USER_ERROR);
}

fn print_json_frontend_diagnostics(file: &str, source: &str, diags: &[jet::Diagnostics::Diagnostic]) -> ! {
    print!("{}", jet::render_all_json(file, source, diags));
    exit(ExitCodes::USER_ERROR);
}

fn print_available_lenses() {
    eprintln!(" available lenses:");
    for l in LENSES {
        eprintln!("   {:<8} {}", l.name, l.summary);
    }
}

/// Quote CLI usage text with the same std-only JSON escaping used by the
/// semantic-index serializer.
fn json_string(value: &str) -> String {
    format!("\"{}\"", jet_foundation::JSON::json_escape(value))
}

fn expand_string(value: impl Into<String>) -> ExpandValue {
    ExpandValue::String(value.into())
}

fn expand_object(fields: Vec<(&str, ExpandValue)>) -> ExpandValue {
    ExpandValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn expand_string_list(values: &[String]) -> ExpandValue {
    ExpandValue::Array(values.iter().cloned().map(ExpandValue::String).collect())
}

fn expand_span(span: jet::Diagnostics::Span, location: Option<(usize, usize)>) -> ExpandValue {
    let mut fields = vec![
        ("start", ExpandValue::Number(span.start)),
        ("end", ExpandValue::Number(span.end)),
    ];
    add_location(&mut fields, location);
    expand_object(fields)
}

fn add_location(fields: &mut Vec<(&str, ExpandValue)>, location: Option<(usize, usize)>) {
    if let Some((line, column)) = location {
        fields.push(("line", ExpandValue::Number(line)));
        fields.push(("column", ExpandValue::Number(column)));
    }
}

fn source_location(
    bundle: &ProgramBundle,
    source: &str,
    offset: usize,
) -> Option<(usize, usize)> {
    bundle
        .modules
        .iter()
        .find(|module| module.display == source || module.path.to_string_lossy() == source)
        .map(|module| jet::Diagnostics::span_line_col(&module.source, offset))
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

fn render_memory_json(bundle: &ProgramBundle, facts: &SemIndexEffectFacts) -> Vec<ExpandValue> {
    let mut rows = Vec::new();
    for declaration in &facts.memory_declarations {
        for root in &declaration.roots {
            let location = source_location(bundle, &declaration.source, declaration.span.start);
            let status = match facts
                .memory_projections
                .get(&(root.clone(), declaration.fact))
            {
                Some(jet::Sema::MemoryProjection::Proven) => {
                    expand_object(vec![("kind", expand_string("proven"))])
                }
                Some(jet::Sema::MemoryProjection::Violated { call_path, operation }) => {
                    expand_object(vec![
                        ("kind", expand_string("violated")),
                        ("operation", expand_string(operation)),
                        ("call_path", expand_string_list(call_path)),
                    ])
                }
                Some(jet::Sema::MemoryProjection::OpenWorld { call_path, reason }) => {
                    expand_object(vec![
                        ("kind", expand_string("open_world")),
                        ("reason", expand_string(reason)),
                        ("call_path", expand_string_list(call_path)),
                    ])
                }
                None => expand_object(vec![("kind", expand_string("not_projected"))]),
            };
            let mut fields = vec![
                ("fact", expand_string(declaration.fact.display())),
                ("root", expand_string(root)),
                ("source", expand_string(&declaration.source)),
                ("span", expand_span(declaration.span, location)),
                ("provenance", expand_string(&declaration.provenance)),
                ("status", status),
            ];
            add_location(&mut fields, location);
            rows.push(expand_object(fields));
        }
    }
    rows.sort_by_key(|row| format!("{row:?}"));
    rows
}

fn render_web(_bundle: &ProgramBundle, facts: &SemIndexEffectFacts) -> Vec<String> {
    let Some(graph) = facts.web_app.as_ref() else {
        return Vec::new();
    };
    if graph.entry_file.is_empty()
        && graph.routes.is_empty()
        && graph.actions.is_empty()
        && graph.mounts.is_empty()
        && graph.routes_from.is_empty()
    {
        return Vec::new();
    }
    graph.explain_lines()
}

fn render_web_json(bundle: &ProgramBundle, facts: &SemIndexEffectFacts) -> Vec<ExpandValue> {
    let Some(graph) = facts.web_app.as_ref() else {
        return Vec::new();
    };
    let routes = graph
        .routes
        .iter()
        .map(|route| {
            let location = source_location(bundle, &graph.entry_file, route.span_start);
            let mut fields = vec![
                ("path", expand_string(&route.path)),
                ("handler", expand_string(&route.handler)),
                ("render", expand_string(route.render.as_str())),
                ("provenance", expand_string(&route.provenance)),
                ("source", expand_string(&graph.entry_file)),
                ("span", expand_span(jet::Diagnostics::Span::new(route.span_start, route.span_end), location)),
            ];
            add_location(&mut fields, location);
            expand_object(fields)
        })
        .collect();
    let actions = graph
        .actions
        .iter()
        .map(|action| {
            let location = source_location(bundle, &graph.entry_file, action.span_start);
            let mut fields = vec![
                ("name", expand_string(&action.name)),
                ("handler", expand_string(&action.handler)),
                ("kind", expand_string(&action.kind)),
                ("provenance", expand_string(&action.provenance)),
                ("source", expand_string(&graph.entry_file)),
                ("span", expand_span(jet::Diagnostics::Span::new(action.span_start, action.span_end), location)),
            ];
            add_location(&mut fields, location);
            expand_object(fields)
        })
        .collect();
    let mounts = graph
        .mounts
        .iter()
        .map(|mount| {
            let location = source_location(bundle, &graph.entry_file, mount.span_start);
            let mut fields = vec![
                ("prefix", expand_string(&mount.prefix)),
                ("handler", expand_string(&mount.handler)),
                ("effects", expand_string_list(&mount.effects)),
                ("security", expand_string_list(&mount.security)),
                ("provenance", expand_string(&mount.provenance)),
                ("source", expand_string(&graph.entry_file)),
                ("span", expand_span(jet::Diagnostics::Span::new(mount.span_start, mount.span_end), location)),
            ];
            add_location(&mut fields, location);
            expand_object(fields)
        })
        .collect();
    let routes_from = graph
        .routes_from
        .iter()
        .map(|root| {
            let location = source_location(bundle, &graph.entry_file, root.span_start);
            let mut fields = vec![
                ("root", expand_string(&root.root)),
                ("source", expand_string(&graph.entry_file)),
                ("span", expand_span(jet::Diagnostics::Span::new(root.span_start, root.span_end), location)),
            ];
            add_location(&mut fields, location);
            expand_object(fields)
        })
        .collect();
    vec![expand_object(vec![
        ("kind", expand_string("web_graph")),
        ("source", expand_string(&graph.entry_file)),
        ("entry", expand_string(&graph.entry_file)),
        ("hydration", expand_string(&graph.hydration)),
        ("shared_tir", ExpandValue::Bool(graph.shared_tir)),
        ("routes", ExpandValue::Array(routes)),
        ("actions", ExpandValue::Array(actions)),
        ("mounts", ExpandValue::Array(mounts)),
        ("routes_from", ExpandValue::Array(routes_from)),
        ("policy", expand_object(vec![
            ("security", expand_string_list(&graph.policy.security)),
            ("assets", expand_string_list(&graph.policy.assets)),
            ("split", expand_string_list(&graph.policy.split)),
            ("cache", expand_string_list(&graph.policy.cache)),
            ("a11y", expand_string_list(&graph.policy.a11y)),
            ("adapters", expand_string_list(&graph.policy.adapters)),
        ])),
    ])]
}

/// D-METHODMACRO1=A: every `#Inline`/`#Inline(Always)` fn or method in the
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

fn ct_field<'a>(value: &'a jet::CtValue, name: &str) -> Option<&'a jet::CtValue> {
    let jet::CtValue::Struct { fields, .. } = value else {
        return None;
    };
    fields.iter().find(|(field, _)| field == name).map(|(_, value)| value)
}

fn ct_display(value: Option<&jet::CtValue>) -> String {
    match value {
        Some(jet::CtValue::Str(value)) => value.clone(),
        Some(jet::CtValue::Int(value)) => value.to_string(),
        Some(jet::CtValue::Failed(jet::CtReport::Clean(_))) | None => "unknown".to_string(),
        Some(value) => value.jet_show(),
    }
}

fn ct_to_expand(value: &jet::CtValue) -> ExpandValue {
    match value {
        jet::CtValue::Int(value) if *value >= 0 => ExpandValue::Number(*value as usize),
        jet::CtValue::Int(value) => ExpandValue::String(value.to_string()),
        jet::CtValue::Bool(value) => ExpandValue::Bool(*value),
        jet::CtValue::Str(value) => ExpandValue::String(value.clone()),
        jet::CtValue::Failed(jet::CtReport::Clean(_)) => ExpandValue::Null,
        jet::CtValue::List(values) => {
            ExpandValue::Array(values.iter().map(ct_to_expand).collect())
        }
        jet::CtValue::Struct { fields, .. } => ExpandValue::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), ct_to_expand(value)))
                .collect(),
        ),
        value => ExpandValue::String(value.jet_show()),
    }
}

fn layout_text(layout: &jet::CtValue) -> String {
    let header = ["kind", "size", "alignment", "stride", "target", "guarantee", "source"]
        .iter()
        .map(|name| format!("{name}={}", ct_display(ct_field(layout, name))))
        .collect::<Vec<_>>()
        .join(" ");
    let fields = match ct_field(layout, "fields") {
        Some(jet::CtValue::List(values)) => values
            .iter()
            .map(|field| {
                let name = ct_display(ct_field(field, "name"));
                let ty = ct_display(ct_field(field, "ty"));
                format!(
                    "{name}:{ty}(offset={},size={})",
                    ct_display(ct_field(field, "offset")),
                    ct_display(ct_field(field, "size")),
                )
            })
            .collect::<Vec<_>>()
            .join(","),
        _ => String::new(),
    };
    format!("{header} fields=[{fields}]")
}

struct LayoutRow {
    module: String,
    source: String,
    name: String,
    span: jet::Diagnostics::Span,
    layout: jet::CtValue,
}

fn collect_layout_rows(bundle: &ProgramBundle) -> Vec<LayoutRow> {
    let mut rows = Vec::new();
    for module in &bundle.modules {
        for item in &module.items {
            let (name, span, layout) = match item {
                Item::Struct(def) => (
                    def.name.clone(),
                    def.name_span,
                    jet::Comptime::build_struct_layout_info(def),
                ),
                Item::Enum(def) => (
                    def.name.clone(),
                    def.name_span,
                    jet::Comptime::build_enum_layout_info(def),
                ),
                _ => continue,
            };
            rows.push(LayoutRow {
                module: module.display.clone(),
                source: module.source.clone(),
                name,
                span,
                layout,
            });
        }
    }
    rows.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then(a.span.start.cmp(&b.span.start))
            .then(a.name.cmp(&b.name))
    });
    rows
}

fn render_layout(bundle: &ProgramBundle, _facts: &SemIndexEffectFacts) -> Vec<String> {
    collect_layout_rows(bundle)
        .into_iter()
        .map(|row| {
            let (line, column) = jet::Diagnostics::span_line_col(&row.source, row.span.start);
            format!(
                "{}:{}:{}   {}.$layout   {}",
                row.module,
                line,
                column,
                row.name,
                layout_text(&row.layout)
            )
        })
        .collect()
}

fn render_layout_json(bundle: &ProgramBundle, _facts: &SemIndexEffectFacts) -> Vec<ExpandValue> {
    collect_layout_rows(bundle)
        .into_iter()
        .map(|row| {
            let location = Some(jet::Diagnostics::span_line_col(&row.source, row.span.start));
            expand_object(vec![
                ("type", expand_string(&row.name)),
                ("source", expand_string(&row.module)),
                ("span", expand_span(row.span, location)),
                ("layout", ct_to_expand(&row.layout)),
            ])
        })
        .collect()
}

fn render_inline_json(bundle: &ProgramBundle, _facts: &SemIndexEffectFacts) -> Vec<ExpandValue> {
    let mut rows = Vec::new();
    for module in &bundle.modules {
        for (qualified, span, contract, attr) in collect_inline_facts(&module.items, None) {
            let location = Some(jet::Diagnostics::span_line_col(&module.source, span.start));
            let qualified_value = qualified.clone();
            rows.push((
                module.display.clone(),
                span.start,
                qualified,
                expand_object(vec![
                    ("name", expand_string(qualified_value)),
                    ("source", expand_string(&module.display)),
                    ("span", expand_span(span, location)),
                    ("contract", expand_string(contract)),
                    ("rust_attribute", expand_string(attr)),
                ]),
            ));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    rows.into_iter().map(|(_, _, _, row)| row).collect()
}

/// Walk one item list for `#Inline`/`#Inline(Always)` functions and methods,
/// recursing into struct/enum trait-impl blocks and inline code modules
/// (D-MOD2) so nothing declared inside one is missed.
type InlineRow = (String, jet::Diagnostics::Span, &'static str, &'static str);

/// One `#Inline`/`#Inline(Always)` fn or method: its qualified name, marker
/// span, contract label, and the Rust attribute codegen emits. `None` for
/// neither marker (the ballot: don't dump every function, only the ones
/// carrying a contract).
fn inline_row(f: &jet::AST::Func, owner: Option<&str>) -> Option<InlineRow> {
    let (contract, attr) = if f.is_inline_always {
        ("#Inline(Always)", "#[inline(always)]")
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
