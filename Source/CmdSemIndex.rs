//! D-SEMINDEX1: `jet inspect semindex` — smoke CLI for the stable semantic-index JSON API.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet::Diagnostics::ReportPath;
use jet::ExitCodes;
use jet_foundation::Report::render_status_json;
use jet_foundation::JSON::json_escape;
use jet_semindex::{
    open, EffectFact, SemIndexError, SemanticProvenance, SemanticSymbol, SemanticSymbolIndex,
    SemanticSymbolKind, SCHEMA_VERSION,
};

pub(crate) fn run_semindex(args: &[String], json: bool) {
    let path = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    let Some(path) = path else {
        crate::cli_error!(@fix "E2104", "`jet inspect semindex` needs an entry file", "jet inspect semindex examples/features/basics/hello.jet");
        exit(ExitCodes::USER_ERROR);
    };

    let abs = absolutize(path);
    match open(&abs) {
        Ok(idx) => {
            if json {
                println!("{}", idx.to_json());
            } else {
                println!("semantic index (schema v{})", SCHEMA_VERSION);
                println!("  definitions: {}", idx.definitions().len());
                println!("  references:  {}", idx.references().len());
                println!("  call edges:  {}", idx.call_edges().len());
                println!("  effects:     {}", idx.effects().len());
                println!("  arithmetic:  {}", idx.arithmetic().len());
                for fact in idx.arithmetic() {
                    println!(
                        "    fixed-width {}: .{}; scope = {}:{}..{}; operation = {}..{}",
                        fact.operation,
                        fact.policy,
                        fact.module_path,
                        fact.scope_span.start,
                        fact.scope_span.end,
                        fact.operation_span.start,
                        fact.operation_span.end,
                    );
                }
                println!("failure contracts:");
                for definition in idx.definitions() {
                    let Some(signature) = &definition.callable_signature else {
                        continue;
                    };
                    println!(
                        "  {}: {} ({})",
                        definition.qualified_name,
                        signature.failure_contract,
                        signature.failure_source
                    );
                }
                println!("nominal type contracts:");
                for definition in idx.definitions() {
                    let Some(base) = &definition.nominal_base else {
                        continue;
                    };
                    println!("  {}: distinct {}", definition.qualified_name, base);
                    for contract in &definition.trait_contracts {
                        println!("    implements {}", contract.trait_name);
                        for (name, ty) in &contract.associated_types {
                            match ty {
                                Some(ty) => println!("      type {} = {}", name, ty),
                                None => println!("      type {}", name),
                            }
                        }
                        for method in &contract.methods {
                            println!("      {}", method);
                        }
                    }
                }
                println!("note: pass --json for the full stable document");
            }
        }
        Err(SemIndexError::Load(diags)) => {
            if json {
                print!(
                    "{}",
                    jet::render_all_json(&ReportPath::from_path(&abs), "", &diags)
                );
            } else {
                for d in &diags {
                    eprintln!(
                        "{}",
                        jet::render_diagnostics(
                            &abs.display().to_string(),
                            "",
                            std::slice::from_ref(d)
                        )
                    );
                }
            }
            exit(ExitCodes::USER_ERROR);
        }
    }
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

#[derive(Default)]
struct FindOptions {
    effect: Option<String>,
    example: Option<String>,
    member: Option<String>,
    positionals: Vec<String>,
}

#[derive(Clone)]
struct TypeShape {
    inputs: Vec<String>,
    output: String,
}

enum FindMode {
    Signature(TypeShape),
    Effect {
        name: String,
        text: String,
    },
    Example {
        raw: String,
        shape: Option<TypeShape>,
    },
    Text(String),
}

struct FindMatch {
    name: String,
    signature: String,
    why: String,
}

/// `jet find` searches the checked semantic index. The source check stays the
/// same front-end path as `jet inspect`; this command only changes the query
/// and presentation layer.
pub(crate) fn run_find(args: &[String], json: bool) {
    let options = match parse_find_options(args) {
        Ok(options) => options,
        Err(message) => {
            crate::cli_error!(@fix "E2104", message, "run `jet find --help` for the query forms");
            exit(ExitCodes::USAGE);
        }
    };
    if options.effect.is_some() && options.example.is_some() {
        crate::cli_error!(@fix "E2104", "`--effect` and `--example` cannot be combined", "use one find mode per command");
        exit(ExitCodes::USAGE);
    }

    let FindOptions {
        effect,
        example,
        member,
        positionals,
    } = options;
    let mut positionals = positionals;
    let target_index = positionals
        .iter()
        .rposition(|value| crate::looks_like_jet_source(value));
    let target = target_index.map(|index| positionals.remove(index));
    let text = positionals.join(" ");
    // `text` moves into the FindMode below, but the renderer also needs it.
    let rendered_text = text.clone();
    if effect.is_none() && example.is_none() && text.is_empty() {
        crate::cli_error!(@fix "E2104", "`jet find` needs a query or an effect filter", "use `jet find \"String -> Path\"` or `jet find --effect IO`");
        exit(ExitCodes::USAGE);
    }

    let mode = match (effect, example) {
        (Some(name), None) => FindMode::Effect { name, text },
        (None, Some(raw)) => {
            if raw.contains("=>") {
                crate::cli_error!(@fix "E2104", "find examples use `->`", "write the input/output example with `->`");
                exit(ExitCodes::USAGE);
            }
            FindMode::Example {
                shape: example_shape(&raw),
                raw,
            }
        }
        (None, None) => {
            if text.contains("=>") {
                crate::cli_error!(@fix "E2104", "find signatures use `->`", "write the input/output shape with `->`");
                exit(ExitCodes::USAGE);
            }
            if text.contains("->") {
                let shape = parse_type_shape(&text).unwrap_or_else(|| {
                    crate::cli_error!(@fix "E2104", "the find signature needs input and output types", "use `jet find \"String -> Path\"`");
                    exit(ExitCodes::USAGE);
                });
                FindMode::Signature(shape)
            } else {
                FindMode::Text(text)
            }
        }
        (Some(_), Some(_)) => unreachable!(),
    };

    let entry = match target.as_deref() {
        Some(raw) if Path::new(raw).is_dir() => {
            crate::resolve_bare_entry("find", Path::new(raw), member.as_deref())
        }
        Some(raw) => Some(PathBuf::from(crate::resolve_source_path(raw))),
        None => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            crate::resolve_bare_entry("find", &cwd, member.as_deref())
        }
    };

    let (symbols, effects) = match entry {
        Some(path) => {
            let checked =
                crate::CmdInspect::check_projection_for_effects(&path, "dev", &BTreeMap::new())
                    .unwrap_or_else(|diagnostics| {
                        crate::CmdInspect::render_check_failure(&path, &diagnostics, json, false);
                    });
            let symbols = jet_semindex::build_symbol_db(&checked.bundle, &checked.facts).symbols;
            (symbols, checked.index.effects().to_vec())
        }
        None => (SemanticSymbolIndex::language(), Vec::new()),
    };
    let matches = find_matches(&symbols, &effects, &mode);
    render_find(&mode, &rendered_text, &matches, json);
}

