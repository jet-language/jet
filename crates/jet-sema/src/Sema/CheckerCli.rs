//! D-CLIFLAG1 (c7cliflag): typed entry-signature CLI parsing.
//!
//! `@[Cli]` is a sibling derive of `@[Codable]` on the same marker/derive
//! infrastructure (D-MARKERMOVE1/D-CLIFLAG1). A `@[Cli]`-derived struct's
//! fields each map to one `core.args` registration; see docs/spec/spec.md
//! ("Typed entry-signature CLI parsing (D-CLIFLAG1)") for the pinned
//! field-mapping rule this validates. Sema does the shape-checking here so
//! `Codegen/Items.rs::emit_struct_cli` can emit unconditionally (I3: sema
//! decides, codegen never re-derives "is this field supported").
//!
//! Also validates `enum Cmd { Variant(Payload) … }` subcommand parameters
//! (E1307) and the `fn run` entry-parameter shape (E1308, invoked from
//! `Bundle.rs`'s entry-point check next to the existing `run` checks).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{Field, Item, Type};

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
/// parameters, S61, a different grammar slot; struct fields, Cli or not, use
/// `#[Default(...)]`)?
fn has_default_marker(f: &Field) -> bool {
    f.serde_markers
        .iter()
        .any(|m| m.name == Syntax::ATTR_DEFAULT)
}

/// D-CLIFLAG1: classify one `@[Cli]` struct field for `core.args` codegen.
/// `None` means the field's type has no flag mapping (E1305).
pub(crate) enum CliFieldKind {
    /// `Bool` field -> `.flag(name, help)`, default `false`.
    Flag,
    /// `T?` field (T a supported scalar) -> optional `.option(...)`, default `null`.
    OptionalOption,
    /// A supported scalar field with `#[Default(expr)]` -> optional `.option(...)`,
    /// default = the marker's expression.
    DefaultedOption,
    /// A supported scalar field, no `Option`/`Default` -> required `.option(...)`;
    /// absent at runtime is a `core.args`-style parse error (no new diagnostic code).
    RequiredOption,
}

pub(crate) fn classify_cli_field(f: &Field) -> Option<CliFieldKind> {
    match &f.ty {
        Type::Bool => Some(CliFieldKind::Flag),
        Type::Option(inner) if is_cli_scalar(inner) => Some(CliFieldKind::OptionalOption),
        ty if is_cli_scalar(ty) => {
            if has_default_marker(f) {
                Some(CliFieldKind::DefaultedOption)
            } else {
                Some(CliFieldKind::RequiredOption)
            }
        }
        _ => None,
    }
}

/// E1305: a `@[Cli]` struct field's type has no flag mapping.
fn e1305(field_name: &str, ty_show: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1305",
        format!(
            "field `{}` has no CLI flag mapping ({})",
            field_name, ty_show
        ),
        "a `@[Cli]` field becomes one `--flag`; only `Int`, `Float`, `Bool`, `String`, `Path`, \
         and `T?` of those have a defined flag shape (nested `@[Cli]` structs and other \
         collection/closure types don't)."
            .to_string(),
        "change the field to a supported type, or drop it from the `@[Cli]` struct".to_string(),
        Some(span),
    )
}

/// E1306: two fields (or a field and the built-in `--help`) would derive the
/// same flag name.
fn e1306(flag: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1306",
        format!("two `@[Cli]` fields both derive the flag `--{}`", flag),
        "every field needs a distinct flag name; `--help` is also reserved (every generated \
         CLI gets one automatically)."
            .to_string(),
        "rename one of the fields".to_string(),
        Some(span),
    )
}

/// E1307: an `enum` used as a `fn run` subcommand parameter has a variant
/// whose payload isn't a `@[Cli]`-derived struct.
pub(crate) fn e1307(variant_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1307",
        format!(
            "subcommand variant `{}` doesn't carry a `@[Cli]` struct",
            variant_name
        ),
        "each subcommand variant's payload is a single `@[Cli]`-derived struct — that struct \
         is where the subcommand's own flags come from."
            .to_string(),
        format!(
            "give `{}` a single `@[Cli]` struct payload, e.g. `{}(SomeArgs)`",
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
        "a typed `fn run(args: T)` entry only works when `T` is a `@[Cli]`-derived struct, or \
         an `enum` whose every variant carries a `@[Cli]` struct payload."
            .to_string(),
        "mark the struct `@[Cli]`, or give the enum's variants `@[Cli]` struct payloads"
            .to_string(),
        span,
    )
}

/// D-CLIFLAG1: validate every `@[Cli]`-derived struct in `items` (E1305/E1306).
/// Mirrors `validate_serde_items`'s shape (same call site in `Bundle.rs`, same
/// "runs after the trait registry is built" timing) but for the CLI-derive
/// plane instead of the wire-serde plane.
pub(crate) fn validate_cli_items(items: &[Item], reg: &TraitRegistry) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for item in items {
        let Item::Struct(s) = item else { continue };
        if !s.derives.iter().any(|(t, _)| t == "Cli") {
            continue;
        }
        let _ = reg; // shape validation here doesn't need the registry (no nested-Cli lookups in v1)
        let mut seen_flags: std::collections::HashMap<String, Span> =
            std::collections::HashMap::new();
        seen_flags.insert("help".to_string(), s.name_span);
        for f in &s.fields {
            if classify_cli_field(f).is_none() {
                out.push(e1305(&f.name, &f.ty.show(), f.name_span));
                continue;
            }
            let flag = cli_flag_name(&f.name);
            if let Some(_prev) = seen_flags.insert(flag.clone(), f.name_span) {
                out.push(e1306(&flag, f.name_span));
            }
        }
    }
    out
}
