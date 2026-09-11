use crate::AST::{CallArg, Expr, Func, Item, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use jet_foundation::AST::{
    DevtoolsFactPublication, DevtoolsPanel, DevtoolsRegistry, DevtoolsRegistryError,
    DevtoolsStateField,
};

/// Registered diagnostic rows owned by the public panel contract.
pub const E_DEVTOOLS_PUBLISH_GATE: &str = "E1404";
pub const E_DEVTOOLS_DUPLICATE_PANEL: &str = "E1410";
pub const E_DEVTOOLS_INVALID_PANEL: &str = "E1411";
pub const E_DEVTOOLS_UNKNOWN_FIELD: &str = "E1412";
pub const E_DEVTOOLS_FIELD_TYPE: &str = "E1413";
pub const E_DEVTOOLS_UNFED_FIELD: &str = "E1414";

/// Validate every `#DevPanel` function in one source module and project it into
/// the one package-wide devtools registry. Struct declarations are package
/// scoped, while the module identity remains the stable source module path.
pub fn check_devtools_panels(
    package: &str,
    module: &str,
    package_items: &[Item],
    module_items: &[Item],
    registry: &mut DevtoolsRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    let structs = package_items.iter().filter_map(|item| match item {
        Item::Struct(def) => Some((def.name.as_str(), def)),
        _ => None,
    });
    let structs = structs.collect::<std::collections::BTreeMap<_, _>>();

    for function in module_items.iter().filter_map(|item| match item {
        Item::Func(function) if has_dev_panel_marker(function) => Some(function),
        _ => None,
    }) {
        let Some(marker) = function.dev_panel_marker() else {
            continue;
        };
        let markers = function
            .markers
            .iter()
            .filter_map(|marker| marker.dev_panel_marker())
            .collect::<Vec<_>>();
        if markers.len() != 1 {
            diags.push(panel_error(
                E_DEVTOOLS_DUPLICATE_PANEL,
                &function.name,
                "a devtools panel is registered once per function",
                "keep one `#DevPanel` marker on the public `panel` function",
                marker.span,
            ));
            continue;
        }
        if !marker.is_valid() {
            diags.push(panel_error(
                E_DEVTOOLS_INVALID_PANEL,
                &function.name,
                "`#DevPanel` has no arguments and cannot be negated",
                "write `#DevPanel` without `!` or marker arguments",
                marker.span,
            ));
            continue;
        }
        let Some(state_type) = validate_panel_signature(function, diags) else {
            continue;
        };
        let Type::Named(state_name) = &state_type else {
            // `validate_panel_signature` already emitted the source diagnostic;
            // this guard keeps the state lookup total for future Type variants.
            continue;
        };
        let Some(state) = structs.get(state_name.as_str()) else {
            diags.push(panel_error(
                E_DEVTOOLS_INVALID_PANEL,
                &function.name,
                &format!("panel state type `{state_name}` is not a declared struct"),
                &format!("declare `struct {state_name}` in the same package"),
                function.params[0].ty_span,
            ));
            continue;
        };
        let state_fields = state
            .fields
            .iter()
            .map(|field| {
                DevtoolsStateField::new(
                    field.name.clone(),
                    field.ty.clone(),
                    field.name_span,
                    field.default.is_some(),
                )
            })
            .collect::<Vec<_>>();
        for field in &state_fields {
            if !field.is_ready_before_publication() {
                diags.push(panel_error(
                    E_DEVTOOLS_INVALID_PANEL,
                    &function.name,
                    &format!(
                        "state field `{}` has neither an absence default nor an optional type",
                        field.name
                    ),
                    &format!(
                        "give `{}` a default value or declare it as `?{}`",
                        field.name,
                        field.ty.show()
                    ),
                    field.span,
                ));
            }
        }
        let panel = DevtoolsPanel::new(
            package,
            module,
            function.name.clone(),
            state_type,
            state_fields,
            function.span,
        );
        if let Err(error) = registry.register_panel(panel) {
            diags.push(registry_error(error));
        }
    }
}

/// Check and record one `core.devtools.publish` call after its arguments have
/// been bound and its value expression has been inferred. The explicit
/// `field:` and `value:` labels are the only publication gate.
pub(crate) fn check_devtools_publish(
    package: &str,
    module: &str,
    args: &mut [CallArg],
    value_type: Option<Type>,
    span: Span,
    registry: &DevtoolsRegistry,
    facts: &crate::Sema::TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    if args.len() != 2
        || arg_label(args, 0) != Some("field")
        || arg_label(args, 1) != Some("value")
    {
        diags.push(Diagnostic::error(
            E_DEVTOOLS_PUBLISH_GATE,
            "this published value has no gate".to_string(),
            "a published value must name itself with `field:` and `value:`".to_string(),
            "write `devtools.publish(field: .state_field, value: <expr>)`".to_string(),
            Some(span),
        ));
        return false;
    }

    let Some(field) = leading_field_name(&args[0].expr) else {
        diags.push(panel_error(
            E_DEVTOOLS_UNKNOWN_FIELD,
            "publish",
            "the `field:` argument must be a leading-dot state field selector",
            "write `field: .state_field` for a field in the marked panel state",
            args[0].expr.span(),
        ));
        return false;
    };
    let Some(value_type) = value_type else {
        return false;
    };
    let Some(panel) = registry
        .panel_for_field(package, module, &field)
        .or_else(|| registry.unique_panel_for_field(package, &field))
    else {
        diags.push(panel_error(
            E_DEVTOOLS_UNKNOWN_FIELD,
            "publish",
            &format!("the package has no uniquely identified panel field `{field}`"),
            "publish a field declared by the package's marked `panel` state",
            args[0].expr.span(),
        ));
        return false;
    };

    let publication = DevtoolsFactPublication::new(
        package,
        panel.module.clone(),
        panel.identity(),
        field,
        value_type,
        span,
    );
    if let Err(error) = registry.validate_publication(&publication) {
        diags.push(registry_error(error));
        return false;
    }
    // The TypeRegistry is the existing per-module semantic-facts owner. Keep
    // this write after inference so no downstream phase needs to re-infer the
    // publication value or reconstruct its source identity.
    facts.record_devtools_publication(publication);
    true
}

fn arg_label(args: &[CallArg], index: usize) -> Option<&str> {
    args.get(index)
        .and_then(|arg| arg.label.as_ref())
        .map(|(label, _)| label.as_str())
}

/// Return the package's state fields that have no publication site.
pub fn unfed_state_fields<'a, 'p>(
    package: &'p str,
    registry: &'a DevtoolsRegistry,
) -> impl Iterator<Item = (&'a DevtoolsPanel, &'a DevtoolsStateField)> + use<'a, 'p> {
    registry
        .panels()
        .filter(move |panel| panel.package == package)
        .flat_map(move |panel| {
            panel
                .state_fields
                .iter()
                .filter(move |field| {
                    !registry.publications().iter().any(|publication| {
                        publication.panel == panel.identity() && publication.field == field.name
                    })
                })
                .map(move |field| (panel, field))
        })
}

/// Emit the registered diagnostic for every panel state field that has no
/// checked publication site. This runs after all module body checks.
pub fn check_unfed_state_fields(registry: &DevtoolsRegistry, diags: &mut Vec<Diagnostic>) {
    for panel in registry.panels() {
        for field in &panel.state_fields {
            let fed = registry.publications().iter().any(|publication| {
                publication.panel == panel.identity() && publication.field == field.name
            });
            if fed {
                continue;
            }
            diags.push(Diagnostic::error(
                E_DEVTOOLS_UNFED_FIELD,
                format!(
                    "devtools panel `{}` state field `{}` has no publication",
                    panel.identity(),
                    field.name
                ),
                "every panel state field must have a checked `core.devtools.publish` site"
                    .to_string(),
                format!("publish `field: .{}` with the field's declared type", field.name),
                Some(field.span),
            ));
        }
    }
}

fn has_dev_panel_marker(function: &Func) -> bool {
    function
        .markers
        .iter()
        .any(|marker| marker.name == Syntax::MARKER_DEV_PANEL)
}

fn validate_panel_signature(function: &Func, diags: &mut Vec<Diagnostic>) -> Option<Type> {
    if !function.is_pub || function.is_package_pub {
        diags.push(panel_error(
            E_DEVTOOLS_INVALID_PANEL,
            &function.name,
            "a devtools panel is a public package entry point",
            "write `pub fn panel(state: State) UiNode -> { ... }`",
            function.name_span,
        ));
        return None;
    }
    if function.name != "panel"
        || !function.type_params.is_empty()
        || function.params.len() != 1
        || function.params[0].name != "state"
        || function.params[0].default.is_some()
        || function.params[0].variadic
    {
        diags.push(panel_error(
            E_DEVTOOLS_INVALID_PANEL,
            &function.name,
            "a devtools panel must be the public `panel(state: State)` function",
            "write one non-generic `state` parameter without a default or variadic marker",
            function.name_span,
        ));
        return None;
    }
    let Some(return_type) = function.return_type.as_ref() else {
        diags.push(panel_error(
            E_DEVTOOLS_INVALID_PANEL,
            &function.name,
            "a devtools panel must return a `UiNode` tree",
            "declare `UiNode` before the function body arrow",
            function.name_span,
        ));
        return None;
    };
    if !matches!(return_type, Type::Named(name) if name == Syntax::TYPE_UI_NODE) {
        diags.push(panel_error(
            E_DEVTOOLS_INVALID_PANEL,
            &function.name,
            &format!("a devtools panel returns `{}` instead of `UiNode`", return_type.show()),
            "return the portable `UiNode` tree from the panel function",
            function.return_type_span.unwrap_or(function.name_span),
        ));
        return None;
    }
    Some(function.params[0].ty.clone())
}

fn leading_field_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Field(base, field, _)
            if matches!(base.as_ref(), Expr::Ident(name, _) if name.is_empty()) =>
        {
            Some(field.clone())
        }
        _ => None,
    }
}