fn parse_find_options(args: &[String]) -> Result<FindOptions, String> {
    let mut options = FindOptions::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--effect" || arg == "--example" {
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| format!("`{arg}` needs a value"))?;
            if arg == "--effect" {
                options.effect = Some(value.clone());
            } else {
                options.example = Some(value.clone());
            }
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--effect=") {
            if value.is_empty() {
                return Err("`--effect` needs a value".to_string());
            }
            options.effect = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--example=") {
            if value.is_empty() {
                return Err("`--example` needs a value".to_string());
            }
            options.example = Some(value.to_string());
        } else if arg == "-p" {
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .ok_or_else(|| "`-p` needs a workspace member".to_string())?;
            options.member = Some(value.clone());
            index += 2;
            continue;
        } else if arg == jet::CLI::MACHINE_OUTPUT_FLAG
            || arg == "--quiet"
            || arg == "--color"
            || arg.starts_with("--color=")
        {
            index += 1;
            continue;
        } else if !arg.starts_with('-') {
            options.positionals.push(arg.clone());
        }
        index += 1;
    }
    Ok(options)
}

fn find_matches(
    symbols: &SemanticSymbolIndex,
    effects: &[EffectFact],
    mode: &FindMode,
) -> Vec<FindMatch> {
    let mut matches = Vec::new();
    for symbol in symbols.symbols() {
        if !matches!(
            &symbol.kind,
            SemanticSymbolKind::Function | SemanticSymbolKind::Member
        ) {
            continue;
        }
        let candidate = match mode {
            FindMode::Signature(shape) => signature_shape(&symbol.signature)
                .filter(|candidate| shape_matches(shape, candidate))
                .map(|_| "signature matches the requested type shape".to_string()),
            FindMode::Effect { name, text } => {
                let Some(fact) = effect_fact(symbol, effects) else {
                    continue;
                };
                if !effect_matches(name, fact) || !text_matches(symbol, text) {
                    continue;
                }
                Some(effect_reason(name, fact))
            }
            FindMode::Example { raw, shape } => {
                if !is_pure_candidate(symbol, effects) {
                    continue;
                }
                let candidate_shape = signature_shape(&symbol.signature);
                let shape_match = shape
                    .as_ref()
                    .zip(candidate_shape.as_ref())
                    .is_some_and(|(wanted, candidate)| shape_matches(wanted, candidate));
                let documented = symbol
                    .examples
                    .iter()
                    .any(|example| normalize_text(example) == normalize_text(raw));
                if !shape_match && !documented {
                    continue;
                }
                Some(if shape_match {
                    "pure candidate has the same input/output shape".to_string()
                } else {
                    "pure candidate has a documented matching example".to_string()
                })
            }
            FindMode::Text(query) => text_matches(symbol, query)
                .then(|| "name or documentation matches the query".to_string()),
        };
        let Some(why) = candidate else { continue };
        matches.push(FindMatch {
            name: symbol.qualified_name.clone(),
            signature: symbol.signature.clone(),
            why,
        });
    }
    matches.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.signature.cmp(&right.signature))
    });
    matches
}

