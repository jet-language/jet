//! LSP language features: hover, go-to-definition, references, rename,
//! refactors, semantic tokens, inlay hints.

use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Lexer::{TokKind, Token};
use crate::Syntax;
use std::collections::BTreeMap;

use super::Completion::{
    context_is_member_access, context_is_option_field, use_statement_for_module, JET_KEYWORDS,
    JET_TYPES,
};
use super::Position::byte_offset_to_lsp;
use super::SymbolDB::{InlayHint, SymKind, SymbolDB};
use jet_foundation::JSON::json_escape;
use jet_semindex::SymRef;
use jetpack::Discovery::{Index as DiscoveryIndex, OptionField, PackageRecord};

// ── Hover ─────────────────────────────────────────────────────────────────────

fn semantic_hover(symbol: &jet_semindex::SemanticSymbol, requested_path: &str) -> String {
    let mut out = String::new();
    if symbol.module_path != requested_path
        && matches!(&symbol.kind, jet_semindex::SemanticSymbolKind::Function)
    {
        out.push_str("from module `");
        out.push_str(&symbol.module_path);
        out.push_str("`\n\n");
    }
    if matches!(
        &symbol.kind,
        jet_semindex::SemanticSymbolKind::Type | jet_semindex::SemanticSymbolKind::Member
    ) && symbol.qualified_name != symbol.name
    {
        out.push('`');
        out.push_str(&symbol.qualified_name);
        out.push_str("`\n\n");
    }
    if !symbol.summary.is_empty() {
        out.push_str(&symbol.summary);
        out.push_str("\n\n---\n\n");
    }
    out.push_str(&symbol.signature);
    for example in &symbol.examples {
        out.push_str("\n\nExample: `");
        out.push_str(example);
        out.push('`');
    }
    out
}

fn append_checked_signature(hover: &mut String, checked_signature: &str) {
    // The consumer-neutral semantic symbol already carries the effective
    // failure fact for references and completion. Keep the richer checked
    // declaration detail for direct hovers, but do not print its identical
    // failure line twice.
    let checked_signature = checked_signature
        .lines()
        .filter(|line| !line.starts_with("failure: "))
        .collect::<Vec<_>>()
        .join("\n");
    if checked_signature.is_empty() {
        return;
    }
    hover.push_str("\n\n");
    hover.push_str(&checked_signature);
}

fn state_graph_hover_at(db: &SymbolDB, module_path: &str, span: Span) -> Option<String> {
    db.hover
        .iter()
        .find(|hover| {
            hover.module_path == module_path
                && hover.span == span
                && hover.text.starts_with("state `")
                && hover.text.contains(".State.")
        })
        .map(|hover| hover.text.clone())
}

pub(crate) fn compute_hover(
    db: &SymbolDB,
    tokens: &[Token],
    _src: &str,
    path: &str,
    offset: usize,
) -> Option<String> {
    let arithmetic_hover = db.arithmetic_hover_at(path, offset).map(str::to_string);
    if let Some(fact) = compiler_fact_at(tokens, offset) {
        return Some(match fact {
            Syntax::COMPILER_FACT_LAYOUT => format!(
                "Compiler fact {}: focused layout metadata with typed optional physical facts.",
                Syntax::COMPILER_FACT_LAYOUT
            ),
            Syntax::COMPILER_FACT_ORIGIN => {
                let type_name = Syntax::fact_read_kind(fact)
                    .and_then(|read| read.public_read_type())
                    .expect("origin fact has a public read type");
                format!(
                    "Compiler fact {}: optional {} provenance derived from sema flow.",
                    Syntax::COMPILER_FACT_ORIGIN,
                    type_name.trim_start_matches('?')
                )
            }
            _ => {
                let kind = Syntax::fact_read_kind(fact)
                    .and_then(|read| read.reflection_kind())
                    .unwrap_or("typed");
                format!("Compiler fact {fact}: typed {kind} metadata.")
            }
        });
    }
    if let Some(hover) = db
        .hover
        .iter()
        .find(|hover| {
            hover.module_path == path
                && hover.span.start <= offset
                && offset <= hover.span.end
                && hover.text.starts_with("state `")
                && hover.text.contains(".State.")
        })
        .map(|hover| hover.text.clone())
    {
        return Some(hover);
    }
    if let Some(symbol) = db.symbols.at(path, offset) {
        let mut hover = semantic_hover(symbol, path);
        if matches!(symbol.kind, jet_semindex::SemanticSymbolKind::Function) {
            if let Some(span) = symbol.span {
                if let Some(checked_signature) = db.hover_at(path, span.start) {
                    append_checked_signature(&mut hover, checked_signature);
                }
            }
        }
        if let Some(record) = derivation_for_symbol(db, symbol) {
            hover.push_str("\n\n---\n\n");
            hover.push_str(&reasoning_summary(record));
        }
        if let Some(arithmetic) = arithmetic_hover.as_deref() {
            hover.push_str("\n\n");
            hover.push_str(arithmetic);
        }
        return Some(hover);
    }
    if let Some(reference) = db.refs.iter().find(|reference| {
        reference.module_path == path
            && reference.span.start <= offset
            && offset <= reference.span.end
    }) {
        if let Some(target) = &reference.target {
            if target.kind == "state" {
                if let Some(hover) =
                    state_graph_hover_at(db, &target.module_path, target.def_span.into())
                {
                    return Some(hover);
                }
            }
            if let Some(symbol) = db.symbols.symbols().iter().find(|symbol| {
                symbol.module_path == target.module_path && symbol.span == Some(target.def_span)
            }) {
                let mut hover = semantic_hover(symbol, path);
                if let Some(record) = derivation_for_symbol(db, symbol) {
                    hover.push_str("\n\n---\n\n");
                    hover.push_str(&reasoning_summary(record));
                }
                if let Some(arithmetic) = arithmetic_hover.as_deref() {
                    hover.push_str("\n\n");
                    hover.push_str(arithmetic);
                }
                return Some(hover);
            }
        }
    }
    if let Some(name) = find_ident_at(tokens, offset) {
        if let Some(symbol) = db.symbols.resolve_visible_in(name, Some(path)) {
            let mut hover = semantic_hover(symbol, path);
            if let Some(record) = derivation_for_symbol(db, symbol) {
                hover.push_str("\n\n---\n\n");
                hover.push_str(&reasoning_summary(record));
            }
            if let Some(arithmetic) = arithmetic_hover.as_deref() {
                hover.push_str("\n\n");
                hover.push_str(arithmetic);
            }
            return Some(hover);
        }
    }
    arithmetic_hover.or_else(|| db.hover_at(path, offset).map(str::to_string))
}
/// Return the checked derivation attached to a semantic symbol.  The symbol
/// identity is only an anchor; the derivation table remains the sole source of
/// the relation payload.
fn derivation_for_symbol<'a>(
    db: &'a SymbolDB,
    symbol: &jet_semindex::SemanticSymbol,
) -> Option<&'a jet_foundation::Facts::DerivationRecord> {
    let definition = db.index.lookup_identity(&symbol.identity).or_else(|| {
        let span = symbol.span?;
        db.index.definitions().iter().find(|definition| {
            definition.module_path == symbol.module_path && definition.def_span == span
        })
    })?;
    let stable_id = db
        .index
        .definition_facts()
        .iter()
        .find(|fact| fact.human_identity == definition.identity)
        .map(|fact| fact.stable_id.as_str());
    stable_id
        .and_then(|subject| db.index.derivations().iter().find(|row| row.subject == subject))
        .or_else(|| {
            db.index
                .derivations()
                .iter()
                .find(|row| row.subject == definition.identity || row.subject == symbol.identity)
        })
}

fn reasoning_summary(record: &jet_foundation::Facts::DerivationRecord) -> String {
    let identity = &record.identity;
    format!(
        "Reasoning\n\nclaim: `{}`\nrecord: `{}`\nproducer: `{}`\nmethod: `{}`\ndisposition: `{}`\npremises: {}\nsource/build/run/target: `{}` / `{}` / `{}` / `{}`",
        record.claim,
        record.id,
        record.producer,
        record.method.as_str(),
        record.disposition.as_str(),
        record.premises.len(),
        identity.source,
        identity.build,
        identity.run,
        identity.target,
    )
}


/// Add bounded inlay hints for derivations whose source anchor belongs to this
/// module.  The label is a cue to open the complete relationship view; it is
/// not a replacement for the canonical record.
pub(crate) fn reasoning_inlay_hints(db: &SymbolDB, path: &str) -> Vec<InlayHint> {
    const MAX_HINTS: usize = 64;
    db.index
        .derivations()
        .iter()
        .filter_map(|record| {
            let fact = db
                .index
                .definition_facts()
                .iter()
                .find(|fact| fact.stable_id == record.subject)?;
            (fact.module_path == path).then(|| InlayHint {
                span: fact.span.into(),
                module_path: fact.module_path.clone(),
                label: format!(
                    "reason: {} · {}",
                    record.claim,
                    record.disposition.as_str()
                ),
            })
        })
        .take(MAX_HINTS)
        .collect()
}

fn reasoning_span_json(
    db: &SymbolDB,
    record: &jet_foundation::Facts::DerivationRecord,
) -> String {
    db.index
        .definition_facts()
        .iter()
        .find(|fact| fact.stable_id == record.subject)
        .map(|fact| format!("{{\"start\":{},\"end\":{}}}", fact.span.start, fact.span.end))
        .unwrap_or_else(|| "null".to_string())
}

fn bounded_reasoning_text(value: &str, limit: usize) -> (String, bool) {
    let mut text = value.chars().take(limit).collect::<String>();
    let truncated = text.chars().count() < value.chars().count();
    if truncated {
        text.push('…');
    }
    (text, truncated)
}