fn panel_error(code: &str, panel: &str, why: &str, fix: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        code,
        format!("devtools panel `{panel}` is invalid"),
        why.to_string(),
        fix.to_string(),
        Some(span),
    )
}

pub(crate) fn registry_error(error: DevtoolsRegistryError) -> Diagnostic {
    match error {
        DevtoolsRegistryError::DuplicatePanel {
            identity,
            first,
            duplicate,
        } => Diagnostic::error(
            E_DEVTOOLS_DUPLICATE_PANEL,
            format!("devtools panel `{identity}` is registered twice"),
            format!(
                "one package/module/function identity may describe only one panel (first at {}..{})",
                first.start, first.end
            ),
            "keep one marked public `panel` function for this package".to_string(),
            Some(duplicate),
        ),
        DevtoolsRegistryError::UnknownStateField {
            package,
            module,
            panel,
            field,
            span,
        } => Diagnostic::error(
            E_DEVTOOLS_UNKNOWN_FIELD,
            format!("devtools publication names unknown state field `{field}`"),
            format!(
                "package `{package}` module `{module}` has no marked panel `{panel}` field with that name"
            ),
            format!("write `field: .{field}` using a field from the package's `panel` state"),
            Some(span),
        ),
        DevtoolsRegistryError::StateFieldTypeMismatch {
            package,
            module,
            panel,
            field,
            expected,
            actual,
            span,
        } => Diagnostic::error(
            E_DEVTOOLS_FIELD_TYPE,
            format!(
                "published value for `{package}::{module}::{panel}::{field}` has type `{}`",
                actual.show()
            ),
            format!("the panel state field has type `{}`", expected.show()),
            format!("publish a value with type `{}` or change the state field", expected.show()),
            Some(span),
        ),
    }
}