fn effect_fact<'a>(symbol: &SemanticSymbol, effects: &'a [EffectFact]) -> Option<&'a EffectFact> {
    if !matches!(&symbol.provenance, SemanticProvenance::Source { .. }) {
        return None;
    }
    let key = symbol
        .owner
        .as_ref()
        .map(|owner| format!("{owner}::{}", symbol.name))
        .unwrap_or_else(|| symbol.name.clone());
    effects
        .iter()
        .find(|fact| fact.function == key)
        .or_else(|| effects.iter().find(|fact| fact.function == symbol.name))
        .or_else(|| {
            let suffix = format!("::{key}");
            effects.iter().find(|fact| fact.function.ends_with(&suffix))
        })
        .or_else(|| {
            effects
                .iter()
                .find(|fact| fact.function.ends_with(&format!("__{}", symbol.name)))
        })
}

fn effect_matches(wanted: &str, fact: &EffectFact) -> bool {
    let wanted_root = wanted.split('.').next().unwrap_or(wanted);
    fact.maximal
        || fact.inferred.iter().any(|actual| {
            actual == wanted
                || actual.starts_with(&format!("{wanted}."))
                || wanted.starts_with(&format!("{actual}."))
                || actual.split('.').next().unwrap_or(actual) == wanted_root
        })
}

fn effect_reason(wanted: &str, fact: &EffectFact) -> String {
    if fact.maximal {
        return format!("holds {wanted} through an open effect set");
    }
    let actual = fact.inferred.join(", ");
    let path = fact
        .provenance
        .iter()
        .find(|proof| {
            proof.effect == wanted
                || proof.effect.starts_with(&format!("{wanted}."))
                || wanted.starts_with(&format!("{}.", proof.effect))
                || proof.effect.split('.').next().unwrap_or(&proof.effect)
                    == wanted.split('.').next().unwrap_or(wanted)
        })
        .map(|proof| proof.call_path.join(" -> "));
    match path {
        Some(path) => format!("holds {wanted} (inferred: {actual}; via {path})"),
        None => format!("holds {wanted} (inferred: {actual})"),
    }
}

fn is_pure_candidate(symbol: &SemanticSymbol, effects: &[EffectFact]) -> bool {
    effect_fact(symbol, effects).is_none_or(|fact| !fact.maximal && fact.inferred.is_empty())
}