fn reasoning_record_json(
    db: &SymbolDB,
    record: &jet_foundation::Facts::DerivationRecord,
    expanded: bool,
) -> String {
    const MAX_ITEMS: usize = 128;
    const MAX_RAW_BYTES: usize = 16 * 1024;
    let (claim, claim_truncated) = bounded_reasoning_text(&record.claim, 1024);
    let (producer, producer_truncated) = bounded_reasoning_text(&record.producer, 512);
    let (rule, rule_truncated) = bounded_reasoning_text(&record.rule, 1024);
    let premises = record
        .premises
        .iter()
        .take(MAX_ITEMS)
        .map(|value| {
            let (value, _) = bounded_reasoning_text(value, 1024);
            format!("\"{}\"", json_escape(&value))
        })
        .collect::<Vec<_>>()
        .join(",");
    let assumptions = record
        .assumptions
        .iter()
        .take(MAX_ITEMS)
        .map(|value| {
            let (value, _) = bounded_reasoning_text(value, 1024);
            format!("\"{}\"", json_escape(&value))
        })
        .collect::<Vec<_>>()
        .join(",");
    let observation = record.observation.as_ref().map_or_else(
        || "null".to_string(),
        |observation| {
            format!(
                "{{\"event\":{},\"counterexample\":{}}}",
                observation
                    .event_id
                    .as_deref()
                    .map(|value| format!("\"{}\"", json_escape(value)))
                    .unwrap_or_else(|| "null".to_string()),
                observation
                    .counterexample_id
                    .as_deref()
                    .map(|value| format!("\"{}\"", json_escape(value)))
                    .unwrap_or_else(|| "null".to_string()),
            )
        },
    );
    let payload = if expanded {
        record
            .payload
            .as_ref()
            .map(|payload| payload.to_json())
            .filter(|payload| payload.len() <= MAX_RAW_BYTES)
            .unwrap_or_else(|| "null".to_string())
    } else {
        "null".to_string()
    };
    let raw_premises = if expanded {
        format!(",\"raw_premises\":[{}]", premises)
    } else {
        String::new()
    };
    format!(
        "{{\"id\":\"{}\",\"subject\":\"{}\",\"claim\":\"{}\",\"producer\":\"{}\",\"method\":\"{}\",\"rule\":\"{}\",\"disposition\":\"{}\",\"identity\":{{\"source\":\"{}\",\"build\":\"{}\",\"run\":\"{}\",\"target\":\"{}\"}},\"source_span\":{},\"premises\":[{}],\"assumptions\":[{}],\"observation\":{},\"payload\":{},\"limits\":{{\"premises_truncated\":{},\"assumptions_truncated\":{},\"claim_truncated\":{},\"producer_truncated\":{},\"rule_truncated\":{},\"payload_truncated\":{}}}{}}}",
        json_escape(&record.id),
        json_escape(&record.subject),
        json_escape(&claim),
        json_escape(&producer),
        record.method.as_str(),
        json_escape(&rule),
        record.disposition.as_str(),
        json_escape(&record.identity.source),
        json_escape(&record.identity.build),
        json_escape(&record.identity.run),
        json_escape(&record.identity.target),
        reasoning_span_json(db, record),
        premises,
        assumptions,
        observation,
        payload,
        record.premises.len() > MAX_ITEMS,
        record.assumptions.len() > MAX_ITEMS,
        claim_truncated,
        producer_truncated,
        rule_truncated,
        expanded
            && record.payload.is_some()
            && record
                .payload
                .as_ref()
                .is_some_and(|payload| payload.to_json().len() > MAX_RAW_BYTES),
        raw_premises,
    )
}

fn reasoning_family(record: &jet_foundation::Facts::DerivationRecord) -> &'static str {
    let text = format!(
        "{} {} {} {}",
        record.subject, record.claim, record.rule, record.producer
    )
    .to_ascii_lowercase();
    let families: [(&str, &[&str]); 8] = [
        ("value", &["value", "observed"][..]),
        ("ownership", &["owner", "ownership", "lifetime", "borrow"][..]),
        ("state", &["state", "transition"][..]),
        ("effects", &["effect", "event", "callback", "task"][..]),
        ("dependencies", &["depend", "premise", "input"][..]),
        ("impact", &["impact", "changed", "change"][..]),
        ("optimization", &["optim", "copy", "cost", "vector"][..]),
        ("counterexamples", &["counterexample", "witness"][..]),
    ];
    families
        .iter()
        .find_map(|(family, needles)| {
            needles
                .iter()
                .any(|needle| text.contains(needle))
                .then_some(*family)
        })
        .unwrap_or("relationships")
}

/// Complete source-linked relationship data for the expandable/pinnable
/// editor command.  It is a projection over `SemIndex::derivations()` and
/// never evaluates source or reconstructs a reason.
pub(crate) fn reasoning_view_json(
    db: &SymbolDB,
    path: &str,
    selection: Option<&str>,
    expanded: bool,
) -> String {
    const MAX_RECORDS: usize = 64;
    let selected = selection.filter(|value| !value.is_empty() && *value != "all");
    let records = db
        .index
        .derivations()
        .iter()
        .filter(|record| {
            selected.is_none_or(|selection| {
                record.id == selection
                    || record.subject == selection
                    || record.claim == selection
            })
        })
        .collect::<Vec<_>>();
    let rendered = records
        .iter()
        .take(MAX_RECORDS)
        .map(|record| reasoning_record_json(db, record, expanded))
        .collect::<Vec<_>>()
        .join(",");
    let mut lens_rows = BTreeMap::<&str, Vec<String>>::new();
    for record in records.iter().take(MAX_RECORDS) {
        lens_rows
            .entry(reasoning_family(record))
            .or_default()
            .push(format!("\"{}\"", json_escape(&record.id)));
    }
    let lenses = [
        "value",
        "ownership",
        "state",
        "effects",
        "dependencies",
        "impact",
        "optimization",
        "counterexamples",
        "relationships",
    ]
    .iter()
    .map(|name| {
        let rows = lens_rows
            .get(name)
            .map(|rows| rows.join(","))
            .unwrap_or_default();
        format!("{{\"name\":\"{}\",\"records\":[{}]}}", name, rows)
    })
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{{\"kind\":\"jet.reasoning/v1\",\"source\":\"{}\",\"selection\":{},\"records\":[{}],\"lenses\":[{}],\"limits\":{{\"records\":{},\"records_truncated\":{},\"expanded\":{}}}}}",
        json_escape(path),
        selected
            .map(|value| format!("\"{}\"", json_escape(value)))
            .unwrap_or_else(|| "\"all\"".to_string()),
        rendered,
        lenses,
        MAX_RECORDS,
        records.len() > MAX_RECORDS,
        expanded,
    )
}


/// Hover over package metadata and typed environment option fields from the
/// same local, offline discovery index used by completion.
pub(crate) fn compute_discovery_hover(
    src: &str,
    offset: usize,
    discovery: &DiscoveryIndex,
) -> Option<String> {
    let name = identifier_at(src, offset)?;
    if let Some(source) = context_is_member_access(src, offset) {
        if let Some(record) = discovery.info(&format!("{source}.{name}")) {
            return Some(package_hover(record));
        }
    }
    // `context_is_option_field` returns the identifier prefix so completion can
    // filter on it. Hover only needs to know it is inside an option block.
    if context_is_option_field(src, offset).is_some() {
        if let Some(field) = discovery
            .packages
            .iter()
            .flat_map(|record| record.options.iter())
            .find(|field| field.name == name)
        {
            return Some(option_hover(field));
        }
    }
    None
}

fn identifier_at(src: &str, offset: usize) -> Option<&str> {
    let bytes = src.as_bytes();
    let mut cursor = offset.min(bytes.len());
    if cursor == bytes.len() || !is_identifier_byte(bytes[cursor]) {
        if cursor == 0 || !is_identifier_byte(bytes[cursor - 1]) {
            return None;
        }
        cursor -= 1;
    }
    while cursor > 0 && is_identifier_byte(bytes[cursor - 1]) {
        cursor -= 1;
    }
    let mut end = offset.min(bytes.len());
    while end < bytes.len() && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    (cursor < end).then(|| &src[cursor..end])
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn package_hover(record: &PackageRecord) -> String {
    let mut out = format!("`{}`\n\n", record.display_ref());
    out.push_str(&format!("ref: `{}`\n", record.reference));
    out.push_str(&format!(
        "version: `{}`\n",
        if record.version.is_empty() {
            "-"
        } else {
            &record.version
        }
    ));
    out.push_str(&format!("platforms: {}\n", record.platforms.join(", ")));
    out.push_str(&format!("tier: {}\n", record.tier));
    out.push_str(&format!(
        "maintainer liveness: {}\n",
        record.maintainer_liveness()
    ));
    out.push_str(&format!("source: local discovery index\n\n{}", record.docs));
    out
}

fn option_hover(field: &OptionField) -> String {
    format!(
        "Environment option `{}`\n\nDefault: `{}`\n\n{}\n\nSource: local discovery index",
        field.name, field.default, field.docs
    )
}

fn find_ident_at<'a>(tokens: &'a [Token], offset: usize) -> Option<&'a str> {
    for tok in tokens {
        if tok.span.start <= offset && offset <= tok.span.end {
            if let TokKind::Ident(name) = &tok.kind {
                return Some(name.as_str());
            }
        }
    }
    None
}

fn compiler_fact_at(tokens: &[Token], offset: usize) -> Option<&str> {
    tokens.iter().find_map(|token| {
        let TokKind::Ident(name) = &token.kind else {
            return None;
        };
        (Syntax::fact_read_kind(name).is_some()
            && token.span.start <= offset
            && offset <= token.span.end)
            .then_some(name.as_str())
    })
}

/// Resolve the semantic symbol at one editor position. Navigation and hover
/// must use the same target identity; spelling lookup alone can pick a shadow.
pub(crate) fn semantic_symbol_at<'a>(
    db: &'a SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
) -> Option<&'a jet_semindex::SemanticSymbol> {
    if let Some(symbol) = db.symbols.at(path, offset) {
        return Some(symbol);
    }
    if let Some(reference) = db.refs.iter().find(|reference| {
        reference.module_path == path
            && reference.span.start <= offset
            && offset <= reference.span.end
    }) {
        if let Some(target) = &reference.target {
            if let Some(identity) = &target.semantic_identity {
                if let Some(symbol) = db.symbols.lookup_identity(identity) {
                    return Some(symbol);
                }
            }
            if let Some(symbol) = db.symbols.symbols().iter().find(|symbol| {
                symbol.module_path == target.module_path
                    && symbol.span == Some(target.def_span.into())
            }) {
                return Some(symbol);
            }
        }
    }
    find_ident_at(tokens, offset).and_then(|name| db.symbols.resolve_visible_in(name, Some(path)))
}

pub(crate) fn semantic_symbol_at_span<'a>(
    db: &'a SymbolDB,
    path: &str,
    span: Span,
) -> Option<&'a jet_semindex::SemanticSymbol> {
    db.symbols
        .symbols()
        .iter()
        .find(|symbol| symbol.module_path == path && symbol.span == Some(span.into()))
}

