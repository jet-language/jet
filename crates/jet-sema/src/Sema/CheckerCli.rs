//! D-CLIFLAG1 (c7cliflag): typed entry-signature CLI parsing.
//!
//! `#[CLI]` is a sibling derive of `#[Codable]` on the same marker/derive
//! infrastructure (D-VERDICT-732-1, formerly D-MARKERMOVE1; D-CLIFLAG1).
//! A `#[CLI]`-derived struct's
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
//! Also validates program-struct callable members and the `fn run` entry-
//! parameter shape (E1308, invoked from the entry-point check beside the
//! existing `run` checks).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{AccessConvention, Expr, Field, Func, Item, StrPart, Type};

/// D-CLIFLAG1: the dashed `--flag` name for a snake_case Jet field name.
/// `config_file` -> `config-file`. Pure textual transform, no rename markers
/// in v1 (I8: one mapping, not a menu of casing styles like D-SERDE3's
/// `RenameAll` — that's a wire-format concern, not a CLI-flag concern).
pub(crate) fn cli_flag_name(field_name: &str) -> String {
    field_name.replace('_', "-")
}

/// D-CLIFLAG1: is `ty` one of the scalar types a CLI flag can hold —
/// `Int` (including an inline range)/`Float`/`Bool`/`String`/`Path`? (`Path` is `Type::Named("Path")`,
/// the stdlib path type, not a user struct.)
fn is_cli_scalar(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::InlineRange { .. } | Type::Float | Type::Bool | Type::String
    )
        || matches!(ty, Type::Named(n) if n == "Path")
}

/// D-FIELDDEF1: does `f` carry the declaration-owned `= expr` field default?
/// The retired `#Default` marker remains visible only to the diagnostic path.
fn has_default_marker(f: &Field) -> bool {
    f.default.is_some()
        || f.serde_markers
            .iter()
            .any(|m| m.name == Syntax::MARKER_DEFAULT)
}

/// D-CLI-POS1=A: does `f` carry `#[Flag]` (opt out of positional filling)?
fn has_flag_marker(f: &Field) -> bool {
    f.serde_markers
        .iter()
        .any(|m| m.name == Syntax::MARKER_FLAG)
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
    /// A supported scalar field with an inline `= expr` -> optional `.option(...)`,
    /// default = the declaration's expression.
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
        "a `#[CLI]` field becomes one `--flag` (and a positional when required); only `Int` \
         (including inline ranges), `Float`, `Bool`, `String`, `Path`, and `T?` of those have a defined shape (nested \
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
         (`T?`), and fields with a `= expr` default already stay flag-only."
            .to_string(),
        "remove `#[Flag]`, or make the field a required scalar without a `= expr` default"
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

/// E1308: `fn run`'s parameter isn't a shape D-CLIFLAG1 recognizes.
pub(crate) fn e1308(span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E1308",
        "`run`'s parameter isn't a CLI-derived type".to_string(),
        "a typed `fn run(args: T)` entry only works when `T` is a `#[CLI]`-derived program struct."
            .to_string(),
        "mark the program struct `#[CLI]` (or `#CLI(Standard)` for the standard pack)"
            .to_string(),
        span,
    )
}

fn e1344(command: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1344",
        format!("CLI command `{command}` collides with another command or root flag"),
        "a program struct has one command namespace and one root flag namespace; a word cannot name both."
            .to_string(),
        "rename the command or the root field so every command word is unique".to_string(),
        Some(span),
    )
}

fn e1345(name: &str, span: Span, why: &str) -> Diagnostic {
    Diagnostic::error(
        "E1345",
        format!("CLI command member `{name}` has no callable shape"),
        why.to_string(),
        "bind a visible function with scalar parameters and, at most, one first read-only parameter of the program-struct type".to_string(),
        Some(span),
    )
}