fn text_matches(symbol: &SemanticSymbol, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        symbol.name,
        symbol.qualified_name,
        symbol.summary,
        symbol.examples.join(" "),
    )
    .to_ascii_lowercase();
    query
        .split_whitespace()
        .filter(|term| term.len() > 1)
        .all(|term| haystack.contains(&term.to_ascii_lowercase()))
}

fn parse_type_shape(query: &str) -> Option<TypeShape> {
    let (inputs, output) = query.split_once("->")?;
    let inputs = if inputs.trim().is_empty() || inputs.trim() == "()" {
        Vec::new()
    } else {
        split_top_level(inputs, ',')
            .into_iter()
            .map(|ty| compact_type(&ty))
            .filter(|ty| !ty.is_empty())
            .collect()
    };
    let output = compact_type(output);
    (!output.is_empty()).then_some(TypeShape { inputs, output })
}

fn signature_shape(signature: &str) -> Option<TypeShape> {
    let open = signature.find('(')?;
    let close = signature.rfind(')')?;
    if close < open {
        return None;
    }
    let inputs = split_top_level(&signature[open + 1..close], ',')
        .into_iter()
        .filter(|part| !matches!(part.trim(), "" | "/" | "*"))
        .map(|part| parameter_type(&part))
        .collect::<Option<Vec<_>>>()?;
    let mut tail = signature[close + 1..].trim();
    if tail.starts_with("-[") {
        tail = "";
    } else if let Some(rest) = tail.strip_prefix("->") {
        tail = rest.trim();
    }
    if let Some((return_type, _)) = tail.split_once(" -[") {
        tail = return_type.trim();
    }
    let output = tail.split_whitespace().next().unwrap_or("()");
    Some(TypeShape {
        inputs,
        output: compact_type(output),
    })
}

fn parameter_type(part: &str) -> Option<String> {
    let part = part.trim();
    let part = part.strip_prefix("...").unwrap_or(part);
    let part = part.trim_start_matches(['~', '&', '^']);
    let (_, ty) = part.split_once(':')?;
    let ty = ty.split_once(" {").map_or(ty, |(ty, _)| ty).trim();
    (!ty.is_empty()).then(|| compact_type(ty))
}

fn example_shape(example: &str) -> Option<TypeShape> {
    let (inputs, output) = example.split_once("->")?;
    let inputs = if inputs.trim().is_empty() || inputs.trim() == "()" {
        Vec::new()
    } else {
        split_top_level(inputs, ',')
            .into_iter()
            .map(|value| literal_type(&value))
            .collect::<Option<Vec<_>>>()?
    };
    Some(TypeShape {
        inputs,
        output: literal_type(output)?,
    })
}

fn literal_type(value: &str) -> Option<String> {
    let value = value.trim();
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        return Some("String".to_string());
    }
    if matches!(value, "true" | "false") {
        return Some("Bool".to_string());
    }
    if value.parse::<i64>().is_ok() {
        return Some("Int".to_string());
    }
    if value.parse::<f64>().is_ok() {
        return Some("Float".to_string());
    }
    if value.starts_with('[') && value.ends_with(']') {
        let inner = &value[1..value.len() - 1];
        if inner.trim().is_empty() {
            return Some("[T]".to_string());
        }
        let types = split_top_level(inner, ',')
            .into_iter()
            .map(|item| literal_type(&item))
            .collect::<Option<Vec<_>>>()?;
        let first = types.first()?.clone();
        return Some(if types.iter().all(|ty| ty == &first) {
            format!("[{first}]")
        } else {
            "[T]".to_string()
        });
    }
    value
        .split_once('{')
        .map(|(ty, _)| compact_type(ty))
        .filter(|ty| !ty.is_empty())
        .or_else(|| (value == "()" || value == "None").then(|| "T".to_string()))
        .or_else(|| (!value.is_empty()).then(|| "T".to_string()))
}

fn shape_matches(wanted: &TypeShape, candidate: &TypeShape) -> bool {
    if wanted.inputs.len() != candidate.inputs.len() {
        return false;
    }
    let mut bindings = BTreeMap::new();
    wanted
        .inputs
        .iter()
        .zip(&candidate.inputs)
        .all(|(wanted, candidate)| type_matches(wanted, candidate, &mut bindings))
        && type_matches(&wanted.output, &candidate.output, &mut bindings)
}