/// Stable LSP extension for nominal definitions. Standard navigation results
/// carry ranges only, so this preserves the checked type and trait contract
/// beside the range without making editors re-check or infer it.
pub(crate) fn semantic_symbol_metadata_json(
    db: &SymbolDB,
    symbol: &jet_semindex::SemanticSymbol,
) -> Option<String> {
    let definition = db.index.lookup_identity(&symbol.identity).or_else(|| {
        let span = symbol.span?;
        db.index.definitions().iter().find(|definition| {
            definition.module_path == symbol.module_path && definition.def_span == span
        })
    })?;
    let derivation = db
        .index
        .definition_facts()
        .iter()
        .find(|fact| fact.human_identity == definition.identity)
        .and_then(|fact| {
            db.index
                .derivations()
                .iter()
                .find(|row| row.subject == fact.stable_id)
        });
    if definition.nominal_base.is_none()
        && definition.trait_contracts.is_empty()
        && derivation.is_none()
    {
        return None;
    }
    let associated_types = |contract: &jet_semindex::TraitContractFact| {
        contract
            .associated_types
            .iter()
            .map(|(name, ty)| {
                format!(
                    "{{\"name\":\"{}\",\"type\":{}}}",
                    json_escape(name),
                    ty.as_deref()
                        .map(|ty| format!("\"{}\"", json_escape(ty)))
                        .unwrap_or_else(|| "null".to_string())
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let contracts = definition
        .trait_contracts
        .iter()
        .map(|contract| {
            let methods = contract
                .methods
                .iter()
                .map(|method| format!("\"{}\"", json_escape(method)))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"name\":\"{}\",\"associated_types\":[{}],\"methods\":[{}]}}",
                json_escape(&contract.trait_name),
                associated_types(contract),
                methods,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let nominal_base = definition
        .nominal_base
        .as_deref()
        .map(|base| format!("\"{}\"", json_escape(base)))
        .unwrap_or_else(|| "null".to_string());
    let derivation_json = derivation
        .map(jet_foundation::Facts::DerivationRecord::to_json)
        .unwrap_or_else(|| "null".to_string());
    Some(format!(
        "{{\"identity\":\"{}\",\"qualified_name\":\"{}\",\"signature\":\"{}\",\"nominal_base\":{},\"trait_contracts\":[{}],\"derivation\":{}}}",
        json_escape(&definition.identity),
        json_escape(&definition.qualified_name),
        json_escape(&symbol.signature),
        nominal_base,
        contracts,
        derivation_json,
    ))
}


fn compiler_fact_receiver_span(tokens: &[Token], offset: usize) -> Option<Span> {
    let fact_index = tokens.iter().position(|token| {
        matches!(&token.kind, TokKind::Ident(name) if Syntax::fact_read_kind(name).is_some())
            && token.span.start <= offset
            && offset <= token.span.end
    })?;
    let receiver = tokens.get(fact_index.checked_sub(2)?)?;
    matches!(&tokens.get(fact_index.checked_sub(1)?)?.kind, TokKind::Dot)
        .then_some(receiver)
        .and_then(|token| matches!(&token.kind, TokKind::Ident(_)).then_some(token.span))
}
fn checked_anchor_identity(
    db: &SymbolDB,
    anchor: &jet_semindex::DefinitionAnchor,
) -> Option<String> {
    if let Some(identity) = anchor
        .semantic_identity
        .as_deref()
        .filter(|identity| !identity.is_empty())
    {
        return Some(identity.to_string());
    }
    let mut identity = None;
    for definition in db.defs.iter().filter(|definition| {
        definition.module_path == anchor.module_path
            && definition.def_span == anchor.def_span.into()
    }) {
        if identity
            .as_ref()
            .is_some_and(|existing: &String| existing != &definition.identity)
        {
            return None;
        }
        identity = Some(definition.identity.clone());
    }
    identity
}

fn checked_reference_in_span<'a>(
    db: &'a SymbolDB,
    path: &str,
    start: usize,
    end: usize,
) -> Result<Option<&'a jet_semindex::SymRef>, ()> {
    let mut candidates = db
        .refs
        .iter()
        .filter(|reference| {
            reference.module_path == path
                && reference.span.start <= start
                && end <= reference.span.end
        })
        .collect::<Vec<_>>();
    let Some(min_len) = candidates
        .iter()
        .map(|reference| reference.span.end.saturating_sub(reference.span.start))
        .min()
    else {
        return Ok(None);
    };
    candidates.retain(|reference| {
        reference.span.end.saturating_sub(reference.span.start) == min_len
    });
    let mut selected = None;
    let mut identity = None;
    for reference in candidates {
        let Some(target) = reference.target.as_ref() else {
            continue;
        };
        let Some(candidate) = checked_anchor_identity(db, target) else {
            continue;
        };
        if identity
            .as_ref()
            .is_some_and(|existing: &String| existing != &candidate)
        {
            return Err(());
        }
        identity = Some(candidate);
        selected.get_or_insert(reference);
    }
    if selected.is_none() {
        return Err(());
    }
    Ok(selected)
}

fn checked_definition_in_span<'a>(
    db: &'a SymbolDB,
    path: &str,
    start: usize,
    end: usize,
) -> Result<Option<&'a jet_semindex::SymDef>, ()> {
    let mut candidates = db
        .defs
        .iter()
        .filter(|definition| {
            definition.module_path == path
                && definition.def_span.start <= start
                && end <= definition.def_span.end
        })
        .collect::<Vec<_>>();
    let Some(min_len) = candidates
        .iter()
        .map(|definition| definition.def_span.end.saturating_sub(definition.def_span.start))
        .min()
    else {
        return Ok(None);
    };
    candidates.retain(|definition| {
        definition.def_span.end.saturating_sub(definition.def_span.start) == min_len
    });
    let mut selected = None;
    let mut identity = None;
    for definition in candidates {
        if identity
            .as_ref()
            .is_some_and(|existing: &String| existing != &definition.identity)
        {
            return Err(());
        }
        identity = Some(definition.identity.clone());
        selected.get_or_insert(definition);
    }
    Ok(selected)
}

fn checked_instance_identity(
    db: &SymbolDB,
    path: &str,
    start: usize,
    end: usize,
) -> Result<Option<String>, ()> {
    let mut candidates = db
        .index
        .instances()
        .iter()
        .flat_map(|instance| &instance.applications)
        .filter(|application| {
            application.module_path == path
                && application.span.start <= start
                && end <= application.span.end
        })
        .collect::<Vec<_>>();
    let Some(min_len) = candidates
        .iter()
        .map(|application| application.span.end.saturating_sub(application.span.start))
        .min()
    else {
        return Ok(None);
    };
    candidates.retain(|application| {
        application.span.end.saturating_sub(application.span.start) == min_len
    });
    let mut identity = None;
    for application in candidates {
        if identity
            .as_ref()
            .is_some_and(|existing: &String| existing != &application.semantic_identity)
        {
            return Err(());
        }
        identity = Some(application.semantic_identity.clone());
    }
    Ok(identity)
}

fn checked_identity_in_span(
    db: &SymbolDB,
    path: &str,
    start: usize,
    end: usize,
) -> Result<Option<String>, ()> {
    if let Some(reference) = checked_reference_in_span(db, path, start, end)? {
        return Ok(reference
            .target
            .as_ref()
            .and_then(|target| checked_anchor_identity(db, target)));
    }
    if let Some(definition) = checked_definition_in_span(db, path, start, end)? {
        return Ok(Some(definition.identity.clone()));
    }
    checked_instance_identity(db, path, start, end)
}

/// Resolve only the checked target under the cursor. Unlike completion and
/// hover, semantic refactors never recover from an unresolved target by name.
pub(crate) fn checked_semantic_identity_at(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
) -> Option<String> {
    if let Ok(Some(identity)) = checked_identity_in_span(db, path, offset, offset) {
        return Some(identity);
    }
    let receiver = compiler_fact_receiver_span(tokens, offset)?;
    checked_identity_in_span(db, path, receiver.start, receiver.end)
        .ok()
        .flatten()
}

pub(crate) fn checked_rename_span_at(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
) -> Option<(String, Span)> {
    let identity = checked_semantic_identity_at(db, tokens, path, offset)?;
    let span = match checked_reference_in_span(db, path, offset, offset).ok()? {
        Some(reference) if reference.target.as_ref().is_some_and(|target| target.kind == "state") => {
            state_leaf_span(reference)?
        }
        _ => token_span_at(tokens, offset)?,
    };
    Some((identity, span))
}

fn token_span_at(tokens: &[Token], offset: usize) -> Option<Span> {
    tokens.iter().find_map(|token| {
        (token.span.start <= offset && offset <= token.span.end).then_some(token.span)
    })
}


/// A checked declaration projected from the canonical BuildPlan source.
///
/// The BuildPlan owns the generated artifact, module identity, and source. The
/// LSP keeps only this fact while resolving a request; declaration and origin
/// responses are projections of it rather than a second generated-symbol DB.
#[derive(Debug, Clone)]
pub(crate) struct GeneratedDeclarationFact {
    pub(crate) identity: String,
    pub(crate) name: String,
    pub(crate) module_name: String,
    pub(crate) module_path: String,
    pub(crate) source: String,
    pub(crate) span: Span,
    pub(crate) plugin: Option<String>,
}

fn generated_declarations(
    plan: &crate::Comptime::Build::BuildPlan,
) -> Vec<GeneratedDeclarationFact> {
    let Ok(modules) = plan.selected_generated_modules() else {
        return Vec::new();
    };
    let mut declarations = Vec::new();
    for module in modules {
        let (tokens, errors) = crate::Lexer::lex_generated(&module.source);
        if !errors.is_empty() {
            continue;
        }
        let tokens = crate::Lexer::without_comments(&tokens);
        for pair in tokens.windows(2) {
            let TokKind::Ident(name) = &pair[1].kind else {
                continue;
            };
            if !matches!(pair[0].kind, TokKind::KwFn) {
                continue;
            }
            declarations.push(GeneratedDeclarationFact {
                identity: format!("fn:module:{}::{name}", module.name),
                name: name.clone(),
                module_name: module.name.clone(),
                module_path: module.path.as_str().to_string(),
                source: module.source.clone(),
                span: pair[1].span,
                plugin: module.plugin.and_then(|handle| {
                    plan.plugins()
                        .iter()
                        .find(|plugin| plugin.id == handle.id())
                        .map(|plugin| plugin.name.clone())
                }),
            });
        }
    }
    declarations.sort_by(|left, right| {
        left.identity
            .cmp(&right.identity)
            .then(left.module_path.cmp(&right.module_path))
            .then(left.span.start.cmp(&right.span.start))
    });
    declarations
}

fn generated_qualifier<'a>(tokens: &'a [Token], index: usize) -> Option<&'a str> {
    let previous = tokens.get(index.checked_sub(1)?)?;
    if !matches!(previous.kind, TokKind::Dot | TokKind::QuestionDot) {
        return None;
    }
    match &tokens.get(index.checked_sub(2)?)?.kind {
        TokKind::Ident(name) => Some(name.as_str()),
        _ => None,
    }
}

fn generated_module_matches(declaration: &GeneratedDeclarationFact, qualifier: &str) -> bool {
    declaration.module_name == qualifier
}