fn e1346(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1346",
        "callable CLI members cannot be declared on a Codable struct".to_string(),
        "CLI commands are behavior, while Codable structs contain only serializable data fields."
            .to_string(),
        "split the command program struct from the Codable data struct".to_string(),
        Some(span),
    )
}

fn e1347(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1347",
        "a CLI method receiver must be read-only".to_string(),
        "shared root flags are parsed into one program value and command dispatch must not mutate that value."
            .to_string(),
        "change the receiver to read-only `self`".to_string(),
        Some(span),
    )
}

fn callable_param_shape(ty: &Type) -> bool {
    is_cli_scalar(ty) || matches!(ty, Type::Option(inner) if is_cli_scalar(inner))
}

fn validate_callable_params(
    function: &Func,
    command_name: &str,
    shared_type: Option<&str>,
    out: &mut Vec<Diagnostic>,
) {
    let shared = shared_type.map(|shared_type| {
        function
            .params
            .iter()
            .enumerate()
            .filter(|(_, param)| named_leaf(&param.ty) == Some(shared_type))
            .collect::<Vec<_>>()
    });
    if let Some(shared) = shared.as_ref() {
        if shared.len() > 1 || shared.first().is_some_and(|(index, _)| *index != 0) {
            out.push(e1345(
                command_name,
                function.name_span,
                "a bound function may receive the shared program struct only once, as its first parameter",
            ));
        }
    }
    for (index, param) in function.params.iter().enumerate() {
        if param.name == Syntax::KW_SELF {
            if shared_type.is_some() {
                out.push(e1345(
                    command_name,
                    param.name_span,
                    "a bound function receives shared state as an ordinary first program-struct parameter, not as a method receiver",
                ));
            }
            continue;
        }
        if shared
            .as_ref()
            .is_some_and(|params| params.iter().any(|(shared_index, _)| *shared_index == index))
        {
            if param.convention != AccessConvention::Read {
                out.push(e1345(
                    command_name,
                    param.name_span,
                    "the shared program-struct parameter must be read-only",
                ));
            }
            continue;
        }
        if param.variadic || param.variadic_bound_list.is_some() {
            out.push(e1345(
                command_name,
                param.name_span,
                "command parameters cannot be variadic; use one scalar or optional scalar parameter per input",
            ));
            continue;
        }
        if !callable_param_shape(&param.ty) {
            out.push(e1345(
                command_name,
                param.ty_span,
                "every command parameter must be a CLI scalar or optional scalar",
            ));
        }
        if param.default.is_some()
            && !param.default.as_deref().is_some_and(|expr| {
                matches!(
                    expr.without_parens(),
                    Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Str(..)
                )
            })
        {
            out.push(e1345(
                command_name,
                param.name_span,
                "a command default must be a CLI compile-time scalar value",
            ));
        }
    }
}

fn named_leaf(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(name) => Some(name.rsplit('.').next().unwrap_or(name)),
        _ => None,
    }
}

fn has_codable_shape(s: &crate::AST::StructDef) -> bool {
    s.derives.iter().any(|(name, _)| {
        matches!(name.as_str(), Syntax::MARKER_CODABLE | "Encode" | "Decode")
    })
}

fn binding_target<'a>(
    items: &'a [Item],
    binding: &crate::AST::CLICommandBinding,
) -> Option<&'a Func> {
    let Expr::Ident(name, _) = binding.target.without_parens() else {
        return None;
    };
    items.iter().find_map(|item| match item {
        Item::Func(function) if function.name == *name => Some(function),
        _ => None,
    })
}

