//! D-CLIFLAG1 (c7cliflag): typed entry-signature CLI parsing.
//!
//! `#[CLI]` is a sibling derive of `#[Codable]` on the same marker/derive
//! infrastructure (D-MARKERMOVE1/D-CLIFLAG1). A `#[CLI]`-derived struct's
//! fields each map to one `core.args` registration; see docs/spec/spec.md
//! ("Typed entry-signature CLI parsing (D-CLIFLAG1)") for the pinned
//! field-mapping rule this validates. Sema does the shape-checking here so
//! `Codegen/Items.rs::emit_struct_cli` can emit unconditionally (I3: sema
//! decides, codegen never re-derives "is this field supported").
//!
//! D-CLI-POS1=A: required value fields also fill from bare argv in declaration
//! order unless `#[Flag]` opts them out. Bool / optional / defaulted stay
//! flag-only; every field still accepts `--field`, and named input wins.
//!
//! Also validates `enum Cmd { Variant(Payload) … }` subcommand parameters
//! (E1307) and the `fn run` entry-parameter shape (E1308, invoked from
//! `Bundle.rs`'s entry-point check next to the existing `run` checks).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{Expr, Field, Item, StrPart, Type};

/// D-CLIFLAG1: the dashed `--flag` name for a snake_case Jet field name.
/// `config_file` -> `config-file`. Pure textual transform, no rename markers
/// in v1 (I8: one mapping, not a menu of casing styles like D-SERDE3's
/// `RenameAll` — that's a wire-format concern, not a CLI-flag concern).
pub(crate) fn cli_flag_name(field_name: &str) -> String {
    field_name.replace('_', "-")
}

/// D-CLIFLAG1: is `ty` one of the scalar types a CLI flag can hold —
/// `Int`/`Float`/`Bool`/`String`/`Path`? (`Path` is `Type::Named("Path")`,
/// the stdlib path type, not a user struct.)
fn is_cli_scalar(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Float | Type::Bool | Type::String)
        || matches!(ty, Type::Named(n) if n == "Path")
}

/// D-CLIFLAG1: does `f` carry a `#[Default(expr)]` marker (D-SERDE5's
/// existing field-default mechanism, reused here rather than inventing a
/// second one — Jet's only *inline* `= expr` default lives on function
/// parameters, S61, a different grammar slot; struct fields, CLI or not, use
/// `#[Default(...)]`)?
fn has_default_marker(f: &Field) -> bool {
    f.default.is_some()
        || f.serde_markers
            .iter()
            .any(|m| m.name == Syntax::ATTR_DEFAULT)
}

/// D-CLI-POS1=A: does `f` carry `#[Flag]` (opt out of positional filling)?
fn has_flag_marker(f: &Field) -> bool {
    f.serde_markers
        .iter()
        .any(|m| m.name == Syntax::CONTRACT_FLAG)
}

fn marker_string<'a>(
    f: &'a Field,
    name: &str,
) -> Option<(&'a crate::AST::Marker, String)> {
    let marker = f.serde_markers.iter().find(|marker| marker.name == name)?;
    match marker.args.first() {
        Some(Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(value) => Some((marker, value.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// D-CLIFLAG1: classify one `#[CLI]` struct field for `core.args` codegen.
/// `None` means the field's type has no flag mapping (E1305).
pub(crate) enum CLIFieldKind {
    /// `Bool` field -> `.flag(name, help)`, default `false`.
    Flag,
    /// `T?` field (T a supported scalar) -> optional `.option(...)`, default `null`.
    OptionalOption,
    /// A supported scalar field with `#[Default(expr)]` -> optional `.option(...)`,
    /// default = the marker's expression.
    DefaultedOption,
    /// A supported scalar field, no `Option`/`Default` -> required value; absent
    /// at runtime is a `core.args`-style parse error (no new diagnostic code).
    /// D-CLI-POS1=A: also fills from a bare positional unless `#[Flag]`.
    RequiredOption,
}

pub(crate) fn classify_cli_field(f: &Field) -> Option<CLIFieldKind> {
    match &f.ty {
        Type::Bool => Some(CLIFieldKind::Flag),
        Type::Option(inner) if is_cli_scalar(inner) => Some(CLIFieldKind::OptionalOption),
        ty if is_cli_scalar(ty) => {
            if has_default_marker(f) {
                Some(CLIFieldKind::DefaultedOption)
            } else {
                Some(CLIFieldKind::RequiredOption)
            }
        }
        _ => None,
    }
}

/// E1305: a `#[CLI]` struct field's type has no flag mapping.
fn e1305(field_name: &str, ty_show: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1305",
        format!(
            "field `{}` has no CLI flag mapping ({})",
            field_name, ty_show
        ),
        "a `#[CLI]` field becomes one `--flag` (and a positional when required); only `Int`, \
         `Float`, `Bool`, `String`, `Path`, and `T?` of those have a defined shape (nested \
         `#[CLI]` structs and other collection/closure types don't)."
            .to_string(),
        "change the field to a supported type, or drop it from the `#[CLI]` struct".to_string(),
        Some(span),
    )
}

/// E1306: two fields (or a field and the built-in `--help`) would derive the
/// same flag name.
fn e1306(flag: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1306",
        format!("two `#[CLI]` fields both derive the flag `--{}`", flag),
        "every field needs a distinct flag name; `--help` is also reserved (every generated \
         CLI gets one automatically)."
            .to_string(),
        "rename one of the fields".to_string(),
        Some(span),
    )
}

/// E1309: `#[Flag]` on a field that is already flag-only (D-CLI-POS1=A).
fn e1309(field_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1309",
        format!(
            "`#[Flag]` on `{}` has nothing to opt out of",
            field_name
        ),
        "`#[Flag]` keeps a required value field flag-only. Bool fields, optional fields \
         (`T?`), and fields with `#[Default(...)]` already stay flag-only."
            .to_string(),
        "remove `#[Flag]`, or make the field a required scalar without `#[Default]`"
            .to_string(),
        Some(span),
    )
}

/// E1318: two `#[CLI]` fields claim the same `#Short` spelling.
fn e1318(short: &str, first: &str, second: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1318",
        format!(
            "`#Short(\"{short}\")` is used by both `{first}` and `{second}`"
        ),
        "each short option must select only one `#CLI` field.".to_string(),
        format!("give `{first}` or `{second}` a different `#Short` value"),
        Some(span),
    )
}

fn e1318_invalid(short: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1318",
        format!("`#Short(\"{short}\")` is not a one-letter option"),
        "the shared command parser treats a short option as one ASCII letter.".to_string(),
        "use one letter, such as `#Short(\"p\")`".to_string(),
        Some(span),
    )
}