/// Register checked generated declarations and their call-site anchors in the
/// existing semantic index. An unqualified call is registered only when one
/// generated declaration owns that leaf; a qualifier selects the matching
/// BuildPlan module identity. This makes iteration order irrelevant and keeps
/// ambiguity unresolved instead of guessing by display name.
pub(crate) fn register_generated_declarations(
    db: &mut SymbolDB,
    plan: &crate::Comptime::Build::BuildPlan,
    source_path: &str,
    tokens: &[Token],
) -> Vec<GeneratedDeclarationFact> {
    let declarations = generated_declarations(plan);
    for declaration in &declarations {
        if db.defs.iter().any(|definition| {
            definition.identity == declaration.identity
                && definition.module_path == declaration.module_path
                && definition.def_span == declaration.span
        }) {
            continue;
        }
        db.defs.push(jet_semindex::SymDef {
            identity: declaration.identity.clone(),
            name: declaration.name.clone(),
            def_span: declaration.span,
            module_path: declaration.module_path.clone(),
            kind: SymKind::Function {
                params: Vec::new(),
                param_contract: Vec::new(),
                param_variadic: Vec::new(),
                ret: None,
                failure_contract: "unknown".to_string(),
                failure_source: "generated BuildPlan declaration".to_string(),
                effects: None,
                effect_via: None,
                param_access: Vec::new(),
                param_defaults: Vec::new(),
                policies: Vec::new(),
            },
        });
    }

    let tokens = crate::Lexer::without_comments(tokens);
    for (index, token) in tokens.iter().enumerate() {
        let TokKind::Ident(name) = &token.kind else {
            continue;
        };
        let Some(next) = tokens.get(index + 1) else {
            continue;
        };
        if !matches!(next.kind, TokKind::LParen) {
            continue;
        }
        let qualifier = generated_qualifier(&tokens, index);
        let matches = declarations
            .iter()
            .filter(|declaration| {
                declaration.name == name.as_str()
                    && qualifier
                        .is_none_or(|qualifier| generated_module_matches(declaration, qualifier))
            })
            .collect::<Vec<_>>();
        let Some(declaration) = (matches.len() == 1).then_some(matches[0]) else {
            continue;
        };
        let anchor = jet_semindex::DefinitionAnchor {
            module_path: declaration.module_path.clone(),
            kind: "function".to_string(),
            def_span: declaration.span.into(),
            semantic_identity: Some(declaration.identity.clone()),
        };
        let existing = db
            .refs
            .iter()
            .enumerate()
            .filter(|(_, reference)| {
                reference.module_path == source_path && reference.span == token.span
            })
            .map(|(index, reference)| {
                let Some(target) = reference.target.as_ref() else {
                    return (index, true, false);
                };
                let generated_target = declarations.iter().any(|candidate| {
                    candidate.name == name.as_str()
                        && candidate.module_path == target.module_path
                        && candidate.span == target.def_span.into()
                });
                let known_target = db.defs.iter().any(|definition| {
                    definition.module_path == target.module_path
                        && definition.def_span == target.def_span.into()
                });
                (index, generated_target, known_target)
            })
            .collect::<Vec<_>>();
        let has_checked_non_generated_target = existing
            .iter()
            .any(|(_, generated_target, known_target)| *known_target && !*generated_target);
        if has_checked_non_generated_target {
            continue;
        }
        if existing.is_empty() {
            db.refs.push(SymRef {
                name: name.clone(),
                span: token.span,
                module_path: source_path.to_string(),
                scope_identity: None,
                target: Some(anchor),
                fact: None,
            });
        } else {
            for (index, _, _) in existing {
                db.refs[index].target = Some(anchor.clone());
            }
        }
    }
    declarations
}

/// Resolve a registered generated fact by its exact artifact and declaration
/// span. This is the sole origin lookup used by LSP definition responses.
pub(crate) fn generated_declaration_at<'a>(
    declarations: &'a [GeneratedDeclarationFact],
    module_path: &str,
    span: Span,
) -> Option<&'a GeneratedDeclarationFact> {
    declarations.iter().find(|declaration| {
        declaration.module_path == module_path && declaration.span == span
    })
}

// ── Go-to-definition ──────────────────────────────────────────────────────────

pub(crate) fn compute_definition(
    db: &SymbolDB,
    tokens: &[Token],
    _src: &str,
    path: &str,
    offset: usize,
) -> Option<(String, Span)> {
    let identity = checked_semantic_identity_at(db, tokens, path, offset)?;
    let mut definition = None;
    for candidate in db.defs.iter().filter(|candidate| candidate.identity == identity) {
        if definition.is_some_and(|existing: &jet_semindex::SymDef| {
            existing.module_path != candidate.module_path || existing.def_span != candidate.def_span
        }) {
            return None;
        }
        definition = Some(candidate);
    }
    definition.map(|definition| (definition.module_path.clone(), definition.def_span))
}

// ── References ────────────────────────────────────────────────────────────────

pub(crate) fn compute_references(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
    include_declaration: bool,
) -> Vec<(String, Span)> {
    let Some(identity) = checked_semantic_identity_at(db, tokens, path, offset) else {
        return Vec::new();
    };
    let mut result: Vec<(String, Span)> = db
        .refs
        .iter()
        .filter_map(|reference| {
            let target = reference.target.as_ref()?;
            (checked_anchor_identity(db, target).as_deref() == Some(identity.as_str()))
                .then_some((reference.module_path.clone(), reference.span))
        })
        .collect();
    if include_declaration {
        result.extend(
            db.defs
                .iter()
                .filter(|definition| definition.identity == identity)
                .map(|definition| (definition.module_path.clone(), definition.def_span)),
        );
    }
    result.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.start.cmp(&b.1.start))
            .then(a.1.end.cmp(&b.1.end))
    });
    result.dedup();
    result
}

#[cfg(test)]
mod generic_instance_tests {
    use super::{compute_definition, compute_references, compute_rename};
    use crate::LSP::Completion::compute_completions;

