//! D-META-ONE1=A: the marker registry is read from Prelude Jet source.
//!
//! `Prelude/Markers.jet` holds one `marker Name(params…)` declaration per rule
//! in the ratified D-META-FORM1=A form. This module turns that text into the
//! rows the parser, sema, formatter, LSP, highlighter, and explain tools read,
//! so no Rust file keeps a second copy of the vocabulary.
//!
//! This reader is deliberately small: it accepts the one shape the file is
//! written in and nothing else, because jet-foundation sits below the Jet
//! parser in the crate graph and cannot call it. `tests/marker_declarations.rs`
//! parses the same file with the real Jet parser and proves the two agree on
//! every name, parameter, and fact, so the shape can never drift apart.

use super::{
    AppliedRule, CompanionSite, PolicyScope, RuleArgType, RuleParam, RuleResolution,
    RuleSignature, RuleSite, RuleStatus,
};

/// The one authority for the marker vocabulary.
pub const MARKER_SOURCE: &str = include_str!("../../../jet-codegen/src/Prelude/Markers.jet");

/// Read every `marker` declaration in `MARKER_SOURCE`.
///
/// A malformed row is a bug in a file that ships inside the compiler, so it
/// panics with the offending line rather than degrading to a partial registry.
pub fn read() -> Vec<AppliedRule> {
    MARKER_SOURCE
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("marker "))
        .map(declaration)
        .collect()
}

fn declaration(line: &str) -> AppliedRule {
    let rest = line["marker ".len()..].trim();
    let open = rest
        .find('(')
        .unwrap_or_else(|| panic!("marker declaration without a parameter list: {line}"));
    let close = rest
        .rfind(')')
        .unwrap_or_else(|| panic!("marker declaration without a closing `)`: {line}"));
    let name = leak(rest[..open].trim());

    let mut params: Vec<RuleParam> = Vec::new();
    let mut variadic: Option<(RuleArgType, &'static str)> = None;
    let mut sites: Vec<RuleSite> = Vec::new();
    let mut scopes: Vec<PolicyScope> = Vec::new();
    let mut repeatable = false;
    let mut owns_menu = false;
    let mut inherits = false;
    let mut resolution = RuleResolution::SiteBound;
    let mut companion_site = None;
    let mut status = RuleStatus::Active;

    for entry in split_top_level(&rest[open + 1..close]) {
        let (label, value) = entry
            .split_once(':')
            .unwrap_or_else(|| panic!("marker parameter without `:` in {line}: {entry}"));
        let (label, value) = (label.trim(), value.trim());
        if let Some(fact) = label.strip_prefix(crate::Syntax::COMPTIME_MARK) {
            match fact {
                "sites" => sites = list(value).iter().map(|item| site(item, line)).collect(),
                "scopes" => scopes = list(value).iter().map(|item| scope(item, line)).collect(),
                "repeatable" => repeatable = flag(value, line),
                "owns_menu" => owns_menu = flag(value, line),
                "inherits" => inherits = flag(value, line),
                "resolution" => resolution = rule_resolution(value, line),
                "companion" => {
                    let parts = list(value);
                    assert_eq!(parts.len(), 2, "`@companion` reads `[Rule, .Site]` in {line}");
                    companion_site = Some(CompanionSite {
                        rule: leak(parts[0]),
                        site: site(parts[1], line),
                    });
                }
                "retired" => {
                    status = RuleStatus::Retired {
                        replacement: leak(&unquote(value, line)),
                    }
                }
                other => panic!("unknown marker fact `${other}` in {line}"),
            }
            continue;
        }
        let (type_text, default) = match split_default(value) {
            Some((type_text, default)) => (type_text, Some(leak(default))),
            None => (value, None),
        };
        if let Some(element) = type_text.strip_prefix("...") {
            let element = element.trim();
            variadic = Some((arg_type(element), leak(element)));
            continue;
        }
        params.push(RuleParam {
            name: leak(label),
            ty: arg_type(type_text),
            source_type: leak(type_text),
            default,
        });
    }

    AppliedRule {
        name,
        signature: RuleSignature {
            params: leak_slice(params),
            variadic: variadic.map(|(ty, _)| ty),
            variadic_source_type: variadic.map(|(_, source_type)| source_type),
        },
        policy_scopes: leak_slice(scopes),
        sites: leak_slice(sites),
        repeatable,
        owns_menu,
        companion_site,
        status,
        inherits,
        resolution,
    }
}

/// D-RULEARG-TYPES1=A: a parameter's written type names either one of the seven
/// argument shapes the binder knows or a closed menu published in `core.lang`.
/// A menu name is written as a bare identifier, so anything that is not one of
/// the remaining six reads as an identifier from that menu.
///
/// `PolicySetting` is the one menu whose entries are written `key = value`
/// rather than as a bare name, so it binds as a free value.
fn arg_type(source_type: &str) -> RuleArgType {
    match source_type {
        "Value" | "PolicySetting" => RuleArgType::Any,
        "String" => RuleArgType::String,
        "Ident" => RuleArgType::Ident,
        "Bool" => RuleArgType::Bool,
        "Int" => RuleArgType::Int,
        "Duration | String" => RuleArgType::DurationOrString,
        "[Effect]" => RuleArgType::EffectRoots,
        _ => RuleArgType::Ident,
    }
}

/// Split `Type = default` on the one `=` that is not part of the type. A `=`
/// inside a bracket, paren, or string belongs to the value it sits in.
fn split_default(value: &str) -> Option<(&str, &str)> {
    let bytes = value.as_bytes();
    let mut scan = Scan::new();
    for (index, byte) in bytes.iter().enumerate() {
        if scan.step(*byte) && *byte == b'=' {
            return Some((value[..index].trim(), value[index + 1..].trim()));
        }
    }
    None
}

/// Comma-separated entries at nesting depth zero, outside string literals.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = 0usize;
    let mut scan = Scan::new();
    for (index, byte) in text.as_bytes().iter().enumerate() {
        if scan.step(*byte) && *byte == b',' {
            entries.push(text[start..index].trim());
            start = index + 1;
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        entries.push(last);
    }
    entries
}

/// Tracks bracket depth and string literals so a separator inside either is
/// never mistaken for a top-level one.
struct Scan {
    depth: i32,
    in_string: bool,
    escaped: bool,
}

impl Scan {
    fn new() -> Self {
        Self { depth: 0, in_string: false, escaped: false }
    }

    /// True when this byte sits at top level and outside a string.
    fn step(&mut self, byte: u8) -> bool {
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
            }
            return false;
        }
        match byte {
            b'"' => {
                self.in_string = true;
                false
            }
            b'[' | b'(' | b'{' => {
                self.depth += 1;
                false
            }
            b']' | b')' | b'}' => {
                self.depth -= 1;
                false
            }
            _ => self.depth == 0,
        }
    }
}