/// E1319: a typed-CLI-only field marker has no builder mapping here.
fn e1319(marker: &str, field: &str, why: &str, fix: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1319",
        format!("`#{marker}` has no CLI mapping for field `{field}`"),
        why.to_string(),
        fix.to_string(),
        Some(span),
    )
}

/// E1307: an `enum` used as a `fn run` subcommand parameter has a variant
/// whose payload isn't a `#[CLI]`-derived struct.
pub(crate) fn e1307(variant_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1307",
        format!(
            "subcommand variant `{}` doesn't carry a `#[CLI]` struct",
            variant_name
        ),
        "each subcommand variant's payload is a single `#[CLI]`-derived struct — that struct \
         is where the subcommand's own flags come from."
            .to_string(),
        format!(
            "give `{}` a single `#[CLI]` struct payload, e.g. `{}(SomeArgs)`",
            variant_name, variant_name
        ),
        Some(span),
    )
}

/// E1308: `fn run`'s parameter isn't a shape D-CLIFLAG1 recognizes.
pub(crate) fn e1308(span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E1308",
        "`run`'s parameter isn't a CLI-derived type".to_string(),
        "a typed `fn run(args: T)` entry only works when `T` is a `#[CLI]`-derived struct, or \
         an `enum` whose every variant carries a `#[CLI]` struct payload."
            .to_string(),
        "mark the struct `#[CLI]`, or give the enum's variants `#[CLI]` struct payloads"
            .to_string(),
        span,
    )
}

/// D-CLIFLAG1 / D-CLI-POS1: validate every `#[CLI]`-derived struct in `items`
/// (E1305/E1306/E1309). Mirrors `validate_serde_items`'s shape (same call site
/// in `Bundle.rs`) but for the CLI-derive plane instead of the wire-serde plane.
pub(crate) fn validate_cli_items(items: &[Item], reg: &TraitRegistry) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for item in items {
        let Item::Struct(s) = item else { continue };
        if !s.derives.iter().any(|(t, _)| t == "CLI") {
            for field in &s.fields {
                for marker in &field.serde_markers {
                    if matches!(
                        marker.name.as_str(),
                        Syntax::CONTRACT_SHORT | Syntax::CONTRACT_ENV
                    ) {
                        out.push(e1319(
                            &marker.name,
                            &field.name,
                            "`#Short` and `#Env` describe generated command inputs, but this struct is not `#CLI`.",
                            "remove the marker, or mark the command-input struct `#CLI`",
                            marker.name_span,
                        ));
                    }
                }
            }
            continue;
        }
        let _ = reg; // shape validation here doesn't need the registry (no nested-CLI lookups in v1)
        let mut seen_flags: std::collections::HashMap<String, Span> =
            std::collections::HashMap::new();
        let mut seen_shorts: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        seen_flags.insert("help".to_string(), s.name_span);
        for f in &s.fields {
            let kind = classify_cli_field(f);
            if kind.is_none() {
                out.push(e1305(&f.name, &f.ty.show(), f.name_span));
                continue;
            }
            if has_flag_marker(f) && !matches!(kind, Some(CLIFieldKind::RequiredOption)) {
                let span = f
                    .serde_markers
                    .iter()
                    .find(|m| m.name == Syntax::CONTRACT_FLAG)
                    .map(|m| m.name_span)
                    .unwrap_or(f.name_span);
                out.push(e1309(&f.name, span));
            }
            let flag = cli_flag_name(&f.name);
            if let Some(_prev) = seen_flags.insert(flag.clone(), f.name_span) {
                out.push(e1306(&flag, f.name_span));
            }
            if let Some((marker, short)) = marker_string(f, Syntax::CONTRACT_SHORT) {
                if short.len() != 1 || !short.as_bytes()[0].is_ascii_alphabetic() {
                    out.push(e1318_invalid(&short, marker.name_span));
                } else if let Some(first) = seen_shorts.insert(short.clone(), f.name.clone()) {
                    out.push(e1318(&short, &first, &f.name, marker.name_span));
                }
            }
            if matches!(kind, Some(CLIFieldKind::Flag)) {
                if let Some((marker, _)) = marker_string(f, Syntax::CONTRACT_ENV) {
                    out.push(e1319(
                        "Env",
                        &f.name,
                        "a `Bool` command field is a presence flag, while `#Env` lowers to a value option.",
                        "remove `#Env`, or use it on an `Int`, `Float`, `String`, `Path`, or optional value field",
                        marker.name_span,
                    ));
                }
            }
        }
    }
    out
}