    #[test]
    fn references_join_applicative_generic_module_aliases() {
        let root =
            std::env::temp_dir().join(format!("jet_lsp_genmod_identity_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("main.jet");
        let source = "module value(n: Int) { pub fn get() Int { return n } }\nmodule a :: value(3)\nmodule b :: value(3)\nfn run() { print(a.get()); print(b.get()) }\n";
        std::fs::write(&path, source).unwrap();
        let shown = path.to_string_lossy().into_owned();
        let mut bundle = crate::Loader::load_entry(&shown).unwrap();
        let (diagnostics, facts) = crate::Sema::check_bundle_with_effect_facts(
            &mut bundle,
            crate::Sema::CompileMode::Check,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error),
            "{diagnostics:#?}"
        );
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(source);
        assert!(lex_diagnostics.is_empty());
        let references =
            compute_references(&db, &tokens, &shown, source.find("a.get").unwrap(), true);
        let spellings: Vec<_> = references
            .iter()
            .map(|(_, span)| &source[span.start..span.end])
            .collect();
        assert!(
            spellings.iter().any(|spelling| *spelling == "a"),
            "{spellings:?}"
        );
        assert!(
            spellings.iter().any(|spelling| *spelling == "b"),
            "{spellings:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn references_never_join_same_alias_spelling_across_distinct_instances() {
        let root =
            std::env::temp_dir().join(format!("jet_lsp_genmod_hostile_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let left = root.join("left.jet");
        let right = root.join("right.jet");
        let left_source = "module value(n: Int) { pub fn get() Int { return n } }\nmodule same :: value(3)\nfn left_value() Int { return same.get() }\n";
        let right_source = "module value(n: Int) { pub fn get() Int { return n } }\nmodule same :: value(4)\nfn right_value() Int { return same.get() }\n";
        std::fs::write(&main, "module left\nmodule right\nfn run() {}\n").unwrap();
        std::fs::write(&left, left_source).unwrap();
        std::fs::write(&right, right_source).unwrap();
        let shown_main = main.to_string_lossy().into_owned();
        let shown_left = "left.jet".to_string();
        let shown_right = "right.jet".to_string();
        let mut bundle = crate::Loader::load_entry(&shown_main).unwrap();
        let (diagnostics, facts) = crate::Sema::check_bundle_with_effect_facts(
            &mut bundle,
            crate::Sema::CompileMode::Check,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error),
            "{diagnostics:#?}"
        );
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(left_source);
        assert!(lex_diagnostics.is_empty());
        let offset = left_source.find("same ::").unwrap();
        let references = compute_references(&db, &tokens, &shown_left, offset, true);
        assert!(
            references
                .iter()
                .any(|(path, span)| path == &shown_left
                    && &left_source[span.start..span.end] == "same"),
            "refs={references:?} defs={:#?} instances={:#?}",
            db.defs,
            db.index.instances()
        );
        assert!(
            !references.iter().any(|(path, _)| path == &shown_right),
            "distinct value(4) instance joined by alias spelling: {references:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn import_alias_references_and_rename_use_the_exact_definition_anchor() {
        let root = std::env::temp_dir().join(format!(
            "jet_lsp_import_alias_identity_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let library = root.join("library.jet");
        let main = root.join("main.jet");
        let source = "use \"./library\" as api\nfn run() { api.report() }\n";
        std::fs::write(&library, "pub fn report() { print(\"library\") }\n").unwrap();
        std::fs::write(&main, source).unwrap();
        let shown = main.to_string_lossy().into_owned();
        let mut bundle = crate::Loader::load_entry(&shown).unwrap();
        let (diagnostics, facts) = crate::Sema::check_bundle_with_effect_facts(
            &mut bundle,
            crate::Sema::CompileMode::Check,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error),
            "{diagnostics:#?}"
        );
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(source);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:#?}");
        let use_offset = source.find("api.report").unwrap();
        let alias_offset = source.find("as api").unwrap() + 3;
        let references = compute_references(&db, &tokens, &shown, use_offset, true);
        assert!(
            references
                .iter()
                .any(|(path, span)| { path == &shown && &source[span.start..span.end] == "api" }),
            "references={references:?}"
        );
        let renamed_from_use = compute_rename(&db, &tokens, &shown, use_offset, "backend")
            .expect("alias use should be renameable");
        let renamed_from_definition = compute_rename(&db, &tokens, &shown, alias_offset, "backend")
            .expect("alias definition should be renameable");
        for spans in [renamed_from_use, renamed_from_definition] {
            assert!(
                spans.iter().any(|(path, span)| {
                    path == &shown && &source[span.start..span.end] == "api"
                }),
                "rename spans={spans:?}"
            );
            assert!(
                spans.len() >= 2,
                "rename must include the declaration and qualified use: {spans:?}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typestate_navigation_and_completion_keep_the_struct_owner() {
        let root =
            std::env::temp_dir().join(format!("jet_lsp_typestate_owner_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("main.jet");
        let source = "struct Door {\n    state { Closed, Open }\n}\nimpl Door {\n    #Transition(_, Door.State.Closed) fn new() Door -[]> { return Door{} }\n    #Transition(Door.State.Closed, Door.State.Open) fn open(self: ^Door) Door -[]> { return self }\n}\nfn run() {}\n";
        std::fs::write(&path, source).unwrap();
        let shown = path.to_string_lossy().into_owned();
        let mut bundle = crate::Loader::load_entry(&shown).unwrap();
        let (diagnostics, facts) = crate::Sema::check_bundle_with_effect_facts(
            &mut bundle,
            crate::Sema::CompileMode::Check,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error),
            "{diagnostics:#?}"
        );
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(source);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:#?}");

        let marker = source.find("Door.State.Closed").unwrap();
        let closed = marker + "Door.State.".len();
        let (definition_path, definition_span) =
            compute_definition(&db, &tokens, source, &shown, closed).expect("state definition");
        assert_eq!(definition_path, shown);
        assert_eq!(
            &source[definition_span.start..definition_span.end],
            "Closed"
        );

        let references = compute_references(&db, &tokens, &shown, closed, true);
        assert!(references
            .iter()
            .any(|(path, span)| { path == &shown && &source[span.start..span.end] == "Closed" }));
        assert!(references.iter().any(|(path, span)| {
            path == &shown && &source[span.start..span.end] == "Door.State.Closed"
        }));

        let renamed = compute_rename(&db, &tokens, &shown, closed, "Sealed").expect("state rename");
        assert!(renamed
            .iter()
            .all(|(path, span)| { path == &shown && &source[span.start..span.end] == "Closed" }));

        let completion_source = format!("{source}fn editor() {{\n    Door.State.\n}}\n");
        let completion_offset =
            completion_source.find("Door.State.\n").unwrap() + "Door.State.".len();
        let labels = compute_completions(
            &db,
            &completion_source,
            completion_offset,
            &shown,
            None,
            None,
        )
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
        assert!(labels.iter().any(|label| label == "Closed"), "{labels:?}");
        assert!(labels.iter().any(|label| label == "Open"), "{labels:?}");

        let _ = std::fs::remove_dir_all(root);
    }
}

// ── Rename ────────────────────────────────────────────────────────────────────

fn is_keyword(name: &str) -> bool {
    JET_KEYWORDS.contains(&name) || JET_TYPES.contains(&name)
}

fn is_valid_ident(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn state_anchor_at(
    db: &SymbolDB,
    path: &str,
    offset: usize,
) -> Option<jet_semindex::DefinitionAnchor> {
    let reference = match checked_reference_in_span(db, path, offset, offset) {
        Err(()) => return None,
        Ok(Some(reference)) => Some(reference),
        Ok(None) => None,
    };
    if let Some(target) = reference.and_then(|reference| {
        reference
            .target
            .as_ref()
            .filter(|target| target.kind == "state")
    }) {
        let mut target = target.clone();
        target.semantic_identity = Some(checked_anchor_identity(db, &target)?);
        return Some(target);
    }
    let definition = checked_definition_in_span(db, path, offset, offset)
        .ok()
        .flatten()
        .filter(|definition| {
            matches!(
                &definition.kind,
                SymKind::EnumVariant { parent } if parent.ends_with(".State")
            )
        })?;
    Some(jet_semindex::DefinitionAnchor {
        module_path: definition.module_path.clone(),
        kind: "state".to_string(),
        def_span: definition.def_span.into(),
        semantic_identity: Some(definition.identity.clone()),
    })
}


fn state_leaf_span(reference: &jet_semindex::SymRef) -> Option<Span> {
    let target = reference
        .target
        .as_ref()
        .filter(|target| target.kind == "state")?;
    let leaf_len = target.def_span.end.saturating_sub(target.def_span.start);
    let start = reference.span.end.checked_sub(leaf_len)?;
    (reference.span.start <= start).then_some(Span::new(start, reference.span.end))
}

/// Compute a workspace edit for renaming the symbol at `offset` to `new_name`.
/// Returns `Err(msg)` if the rename is invalid.
pub(crate) fn compute_rename(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
    new_name: &str,
) -> Result<Vec<(String, Span)>, String> {
    if !is_valid_ident(new_name) {
        return Err(format!("`{}` is not a valid identifier", new_name));
    }
    if crate::Syntax::classify_identifier(new_name) == crate::Syntax::IdentifierClass::Reserved {
        return Err(format!(
            "`{new_name}` is reserved for Jet and cannot be used as a name"
        ));
    }
    if is_keyword(new_name) {
        return Err(format!(
            "`{}` is a keyword and cannot be used as a name",
            new_name
        ));
    }
    if let Some(fact) = compiler_fact_at(tokens, offset) {
        return Err(format!(
            "compiler-owned {} is fixed; rename the reflected type or field instead",
            fact
        ));
    }
    let name = match find_ident_at(tokens, offset) {
        Some(n) => n,
        None => return Err("no identifier at cursor".to_string()),
    };
    if is_keyword(name) {
        return Err(format!("`{}` is a keyword and cannot be renamed", name));
    }
    let identity = checked_semantic_identity_at(db, tokens, path, offset)
        .ok_or_else(|| "identifier has no checked semantic target".to_string())?;
    let alias_target = match checked_reference_in_span(db, path, offset, offset) {
        Ok(Some(reference)) => reference
            .target
            .as_ref()
            .filter(|target| target.kind == "import_alias")
            .and_then(|target| {
                let mut target = target.clone();
                target.semantic_identity = Some(checked_anchor_identity(db, &target)?);
                Some(target)
            }),
        Ok(None) => checked_definition_in_span(db, path, offset, offset)
            .ok()
            .flatten()
            .filter(|definition| definition.identity.starts_with("import:"))
            .map(|definition| jet_semindex::DefinitionAnchor {
                module_path: definition.module_path.clone(),
                kind: "import_alias".to_string(),
                def_span: definition.def_span.into(),
                semantic_identity: Some(definition.identity.clone()),
            }),
        Err(()) => None,
    };
    let indexed_case = db
        .defs
        .iter()
        .find(|def| def.identity == identity)
        .map(|def| match &def.kind {
            SymKind::Struct { .. }
            | SymKind::Enum { .. }
            | SymKind::Trait
            | SymKind::Tag
            | SymKind::Type { .. }
            | SymKind::EnumVariant { .. } => ("type-like name", Syntax::NameCase::Pascal),
            SymKind::Module
            | SymKind::Function { .. }
            | SymKind::Const
            | SymKind::Field { .. }
            | SymKind::Local { .. }
            | SymKind::Param { .. } => ("value-like name", Syntax::NameCase::Snake),
        });
    // Some declaration families are expanded before SemIndex sees the checked
    // bundle (protocols, derives, state/unit sugar). Their already-validated
    // source spelling still determines the strict two-tier category exactly.
    let source_case = match (
        Syntax::name_has_case(name, Syntax::NameCase::Pascal),
        Syntax::name_has_case(name, Syntax::NameCase::Snake),
    ) {
        (true, false) => Some(("type-like name", Syntax::NameCase::Pascal)),
        (false, true) => Some(("value-like name", Syntax::NameCase::Snake)),
        _ => None,
    };
    if let Some((category, case)) = source_case.or(indexed_case) {
        if !Syntax::name_has_case(new_name, case) {
            return Err(format!(
                "`{new_name}` is not a valid {category}; use `{}`",
                Syntax::canonical_name_case(new_name, case)
            ));
        }
    }
    if let Some(target) = alias_target {
        let mut spans = vec![(
            target.module_path.clone(),
            Span::new(target.def_span.start, target.def_span.end),
        )];
        spans.extend(db.refs.iter().filter_map(|reference| {
            let candidate = reference.target.as_ref()?;
            (candidate.kind == "import_alias"
                && checked_anchor_identity(db, candidate).as_deref() == Some(identity.as_str()))
                .then_some((reference.module_path.clone(), reference.span))
        }));
        spans.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.start.cmp(&right.1.start))
                .then(left.1.end.cmp(&right.1.end))
        });
        spans.dedup();
        return Ok(spans);
    }
    if let Some(target) = state_anchor_at(db, path, offset) {
        let mut spans = compute_references(db, tokens, path, offset, true);
        for (reference_path, span) in &mut spans {
            let Some(reference) = db.refs.iter().find(|reference| {
                reference.module_path == *reference_path
                    && reference.span == *span
                    && reference.target.as_ref().is_some_and(|candidate| {
                        candidate.kind == "state"
                            && checked_anchor_identity(db, candidate).as_deref()
                                == target.semantic_identity.as_deref()
                    })
            }) else {
                continue;
            };
            if let Some(leaf_span) = state_leaf_span(reference) {
                *span = leaf_span;
            }
        }
        spans.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.start.cmp(&right.1.start))
                .then(left.1.end.cmp(&right.1.end))
        });
        spans.dedup();
        if !spans.is_empty() {
            return Ok(spans);
        }
    }
    let mut spans: Vec<(String, Span)> = db
        .refs
        .iter()
        .filter_map(|reference| {
            let target = reference.target.as_ref()?;
            (checked_anchor_identity(db, target).as_deref() == Some(identity.as_str()))
                .then_some((reference.module_path.clone(), reference.span))
        })
        .collect();
    spans.extend(
        db.defs
            .iter()
            .filter(|definition| definition.identity == identity)
            .map(|definition| (definition.module_path.clone(), definition.def_span)),
    );
    if spans.is_empty() {
        return Err(format!("no checked occurrences of `{}` found", name));
    }
    spans.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.start.cmp(&right.1.start))
            .then(left.1.end.cmp(&right.1.end))
    });
    spans.dedup();
    Ok(spans)
}

// ── Code actions ──────────────────────────────────────────────────────────────

pub(crate) struct RefactorAction {
    pub title: String,
    pub kind: &'static str,
    pub edits: Vec<TextEdit>,
}

pub(crate) fn compute_refactor_actions(
    db: &SymbolDB,
    tokens: &[Token],
    diagnostics: &[Diagnostic],
    src: &str,
    path: &str,
    workspace_root: Option<&str>,
    import_sources: &std::collections::HashMap<String, String>,
    excluded_import_paths: &std::collections::HashSet<String>,
    requested: Span,
) -> Vec<RefactorAction> {
    let mut actions = adjacent_subject_dispatch_actions(diagnostics, requested);
    actions.extend(import_actions(
        db,
        diagnostics,
        src,
        path,
        workspace_root,
        import_sources,
        excluded_import_paths,
        requested,
    ));
    let Some(selected) = trim_span(src, requested) else {
        actions.extend(inline_actions(db, tokens, src, path, requested));
        return actions;
    };
    let mut extracted = false;
    if let Some(selected) = extractable_expr(db, src, path, selected)
        .filter(|span| !is_trivial_extract(tokens, *span))
        .filter(|span| is_total_pure_expr(db, tokens, path, *span))
    {
        if let Some(insert_at) = extract_insert_point(db, src, path, selected) {
            if let Some(expression) = src.get(selected.start..selected.end) {
                let indent_len = src[insert_at..]
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .map(char::len_utf8)
                    .sum::<usize>();
                let indent = &src[insert_at..insert_at + indent_len];
                let name = fresh_name(db, "extracted_value");
                actions.push(RefactorAction {
                    title: "Extract binding".to_string(),
                    kind: "refactor.extract",
                    edits: vec![
                        TextEdit {
                            span: Span::new(insert_at, insert_at),
                            new_text: format!("{indent}{name} :: {expression}\n"),
                        },
                        TextEdit {
                            span: selected,
                            new_text: name,
                        },
                    ],
                });
                if let Some(action) =
                    extract_function_action(db, tokens, src, path, selected, expression)
                {
                    actions.push(action);
                }
                extracted = true;
            }
        }
    }
    if !extracted {
        actions.extend(inline_actions(db, tokens, src, path, selected));
    }
    actions
}

fn adjacent_subject_dispatch_actions(
    diagnostics: &[Diagnostic],
    requested: Span,
) -> Vec<RefactorAction> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "L0514")
        .filter(|diagnostic| {
            diagnostic
                .edit
                .as_ref()
                .map(|edit| spans_touch(edit.span, requested))
                .or_else(|| diagnostic.span.map(|span| spans_touch(span, requested)))
                .unwrap_or(false)
        })
        .filter_map(|diagnostic| {
            diagnostic.edit.clone().map(|edit| RefactorAction {
                title: "Rewrite adjacent guards as an ordered subject table".to_string(),
                kind: "refactor.rewrite",
                edits: vec![edit],
            })
        })
        .collect()
}