/// The entries of a `[…]` value.
fn list(value: &str) -> Vec<&str> {
    let inner = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or_else(|| panic!("expected a `[…]` list, found `{value}`"));
    split_top_level(inner)
}

fn flag(value: &str, line: &str) -> bool {
    match value {
        "true" => true,
        "false" => false,
        other => panic!("a marker fact reads `true` or `false`, found `{other}` in {line}"),
    }
}

fn unquote(value: &str, line: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("expected a quoted value, found `{value}` in {line}"));
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for character in inner.chars() {
        match (escaped, character) {
            (true, other) => {
                out.push(other);
                escaped = false;
            }
            (false, '\\') => escaped = true,
            (false, other) => out.push(other),
        }
    }
    out
}

fn site(written: &str, line: &str) -> RuleSite {
    let name = written.strip_prefix('.').unwrap_or(written);
    RuleSite::ALL
        .into_iter()
        .find(|site| site.name() == name)
        .unwrap_or_else(|| panic!("`{written}` is not a marker site, in {line}"))
}

fn scope(written: &str, line: &str) -> PolicyScope {
    match written.strip_prefix('.').unwrap_or(written) {
        "Organization" => PolicyScope::Organization,
        "Package" => PolicyScope::Package,
        "Module" => PolicyScope::Module,
        "Function" => PolicyScope::Function,
        "Block" => PolicyScope::Block,
        other => panic!("`{other}` is not a policy scope, in {line}"),
    }
}

fn rule_resolution(written: &str, line: &str) -> RuleResolution {
    match written.strip_prefix('.').unwrap_or(written) {
        "SiteBound" => RuleResolution::SiteBound,
        "Override" => RuleResolution::Override,
        "Merge" => RuleResolution::Merge,
        "Tighten" => RuleResolution::Tighten,
        other => panic!("`{other}` is not a resolution, in {line}"),
    }
}

// ponytail: the registry is read once and lives for the whole process, so the
// rows are leaked rather than threaded through every reader as a lifetime.
fn leak(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

fn leak_slice<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reader has to read what the file actually says, not just parse it.
    #[test]
    fn a_row_carries_its_parameters_and_its_facts() {
        let rows = read();
        let unsafe_row = rows.iter().find(|row| row.name == "Unsafe").expect("#Unsafe");
        assert_eq!(unsafe_row.signature.params.len(), 2);
        assert_eq!(unsafe_row.signature.params[0].name, "reason");
        assert_eq!(unsafe_row.signature.params[0].ty, RuleArgType::String);
        assert_eq!(unsafe_row.signature.params[1].source_type, "ObligationMode");
        assert_eq!(unsafe_row.signature.params[1].default, Some(".None"));
        assert!(unsafe_row.sites.contains(&RuleSite::Operation));

        let caps = rows.iter().find(|row| row.name == "Caps").expect("#Caps");
        assert_eq!(caps.signature.variadic, Some(RuleArgType::Ident));
        assert_eq!(caps.signature.variadic_source_type, Some("Capability"));

        let pre = rows.iter().find(|row| row.name == "Pre").expect("#Pre");
        assert!(pre.repeatable);

        let doc = rows.iter().find(|row| row.name == "Doc").expect("#Doc");
        let companion = doc.companion_site.expect("#Doc rides #Job");
        assert_eq!(companion.rule, "Job");
        assert_eq!(companion.site, RuleSite::Function);

        let policy = rows.iter().find(|row| row.name == "Policy").expect("#Policy");
        assert!(policy.inherits);
        assert_eq!(policy.resolution, RuleResolution::Tighten);
        assert!(policy.policy_scopes.contains(&PolicyScope::Module));
    }

    /// A retired row keeps the replacement text, escapes and all.
    #[test]
    fn a_retired_row_keeps_its_replacement() {
        let rows = read();
        let suppress = rows.iter().find(|row| row.name == "Suppress").expect("#Suppress");
        assert_eq!(
            suppress.status,
            RuleStatus::Retired { replacement: ".drop(\"reason\")" }
        );
        let known = rows.iter().find(|row| row.name == "Known").expect("#Known");
        assert!(known.sites.is_empty());
    }

    /// A comma inside a list, a paren, or a string never splits an entry.
    #[test]
    fn separators_inside_brackets_and_strings_are_not_separators() {
        assert_eq!(split_top_level("a: [x, y], b: \"p, q\""), vec!["a: [x, y]", "b: \"p, q\""]);
        assert_eq!(split_default("String = \"a = b\""), Some(("String", "\"a = b\"")));
        assert_eq!(split_default("Duration | String"), None);
    }
}