/// D-CLIFLAG1 / D-CLI-GLOBAL1=E / D-CLI-POS1: validate every `#[CLI]`-
/// derived program struct in `items` (E1305/E1306/E1308/E1309/E1344-E1347).
/// This is the CLI-derive plane, not the wire-serde plane.
pub(crate) fn validate_cli_items(items: &[Item], reg: &TraitRegistry) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for item in items {
        let Item::Struct(s) = item else { continue };
        if !s.derives.iter().any(|(t, _)| t == "CLI") {
            for binding in &s.cli_bindings {
                out.push(e1345(
                    &binding.name,
                    binding.name_span,
                    "a callable member is only valid inside a `#CLI` program struct",
                ));
            }
            for field in &s.fields {
                for marker in &field.serde_markers {
                    if matches!(
                        marker.name.as_str(),
                        Syntax::MARKER_SHORT | Syntax::MARKER_ENV
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
        let standard = s.type_markers.iter().any(|marker| {
            marker.name == Syntax::MARKER_CLI
                && marker.args.iter().any(|arg| {
                    matches!(arg, Expr::Ident(name, _) if name == "Standard")
                })
        });
        if standard {
            for name in ["verbose", "quiet", "color", "version"] {
                seen_flags.insert(name.to_string(), s.name_span);
            }
            seen_shorts.insert("v".to_string(), "standard --verbose".to_string());
            seen_shorts.insert("q".to_string(), "standard --quiet".to_string());
        }
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
                    .find(|m| m.name == Syntax::MARKER_FLAG)
                    .map(|m| m.name_span)
                    .unwrap_or(f.name_span);
                out.push(e1309(&f.name, span));
            }
            let flag = cli_flag_name(&f.name);
            if let Some(_prev) = seen_flags.insert(flag.clone(), f.name_span) {
                out.push(e1306(&flag, f.name_span));
            }
            if let Some((marker, short)) = marker_string(f, Syntax::MARKER_SHORT) {
                if short.len() != 1 || !short.as_bytes()[0].is_ascii_alphabetic() {
                    out.push(e1318_invalid(&short, marker.name_span));
                } else if let Some(first) = seen_shorts.insert(short.clone(), f.name.clone()) {
                    out.push(e1318(&short, &first, &f.name, marker.name_span));
                }
            }
            if matches!(kind, Some(CLIFieldKind::Flag)) {
                if let Some((marker, _)) = marker_string(f, Syntax::MARKER_ENV) {
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
        let mut seen_commands: std::collections::HashMap<String, Span> =
            std::collections::HashMap::new();
        let mut command_count = 0usize;
        for function in &s.methods {
            if s.fields.iter().any(|field| {
                field.computed.is_some() && field.name == function.name
            }) {
                continue;
            }
            command_count += 1;
            let command = function.name.to_lowercase();
            if seen_flags.contains_key(&command)
                || seen_commands
                    .insert(command.clone(), function.name_span)
                    .is_some()
            {
                out.push(e1344(&command, function.name_span));
            }
            if let Some(self_param) = function
                .params
                .iter()
                .find(|param| param.name == Syntax::KW_SELF)
            {
                if self_param.convention != AccessConvention::Read {
                    out.push(e1347(self_param.name_span));
                }
            }
            validate_callable_params(function, &command, None, &mut out);
        }
        for binding in &s.cli_bindings {
            command_count += 1;
            let command = binding.name.to_lowercase();
            if seen_flags.contains_key(&command)
                || seen_commands
                    .insert(command.clone(), binding.name_span)
                    .is_some()
            {
                out.push(e1344(&command, binding.name_span));
            }
            if binding
                .markers
                .iter()
                .any(|marker| marker.name != Syntax::MARKER_DOC)
            {
                out.push(e1345(
                    &binding.name,
                    binding.name_span,
                    "a callable member accepts `#Doc`, but field-only CLI markers do not apply to bindings",
                ));
            }
            let Some(function) = binding_target(items, binding) else {
                out.push(e1345(
                    &binding.name,
                    binding.target.span(),
                    "the binding target must be a function declared in the same program module",
                ));
                continue;
            };
            validate_callable_params(function, &binding.name, Some(&s.name), &mut out);
        }
        if command_count > 0 && has_codable_shape(s) {
            out.push(e1346(s.name_span));
        }
    }
    out
}