fn import_actions(
    db: &SymbolDB,
    diagnostics: &[Diagnostic],
    src: &str,
    path: &str,
    workspace_root: Option<&str>,
    import_sources: &std::collections::HashMap<String, String>,
    excluded_import_paths: &std::collections::HashSet<String>,
    requested: Span,
) -> Vec<RefactorAction> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for diagnostic in diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code.as_str(), "E0102" | "E0107"))
    {
        let Some(span) = diagnostic.span.filter(|span| spans_touch(*span, requested)) else {
            continue;
        };
        let Some(name) = src.get(span.start..span.end) else {
            continue;
        };
        let mut modules = db
            .symbols
            .lookup(name)
            .into_iter()
            .filter(|symbol| {
                symbol.module_path != path
                    && !excluded_import_paths.contains(&symbol.module_path)
                    && matches!(
                        symbol.provenance,
                        jet_semindex::SemanticProvenance::Source { .. }
                    )
                    && source_symbol_is_exported(symbol, import_sources)
                    && !matches!(
                        &symbol.kind,
                        jet_semindex::SemanticSymbolKind::Local
                            | jet_semindex::SemanticSymbolKind::Parameter
                            | jet_semindex::SemanticSymbolKind::Member
                    )
            })
            .filter_map(|symbol| {
                use_statement_for_module(path, workspace_root, &symbol.module_path)
            })
            .collect::<Vec<_>>();
        modules.sort();
        modules.dedup();
        let [statement] = modules.as_slice() else {
            continue;
        };
        if src.lines().any(|line| line.trim() == statement.trim())
            || !seen.insert(statement.clone())
        {
            continue;
        }
        let module = statement
            .trim()
            .strip_prefix("use ")
            .unwrap_or(statement.trim());
        out.push(RefactorAction {
            title: format!("Import `{module}`"),
            kind: "quickfix",
            edits: vec![TextEdit {
                span: Span::new(0, 0),
                new_text: statement.clone(),
            }],
        });
    }
    out
}

fn extract_function_action(
    db: &SymbolDB,
    tokens: &[Token],
    src: &str,
    path: &str,
    selected: Span,
    expression: &str,
) -> Option<RefactorAction> {
    if !is_total_pure_expr(db, tokens, path, selected) {
        return None;
    }
    let return_type = infer_total_pure_return_type(db, tokens, path, selected)?;
    if !is_scalar_name(&return_type) {
        return None;
    }
    let function = db
        .index
        .definition_facts()
        .iter()
        .filter(|definition| {
            definition.module_path == path
                && definition.kind == "function"
                && definition.span.start <= selected.start
                && selected.end <= definition.span.end
        })
        .min_by_key(|definition| definition.span.end - definition.span.start)?;

    let mut inputs: Vec<(&jet_semindex::SymDef, usize)> = Vec::new();
    for reference in db.refs.iter().filter(|reference| {
        reference.module_path == path
            && selected.start <= reference.span.start
            && reference.span.end <= selected.end
    }) {
        let Some(target) = reference.target.as_ref() else {
            return None;
        };
        let Some(definition) = definition_for_anchor(db, target) else {
            return None;
        };
        if selected.start <= definition.def_span.start && definition.def_span.end <= selected.end {
            continue;
        }
        match &definition.kind {
            SymKind::Param { ty } if is_scalar_type(ty) => {}
            SymKind::Local {
                mutable: false,
                ty: Some(ty),
            } if is_scalar_type(ty) => {}
            SymKind::Local { .. } | SymKind::Param { .. } => return None,
            _ => continue,
        }
        if let Some((_, first)) = inputs
            .iter_mut()
            .find(|(existing, _)| existing.identity == definition.identity)
        {
            *first = (*first).min(reference.span.start);
        } else {
            inputs.push((definition, reference.span.start));
        }
    }
    inputs.sort_by_key(|(_, first)| *first);
    let function_name = fresh_name(db, "extracted_fn");
    let params = inputs
        .iter()
        .map(|(definition, _)| {
            let ty = match &definition.kind {
                SymKind::Param { ty } | SymKind::Local { ty: Some(ty), .. } => ty,
                _ => unreachable!(),
            };
            format!("{}: {}", definition.name, ty.name())
        })
        .collect::<Vec<_>>()
        .join(", ");
    let args = inputs
        .iter()
        .map(|(definition, _)| definition.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let insert = line_start(src, function.span.start);
    Some(RefactorAction {
        title: "Extract function".to_string(),
        kind: "refactor.extract",
        edits: vec![
            TextEdit {
                span: Span::new(insert, insert),
                new_text: format!(
                    "fn {function_name}({params}) {return_type} {{ return {expression} }}\n\n"
                ),
            },
            TextEdit {
                span: selected,
                new_text: format!("{function_name}({args})"),
            },
        ],
    })
}

fn inline_actions(
    db: &SymbolDB,
    tokens: &[Token],
    src: &str,
    path: &str,
    requested: Span,
) -> Vec<RefactorAction> {
    let Some(reference) = db
        .refs
        .iter()
        .find(|reference| reference.module_path == path && spans_touch(reference.span, requested))
    else {
        return Vec::new();
    };
    let Some(target) = reference.target.as_ref() else {
        return Vec::new();
    };
    let Some(binding) = definition_for_anchor(db, target) else {
        return Vec::new();
    };
    if !matches!(binding.kind, SymKind::Local { mutable: false, .. }) {
        return Vec::new();
    }
    let uses: Vec<_> = db
        .refs
        .iter()
        .filter(|candidate| {
            candidate.module_path == path
                && candidate
                    .target
                    .as_ref()
                    .is_some_and(|anchor| same_anchor(anchor, target))
        })
        .collect();
    if uses.is_empty() {
        return Vec::new();
    }
    let Some(initializer) = initializer_for_binding(db, src, path, binding) else {
        return Vec::new();
    };
    let initializer_span = Span::new(initializer.span.start, initializer.span.end);
    if !is_total_pure_expr(db, tokens, path, initializer_span)
        || !initializer_refs_are_stable(db, path, initializer_span)
    {
        return Vec::new();
    }
    let start = line_start(src, binding.def_span.start);
    let end = line_end_including_newline(src, initializer_span.end);
    let prefix = &src[start..binding.def_span.start];
    let suffix = src
        .get(initializer_span.end..end)
        .unwrap_or("")
        .trim_end_matches(['\r', '\n'])
        .trim();
    if !prefix.chars().all(char::is_whitespace)
        || !suffix.trim_end_matches(';').trim().is_empty()
        || uses.iter().any(|use_site| end > use_site.span.start)
    {
        return Vec::new();
    }
    let Some(expression) = src.get(initializer_span.start..initializer_span.end) else {
        return Vec::new();
    };
    let mut edits: Vec<TextEdit> = uses
        .iter()
        .map(|use_site| TextEdit {
            span: use_site.span,
            new_text: format!("({expression})"),
        })
        .collect();
    edits.push(TextEdit {
        span: Span::new(start, end),
        new_text: String::new(),
    });
    vec![RefactorAction {
        title: format!("Inline `{}`", binding.name),
        kind: "refactor.inline",
        edits,
    }]
}

fn initializer_refs_are_stable(db: &SymbolDB, path: &str, span: Span) -> bool {
    db.refs
        .iter()
        .filter(|reference| {
            reference.module_path == path
                && span.start <= reference.span.start
                && reference.span.end <= span.end
        })
        .all(|reference| {
            reference
                .target
                .as_ref()
                .and_then(|target| definition_for_anchor(db, target))
                .is_some_and(|definition| {
                    !matches!(&definition.kind, SymKind::Local { mutable: true, .. })
                })
        })
}

fn source_symbol_is_exported(
    symbol: &jet_semindex::SemanticSymbol,
    sources: &std::collections::HashMap<String, String>,
) -> bool {
    let Some(span) = symbol.span else {
        return false;
    };
    let Some(source) = sources.get(&symbol.module_path) else {
        return false;
    };
    let start = line_start(source, span.start);
    source.get(start..span.start).is_some_and(|prefix| {
        prefix
            .split_whitespace()
            .any(|word| word == "pub" || word.starts_with("pub("))
    })
}

fn exact_expr<'a>(
    db: &'a SymbolDB,
    path: &str,
    selected: Span,
) -> Option<&'a jet_semindex::StructuralNode> {
    db.nodes.iter().find(|node| {
        node.module_path == path
            && node.class == "expr"
            && node.span.start == selected.start
            && node.span.end == selected.end
    })
}

/// Exact expr node, or a single outer `(…)` group around one.
fn extractable_expr(db: &SymbolDB, src: &str, path: &str, selected: Span) -> Option<Span> {
    if exact_expr(db, path, selected).is_some() {
        return Some(selected);
    }
    let text = src.get(selected.start..selected.end)?;
    let start_ws = text.len() - text.trim_start().len();
    let end_ws = text.len() - text.trim_end().len();
    let core = text.trim();
    if core.len() < 2 || !core.starts_with('(') || !core.ends_with(')') {
        return None;
    }
    let inner = Span::new(selected.start + start_ws + 1, selected.end - end_ws - 1);
    let inner = trim_span(src, inner)?;
    exact_expr(db, path, inner).map(|_| selected)
}

fn extract_insert_point(db: &SymbolDB, src: &str, path: &str, selected: Span) -> Option<usize> {
    if let Some(initializer) = db.nodes.iter().find(|node| {
        node.module_path == path
            && node.class == "expr"
            && node.slot == "initializer"
            && node.span.start == selected.start
            && node.span.end == selected.end
    }) {
        if let Some(binding) = binding_for_initializer(db, src, path, initializer) {
            return Some(line_start(src, binding.def_span.start));
        }
    }
    // Call expr-stmts currently record a narrow callee-name stmt span, so
    // enclosure checks against stmt nodes miss argument subexpressions.
    // Insert on the line that holds the selection.
    Some(line_start(src, selected.start))
}

fn binding_for_initializer<'a>(
    db: &'a SymbolDB,
    src: &str,
    path: &str,
    initializer: &jet_semindex::StructuralNode,
) -> Option<&'a jet_semindex::SymDef> {
    let start = line_start(src, initializer.span.start);
    db.defs
        .iter()
        .filter(|definition| {
            definition.module_path == path
                && matches!(&definition.kind, SymKind::Local { .. })
                && start <= definition.def_span.start
                && definition.def_span.end <= initializer.span.start
        })
        .max_by_key(|definition| definition.def_span.start)
}

fn initializer_for_binding<'a>(
    db: &'a SymbolDB,
    src: &str,
    path: &str,
    binding: &jet_semindex::SymDef,
) -> Option<&'a jet_semindex::StructuralNode> {
    let end = src[binding.def_span.start..]
        .find(['\r', '\n'])
        .map_or(src.len(), |offset| binding.def_span.start + offset);
    db.nodes
        .iter()
        .filter(|node| {
            node.module_path == path
                && node.class == "expr"
                && node.slot == "initializer"
                && binding.def_span.end <= node.span.start
                && node.span.end <= end
        })
        .min_by_key(|node| node.span.start)
}