fn type_matches(wanted: &str, candidate: &str, bindings: &mut BTreeMap<String, String>) -> bool {
    let wanted = compact_type(wanted);
    let candidate = compact_type(candidate);
    if is_type_variable(&wanted) {
        return match bindings.get(&wanted) {
            Some(bound) => bound == &candidate,
            None => {
                bindings.insert(wanted, candidate);
                true
            }
        };
    }
    if wanted.starts_with('[')
        && wanted.ends_with(']')
        && candidate.starts_with('[')
        && candidate.ends_with(']')
    {
        return type_matches(
            &wanted[1..wanted.len() - 1],
            &candidate[1..candidate.len() - 1],
            bindings,
        );
    }
    if wanted.starts_with('(')
        && wanted.ends_with(')')
        && candidate.starts_with('(')
        && candidate.ends_with(')')
    {
        let wanted_parts = split_top_level(&wanted[1..wanted.len() - 1], ',');
        let candidate_parts = split_top_level(&candidate[1..candidate.len() - 1], ',');
        return wanted_parts.len() == candidate_parts.len()
            && wanted_parts
                .iter()
                .zip(candidate_parts)
                .all(|(wanted, candidate)| type_matches(wanted, &candidate, bindings));
    }
    if let (Some((wanted_head, wanted_args)), Some((candidate_head, candidate_args))) =
        (generic_parts(&wanted), generic_parts(&candidate))
    {
        return type_names_equal(wanted_head, candidate_head)
            && wanted_args.len() == candidate_args.len()
            && wanted_args
                .iter()
                .zip(candidate_args)
                .all(|(wanted, candidate)| type_matches(wanted, &candidate, bindings));
    }
    type_names_equal(&wanted, &candidate)
}

fn generic_parts(ty: &str) -> Option<(&str, Vec<String>)> {
    let open = ty.find('<')?;
    ty.ends_with('>').then(|| {
        (
            &ty[..open],
            split_top_level(&ty[open + 1..ty.len() - 1], ',')
                .into_iter()
                .map(|part| compact_type(&part))
                .collect(),
        )
    })
}

fn is_type_variable(ty: &str) -> bool {
    ty.len() == 1 && ty.chars().next().is_some_and(|ch| ch.is_ascii_uppercase())
}

fn type_names_equal(left: &str, right: &str) -> bool {
    left == right || left.rsplit(['.', ':']).next() == right.rsplit(['.', ':']).next()
}

fn compact_type(ty: &str) -> String {
    ty.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn split_top_level(input: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut round = 0;
    let mut square = 0;
    let mut angle = 0;
    for (index, ch) in input.char_indices() {
        match ch {
            '(' => round += 1,
            ')' => round -= 1,
            '[' => square += 1,
            ']' => square -= 1,
            '<' => angle += 1,
            '>' => angle -= 1,
            _ => {}
        }
        if ch == separator && round == 0 && square == 0 && angle == 0 {
            parts.push(input[start..index].trim().to_string());
            start = index + ch.len_utf8();
        }
    }
    parts.push(input[start..].trim().to_string());
    parts
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_find(mode: &FindMode, query: &str, matches: &[FindMatch], json: bool) {
    if json {
        let rows = matches
            .iter()
            .map(|item| {
                format!(
                    "{{\"name\":\"{}\",\"signature\":\"{}\",\"why\":\"{}\"}}",
                    json_escape(&item.name),
                    json_escape(&item.signature),
                    json_escape(&item.why),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{}",
            render_status_json(
                "ok",
                true,
                "find",
                &format!(
                    ",\"query\":\"{}\",\"matches\":[{}]",
                    json_escape(query),
                    rows
                ),
            )
        );
        return;
    }
    let heading = match mode {
        FindMode::Signature(_) => format!("signature {query}"),
        FindMode::Effect { name, text } if text.is_empty() => format!("effect {name}"),
        FindMode::Effect { name, text } => format!("effect {name} / {text}"),
        FindMode::Example { raw, .. } => format!("example {raw}"),
        FindMode::Text(text) => format!("text {text}"),
    };
    println!("find: {heading}");
    if matches.is_empty() {
        println!("  no matches");
        return;
    }
    for (index, item) in matches.iter().enumerate() {
        println!("  {}. {}", index + 1, item.signature);
        println!("     why: {}", item.why);
    }
}