fn definition_for_anchor<'a>(
    db: &'a SymbolDB,
    anchor: &jet_semindex::DefinitionAnchor,
) -> Option<&'a jet_semindex::SymDef> {
    db.defs.iter().find(|definition| {
        anchor
            .semantic_identity
            .as_ref()
            .is_some_and(|identity| identity == &definition.identity)
            || (definition.module_path == anchor.module_path
                && definition.def_span.start == anchor.def_span.start
                && definition.def_span.end == anchor.def_span.end)
    })
}

fn same_anchor(
    left: &jet_semindex::DefinitionAnchor,
    right: &jet_semindex::DefinitionAnchor,
) -> bool {
    match (&left.semantic_identity, &right.semantic_identity) {
        (Some(left), Some(right)) => left == right,
        _ => {
            left.module_path == right.module_path
                && left.def_span.start == right.def_span.start
                && left.def_span.end == right.def_span.end
        }
    }
}

fn is_scalar_type(ty: &crate::AST::Type) -> bool {
    is_scalar_name(&ty.name())
}

fn is_scalar_name(name: &str) -> bool {
    matches!(name, "Bool" | "Char" | "Int" | "Float")
}

fn is_trivial_extract(tokens: &[Token], span: Span) -> bool {
    let significant: Vec<_> = tokens
        .iter()
        .filter(|token| {
            span.start <= token.span.start
                && token.span.end <= span.end
                && token.span.start < token.span.end
                && !matches!(token.kind, TokKind::Eof | TokKind::LParen | TokKind::RParen)
        })
        .collect();
    matches!(
        significant.as_slice(),
        [token]
            if matches!(
                token.kind,
                TokKind::Ident(_)
                    | TokKind::Int(_, _)
                    | TokKind::Float(..)
                    | TokKind::Char(_)
                    | TokKind::KwTrue
                    | TokKind::KwFalse
                    | TokKind::Str(_)
            )
    )
}

fn is_total_pure_expr(db: &SymbolDB, tokens: &[Token], path: &str, span: Span) -> bool {
    let enclosed: Vec<&Token> = tokens
        .iter()
        .filter(|token| {
            span.start <= token.span.start
                && token.span.end <= span.end
                && token.span.start < token.span.end
                && !matches!(token.kind, TokKind::Eof)
        })
        .collect();
    if enclosed.is_empty() {
        return false;
    }
    // `name()` / `name(args)` are effect-unknown calls even when parens are otherwise
    // allowed for grouping. Reject any Ident immediately followed by `(`.
    for pair in enclosed.windows(2) {
        if matches!(pair[0].kind, TokKind::Ident(_)) && matches!(pair[1].kind, TokKind::LParen) {
            return false;
        }
    }
    let mut has_comparison = false;
    for token in &enclosed {
        let safe = match &token.kind {
            TokKind::Ident(_)
            | TokKind::Int(_, _)
            | TokKind::Float(..)
            | TokKind::Char(_)
            | TokKind::KwTrue
            | TokKind::KwFalse
            | TokKind::AndAnd
            | TokKind::OrOr
            | TokKind::Bang
            | TokKind::LParen
            | TokKind::RParen => true,
            TokKind::EqEq | TokKind::NotEq => {
                has_comparison = true;
                true
            }
            TokKind::Str(parts) => parts
                .iter()
                .all(|part| matches!(part, crate::Lexer::StrTokPart::Lit(_))),
            _ => false,
        };
        if !safe {
            return false;
        }
    }
    if has_comparison {
        return expr_comparison_operands_are_scalar(db, tokens, path, span);
    }
    true
}

/// `==`/`!=` are total for matching scalars. Bool ops (`&&`/`||`/`!`) may only
/// combine Bool leaves. Pure scalar comparisons (no bool ops) allow any one
/// matching scalar type: Bool, Char, Int, or Float.
fn expr_comparison_operands_are_scalar(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    span: Span,
) -> bool {
    let enclosed: Vec<&Token> = tokens
        .iter()
        .filter(|token| {
            span.start <= token.span.start
                && token.span.end <= span.end
                && token.span.start < token.span.end
        })
        .collect();
    let has_bool_op = enclosed
        .iter()
        .any(|token| matches!(token.kind, TokKind::AndAnd | TokKind::OrOr | TokKind::Bang));
    let mut leaf: Option<String> = None;
    for token in &enclosed {
        match &token.kind {
            TokKind::KwTrue | TokKind::KwFalse => {
                if !agree_scalar_leaf(&mut leaf, "Bool") {
                    return false;
                }
            }
            TokKind::Int(_, _) => {
                if has_bool_op || !agree_scalar_leaf(&mut leaf, "Int") {
                    return false;
                }
            }
            TokKind::Float(..) => {
                if has_bool_op || !agree_scalar_leaf(&mut leaf, "Float") {
                    return false;
                }
            }
            TokKind::Char(_) => {
                if has_bool_op || !agree_scalar_leaf(&mut leaf, "Char") {
                    return false;
                }
            }
            TokKind::Ident(_) => {
                let Some(name) = resolved_type_name(db, path, token.span) else {
                    return false;
                };
                if !is_scalar_name(&name) {
                    return false;
                }
                if has_bool_op && name != "Bool" {
                    return false;
                }
                if !agree_scalar_leaf(&mut leaf, &name) {
                    return false;
                }
            }
            TokKind::AndAnd
            | TokKind::OrOr
            | TokKind::Bang
            | TokKind::EqEq
            | TokKind::NotEq
            | TokKind::LParen
            | TokKind::RParen => {}
            _ => return false,
        }
    }
    leaf.as_deref().is_some_and(is_scalar_name)
}

fn agree_scalar_leaf(leaf: &mut Option<String>, name: &str) -> bool {
    match leaf {
        Some(existing) => existing == name,
        None => {
            *leaf = Some(name.to_string());
            true
        }
    }
}

fn resolved_type_name(db: &SymbolDB, path: &str, span: Span) -> Option<String> {
    let reference = db.refs.iter().find(|reference| {
        reference.module_path == path
            && reference.span.start == span.start
            && reference.span.end == span.end
    })?;
    let target = reference.target.as_ref()?;
    let definition = definition_for_anchor(db, target)?;
    match &definition.kind {
        SymKind::Param { ty } => Some(ty.name()),
        SymKind::Local { ty: Some(ty), .. } => Some(ty.name()),
        _ => None,
    }
}

fn infer_total_pure_return_type(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    span: Span,
) -> Option<String> {
    let mut saw_bool_op = false;
    let mut saw_comparison = false;
    let mut literal: Option<&str> = None;
    let mut from_ident: Option<String> = None;
    for token in tokens.iter().filter(|token| {
        span.start <= token.span.start
            && token.span.end <= span.end
            && token.span.start < token.span.end
            && !matches!(token.kind, TokKind::Eof)
    }) {
        match &token.kind {
            TokKind::AndAnd
            | TokKind::OrOr
            | TokKind::Bang
            | TokKind::KwTrue
            | TokKind::KwFalse => {
                saw_bool_op = true;
            }
            TokKind::EqEq | TokKind::NotEq => saw_comparison = true,
            TokKind::Int(_, _) => {
                if literal.is_some_and(|existing| existing != "Int") {
                    return None;
                }
                literal = Some("Int");
            }
            TokKind::Float(..) => {
                if literal.is_some_and(|existing| existing != "Float") {
                    return None;
                }
                literal = Some("Float");
            }
            TokKind::Char(_) => {
                if literal.is_some_and(|existing| existing != "Char") {
                    return None;
                }
                literal = Some("Char");
            }
            TokKind::Ident(_) => {
                let name = resolved_type_name(db, path, token.span)?;
                if !is_scalar_name(&name) {
                    return None;
                }
                match &from_ident {
                    Some(existing) if existing != &name => {
                        if !saw_bool_op && !saw_comparison {
                            return None;
                        }
                    }
                    None => from_ident = Some(name),
                    _ => {}
                }
            }
            TokKind::LParen | TokKind::RParen => {}
            TokKind::Str(_) => return None,
            _ => return None,
        }
    }
    if saw_bool_op || saw_comparison {
        return Some("Bool".to_string());
    }
    if let Some(name) = literal {
        return Some(name.to_string());
    }
    from_ident.filter(|name| is_scalar_name(name))
}

#[cfg(test)]
mod refactor_safety_tests {
    use super::{extract_function_action, infer_total_pure_return_type, is_total_pure_expr};
    use crate::Diagnostics::{Severity, Span};

    #[test]
    fn code_actions_reject_non_scalar_return_at_type_gate() {
        let root =
            std::env::temp_dir().join(format!("jet-lsp-string-result-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("main.jet");
        let source = "fn run() {\n    print(\"result\")\n}\n";
        std::fs::write(&path, source).unwrap();
        let shown = path.to_string_lossy().into_owned();
        let mut bundle = crate::Loader::load_entry(&shown).unwrap();
        let (diagnostics, facts) = crate::Sema::check_bundle_with_effect_facts(
            &mut bundle,
            crate::Sema::CompileMode::Check,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != Severity::Error),
            "{diagnostics:#?}"
        );
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(source);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:#?}");
        let start = source.find("\"result\"").unwrap();
        let selected = Span::new(start, start + "\"result\"".len());

        assert!(is_total_pure_expr(&db, &tokens, &shown, selected));
        assert_eq!(
            infer_total_pure_return_type(&db, &tokens, &shown, selected),
            None
        );
        assert!(
            extract_function_action(&db, &tokens, source, &shown, selected, "\"result\"").is_none()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

fn trim_span(src: &str, span: Span) -> Option<Span> {
    let text = src.get(span.start..span.end)?;
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    (start < end).then(|| Span::new(span.start + start, span.start + end))
}

fn spans_touch(left: Span, right: Span) -> bool {
    left.start <= right.end && right.start <= left.end
}

fn line_start(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .rfind('\n')
        .map_or(0, |index| index + 1)
}

fn line_end_including_newline(src: &str, offset: usize) -> usize {
    src[offset.min(src.len())..]
        .find('\n')
        .map_or(src.len(), |index| offset.min(src.len()) + index + 1)
}

fn fresh_name(db: &SymbolDB, base: &str) -> String {
    if !db.defs.iter().any(|definition| definition.name == base) {
        return base.to_string();
    }
    for suffix in 2.. {
        let candidate = format!("{base}_{suffix}");
        if !db
            .defs
            .iter()
            .any(|definition| definition.name == candidate)
        {
            return candidate;
        }
    }
    unreachable!()
}

// ── Semantic tokens ───────────────────────────────────────────────────────────
//
// Token type indices (must match the legend in initialize_response).
mod st {
    pub const KEYWORD: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const VARIABLE: u32 = 3;
    pub const STRING: u32 = 7;
    pub const NUMBER: u32 = 8;
    pub const COMMENT: u32 = 9;
    pub const OPERATOR: u32 = 10;
    pub const OWNERSHIP: u32 = 12;
    pub const DECORATOR: u32 = 13;
}

// Modifier bitmasks
mod sm {
    pub const READONLY: u32 = 1 << 1;
    pub const MOVE: u32 = 1 << 2;
    pub const WRITE_BORROW: u32 = 1 << 3;
    pub const COPY: u32 = 1 << 4;
    pub const RULE: u32 = 1 << 5;
    pub const ARITHMETIC_CHECKED: u32 = 1 << 6;
    pub const ARITHMETIC_WRAPPING: u32 = 1 << 7;
    pub const ARITHMETIC_SATURATING: u32 = 1 << 8;
}

fn semantic_token_type_for(tokens: &[Token], idx: usize, src: &str) -> Option<(u32, u32)> {
    let tok = &tokens[idx];
    if let Some(marker_mod) = marker_modifier(tokens, idx) {
        return Some((st::DECORATOR, marker_mod));
    }

    match &tok.kind {
        TokKind::KwFn
        | TokKind::KwPub
        | TokKind::KwIf
        | TokKind::KwElse
        | TokKind::KwBreak
        | TokKind::KwReturn
        | TokKind::KwStruct
        | TokKind::KwEnum
        | TokKind::KwImpl
        | TokKind::KwTrait
        | TokKind::KwTag
        | TokKind::KwDerive
        | TokKind::KwConst
        | TokKind::KwComptime
        | TokKind::KwUse
        | TokKind::KwExtern
        | TokKind::KwLoop
        | TokKind::KwSelf
        | TokKind::KwNull
        | TokKind::KwIt
        | TokKind::KwModule => Some((st::KEYWORD, 0)),

        TokKind::KwTrue | TokKind::KwFalse => Some((st::KEYWORD, sm::READONLY)),

        TokKind::KwCopy => Some((st::OWNERSHIP, sm::COPY)),

        TokKind::KwMutate | TokKind::KwMove => None,

        TokKind::Ident(name) => {
            if name == Syntax::KW_NEXT && is_contextual_next(tokens, idx) {
                return Some((st::KEYWORD, 0));
            }
            if is_live_teaching_semantic_word(name) {
                return None;
            }
            // Classify identifiers by name convention:
            // PascalCase → type, everything else → variable
            if name.starts_with(|c: char| c.is_uppercase()) {
                Some((st::TYPE, 0))
            } else {
                Some((st::VARIABLE, 0))
            }
        }

        TokKind::Str(_) => Some((st::STRING, 0)),

        TokKind::Int(..) | TokKind::Float(..) | TokKind::Char(_) => Some((st::NUMBER, 0)),

        TokKind::LineComment(_) | TokKind::BlockComment(_) => Some((st::COMMENT, 0)),

        TokKind::Plus
        | TokKind::Minus
        | TokKind::Star
        | TokKind::Slash
        | TokKind::Percent
        | TokKind::Pipe
        | TokKind::Shl
        | TokKind::Shr
        | TokKind::AndAnd
        | TokKind::OrOr
        | TokKind::Bang
        | TokKind::EqEq
        | TokKind::NotEq
        | TokKind::Lt
        | TokKind::Gt
        | TokKind::Le
        | TokKind::Ge
        | TokKind::Compare
        | TokKind::Arrow
        | TokKind::UnifiedArrow
        | TokKind::LambdaArrow
        | TokKind::Question
        | TokKind::DotDot => Some((st::OPERATOR, 0)),
        TokKind::DotDotLt => Some((st::OPERATOR, 0)),

        TokKind::Amp if token_text(src, tok) == crate::Syntax::SIGIL_WRITE => {
            Some((st::OWNERSHIP, sm::WRITE_BORROW))
        }

        TokKind::Caret if token_text(src, tok) == crate::Syntax::SIGIL_MOVE => {
            Some((st::OWNERSHIP, sm::MOVE))
        }

        _ => None,
    }
}

fn marker_modifier(tokens: &[Token], idx: usize) -> Option<u32> {
    let tok = &tokens[idx];
    match tok.kind {
        TokKind::Hash => marker_kind_after(tokens, idx).map(|kind| kind.modifier()),
        _ => {
            let prev = previous_significant(tokens, idx)?;
            match tokens[prev].kind {
                TokKind::Hash if marker_name(tokens, idx).is_some() => {
                    marker_kind_for(&tokens[prev], tokens, idx).map(|kind| kind.modifier())
                }
                _ => None,
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MarkerKind {
    Rule,
}

impl MarkerKind {
    fn modifier(self) -> u32 {
        match self {
            MarkerKind::Rule => sm::RULE,
        }
    }
}

fn marker_kind_after(tokens: &[Token], idx: usize) -> Option<MarkerKind> {
    let next = next_significant(tokens, idx)?;
    marker_kind_for(&tokens[idx], tokens, next)
}

fn marker_kind_for(prefix: &Token, tokens: &[Token], name_idx: usize) -> Option<MarkerKind> {
    let name = marker_name(tokens, name_idx)?;
    match prefix.kind {
        TokKind::Hash if crate::Syntax::is_applied_rule(name) => Some(MarkerKind::Rule),
        _ => None,
    }
}

fn marker_name(tokens: &[Token], idx: usize) -> Option<&str> {
    match &tokens[idx].kind {
        TokKind::Ident(name) => Some(name.as_str()),
        _ => None,
    }
}

fn previous_significant(tokens: &[Token], idx: usize) -> Option<usize> {
    tokens[..idx]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, tok)| (!is_trivia(tok)).then_some(i))
}

fn next_significant(tokens: &[Token], idx: usize) -> Option<usize> {
    tokens
        .iter()
        .enumerate()
        .skip(idx + 1)
        .find_map(|(i, tok)| (!is_trivia(tok)).then_some(i))
}

fn is_contextual_next(tokens: &[Token], idx: usize) -> bool {
    let previous = previous_significant(tokens, idx).map(|idx| &tokens[idx].kind);
    let next_idx = next_significant(tokens, idx);
    let next = next_idx.map(|idx| &tokens[idx].kind);
    if matches!(previous, Some(TokKind::QuestionQuestion)) {
        return true;
    }
    if !matches!(previous, Some(TokKind::LBrace | TokKind::Semi)) {
        return false;
    }
    matches!(next, Some(TokKind::Semi | TokKind::RBrace))
}

fn is_trivia(tok: &Token) -> bool {
    matches!(tok.kind, TokKind::LineComment(_) | TokKind::BlockComment(_))
}

fn token_text<'a>(src: &'a str, tok: &Token) -> &'a str {
    src.get(tok.span.start..tok.span.end.min(src.len()))
        .unwrap_or("")
}

pub(crate) fn is_live_teaching_semantic_word(name: &str) -> bool {
    matches!(
        name,
        crate::Syntax::METHOD_VIEW
            | crate::Syntax::FOREIGN_PRIVATE
            | crate::Syntax::FOREIGN_UNSAFE
            | crate::Syntax::FOREIGN_NAMESPACE
            | crate::Syntax::FOREIGN_OWNED
            | crate::Syntax::FOREIGN_SANITIZER
            | crate::Syntax::FOREIGN_VEC
            | crate::Syntax::FOREIGN_DICT
            | crate::Syntax::FOREIGN_EPRINTLN
            | crate::Syntax::FOREIGN_OPEN
            | crate::Syntax::FOREIGN_GETENV
            | crate::Syntax::FOREIGN_OS
            | crate::Syntax::FOREIGN_ASYNC
            | crate::Syntax::FOREIGN_AWAIT
            | crate::Syntax::FOREIGN_MUTEX
            | crate::Syntax::FOREIGN_LOCK
    )
}

pub(crate) fn encode_semantic_tokens_with_arithmetic(
    tokens: &[Token],
    src: &str,
    arithmetic: &[jet_semindex::ArithmeticOperationFact],
) -> Vec<u32> {
    encode_semantic_tokens_where(tokens, src, |_| true, arithmetic)
}

pub(crate) fn encode_semantic_tokens_in_span_with_arithmetic(
    tokens: &[Token],
    src: &str,
    span: Span,
    arithmetic: &[jet_semindex::ArithmeticOperationFact],
) -> Vec<u32> {
    encode_semantic_tokens_where(
        tokens,
        src,
        |tok| tok.span.start >= span.start && tok.span.start < span.end,
        arithmetic,
    )
}

fn encode_semantic_tokens_where(
    tokens: &[Token],
    src: &str,
    include: impl Fn(&Token) -> bool,
    arithmetic: &[jet_semindex::ArithmeticOperationFact],
) -> Vec<u32> {
    let mut data: Vec<u32> = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for (idx, tok) in tokens.iter().enumerate() {
        if matches!(tok.kind, TokKind::Eof) {
            break;
        }
        if !include(tok) {
            continue;
        }
        let (tok_type, tok_mods) = match semantic_token_type_for(tokens, idx, src) {
            Some(t) => t,
            None => continue,
        };
        let tok_mods = tok_mods | arithmetic_modifier(&tok.span, arithmetic);
        let lsp_start = byte_offset_to_lsp(src, tok.span.start);
        let line = lsp_start.line;
        let start = lsp_start.character;

        // Compute length in UTF-16 code units
        let text = src
            .get(tok.span.start..tok.span.end.min(src.len()))
            .unwrap_or("");
        // For multi-line tokens (strings with newlines) just use first line
        let first_line_text: &str = text.split('\n').next().unwrap_or(text);
        let length = first_line_text.encode_utf16().count() as u32;
        if length == 0 {
            continue;
        }

        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            start - prev_start
        } else {
            start
        };

        data.push(delta_line);
        data.push(delta_start);
        data.push(length);
        data.push(tok_type);
        data.push(tok_mods);

        prev_line = line;
        prev_start = start;
    }
    data
}

fn arithmetic_modifier(token: &Span, arithmetic: &[jet_semindex::ArithmeticOperationFact]) -> u32 {
    arithmetic
        .iter()
        .filter(|fact| {
            token.start < fact.operation_span.end && fact.operation_span.start < token.end
        })
        .min_by_key(|fact| {
            fact.operation_span
                .end
                .saturating_sub(fact.operation_span.start)
        })
        .map_or(0, |fact| match fact.policy.as_str() {
            "Checked" => sm::ARITHMETIC_CHECKED,
            "Wrapping" => sm::ARITHMETIC_WRAPPING,
            "Saturating" => sm::ARITHMETIC_SATURATING,
            _ => 0,
        })
}

// ── Inlay hints ───────────────────────────────────────────────────────────────

pub(crate) fn format_inlay_hints(hints: &[&InlayHint], src: &str) -> String {
    let mut items = String::new();
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        // Position: at the semantic source anchor (binding name or argument expression).
        let pos = byte_offset_to_lsp(src, h.span.end);
        items.push_str(&format!(
            r#"{{"position":{{"line":{},"character":{}}},"label":"{}","kind":1}}"#,
            pos.line,
            pos.character,
            json_escape(&h.label)
        ));
    }
    format!("[{}]", items)
}
