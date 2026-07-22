use super::*;

pub(super) fn is_fallible_void_entry_return(ty: &Type, state: &ModuleState) -> bool {
    matches!(
        ty,
        Type::Result { ok, err }
            if matches!(ok.as_ref(), Type::Named(n) if n == Syntax::TYPE_VOID)
                && matches!(err.as_ref(), Type::Named(n)
                    if n == Syntax::TYPE_ERROR
                        || (n == "CryptoError"
                            && !state.registry.contains(n)
                            && state.core_imports.values().any(|module| module == "jet.crypto" || module == "core.crypto")))
    )
}
/// D-CLIFLAG1: what `fn run`'s single parameter type turned out to be.
pub(super) enum CliEntryShape {
    /// A `@[Cli]`-derived struct — flags come straight from its fields.
    Struct,
    /// An `enum` whose every variant carries a `@[Cli]` struct payload.
    Enum,
    /// An `enum` parameter with at least one non-`@[Cli]` variant (E1307).
    EnumBadVariants(Vec<Diagnostic>),
    /// Neither of the above (E1308).
    Invalid,
}

/// D-CLIFLAG1: classify `fn run`'s parameter type against its defining module.
/// The entry signature stays in the entry file; its public `@[Cli]` type may
/// live in one directly imported module.
pub(super) fn cli_entry_param_shape(items: &[Item], ty: &Type, reg: &TraitRegistry) -> CliEntryShape {
    let Type::Named(name) = ty else {
        return CliEntryShape::Invalid;
    };
    let name = name.rsplit('.').next().unwrap_or(name);
    if reg.implements_trait(name, "Cli") {
        return CliEntryShape::Struct;
    }
    let enum_def: Option<&EnumDef> = items.iter().find_map(|i| match i {
        Item::Enum(e) if &e.name == name => Some(e),
        _ => None,
    });
    let Some(e) = enum_def else {
        return CliEntryShape::Invalid;
    };
    let mut bad = Vec::new();
    for v in &e.variants {
        let ok = matches!(
            &v.payload,
            VariantPayload::Single(Type::Named(p), _) if reg.implements_trait(p, "Cli")
        );
        if !ok {
            bad.push(e1307(&v.name, v.name_span));
        }
    }
    if bad.is_empty() {
        CliEntryShape::Enum
    } else {
        CliEntryShape::EnumBadVariants(bad)
    }
}

/// E0101: the entry file has no canonical `fn run`.
pub(super) fn no_run_error() -> Diagnostic {
    Diagnostic::error(
        "E0101",
        "this program has no `run` function".to_string(),
        "running a program starts at `fn run`, and the entry file doesn't define one".to_string(),
        "add `fn run() { ... }` to the entry file".to_string(),
        None,
    )
}

fn output_error(what: String, why: String, fix: String, span: Span) -> Diagnostic {
    Diagnostic::error("E1321", what, why, fix, Some(span))
}

fn output_string(expr: &Expr) -> Option<String> {
    let Expr::Str(parts, _) = expr else { return None };
    match parts.as_slice() {
        [StrPart::Lit(value)] => Some(value.clone()),
        _ => None,
    }
}

fn output_fields<'a>(args: &'a [EnumLitArg], span: Span, diags: &mut Vec<Diagnostic>) -> Option<HashMap<&'a str, &'a Expr>> {
    let mut fields = HashMap::new();
    for arg in args {
        let EnumLitArg::Named { label, expr } = arg else {
            diags.push(output_error(
                "an Output payload uses named fields".to_string(),
                "Output facts need stable field names for checking and inspection".to_string(),
                "write `.Executable.{ name: \"app\", entry: run }`".to_string(),
                span,
            ));
            return None;
        };
        if fields.insert(label.as_str(), expr).is_some() {
            diags.push(output_error(
                format!("Output field `{label}` is written twice"),
                "one Output field has one checked value".to_string(),
                format!("remove one `{label}:` field"),
                expr.span(),
            ));
            return None;
        }
    }
    Some(fields)
}

fn resolve_output_callable(
    module_idx: usize,
    address: String,
    kind: crate::AST::OutputKind,
    output_name: String,
    entry: &Expr,
    bundle: &ProgramBundle,
    states: &[ModuleState],
    diags: &mut Vec<Diagnostic>,
) -> Option<crate::AST::ResolvedOutput> {
    let (target, source_name, lowered_name) = match entry {
        Expr::Ident(name, span) => {
            let state = &states[module_idx];
            if state.funcs.contains_key(name) {
                (module_idx, name.clone(), name.clone())
            } else if let Some(mangled) = state.unqualified.get(name) {
                (module_idx, name.clone(), mangled.clone())
            } else if let Some((real, target)) = state.unqualified_file.get(name) {
                (*target, real.clone(), real.clone())
            } else {
                diags.push(output_error(
                    format!("Output entry `{name}` does not resolve to a function"),
                    "an Output entry is an ordinary checked function reference, not a text lookup".to_string(),
                    format!("define `fn {name}(...)`, import it, or update `entry:`"),
                    *span,
                ));
                return None;
            }
        }
        Expr::Field(base, member, span) => {
            let Expr::Ident(alias, alias_span) = base.as_ref() else {
                diags.push(output_error(
                    "Output entry has no resolvable module owner".to_string(),
                    "qualified function references use one imported or inline module alias".to_string(),
                    "write `entry: module_name.function`".to_string(),
                    *span,
                ));
                return None;
            };
            let state = &states[module_idx];
            if let Some(canonical) = state.code_modules.get(alias) {
                let lowered = format!("{canonical}__{member}");
                if !state.funcs.contains_key(&lowered) {
                    diags.push(output_error(
                        format!("module `{alias}` has no function `{member}`"),
                        "Output entries use ordinary module member resolution".to_string(),
                        "update `entry:` to a function that exists".to_string(),
                        *span,
                    ));
                    return None;
                }
                (module_idx, member.clone(), lowered)
            } else if let Some(target) = state.imports.get(alias).copied() {
                let target_state = &states[target];
                if !target_state.funcs.contains_key(member) {
                    diags.push(output_error(
                        format!("module `{alias}` has no function `{member}`"),
                        "Output entries use ordinary module member resolution".to_string(),
                        "update `entry:` to a function that exists".to_string(),
                        *span,
                    ));
                    return None;
                }
                let same_package = target_state.package_scope == state.package_scope;
                let visible = target_state.func_pub.get(member).copied().unwrap_or(false)
                    || (same_package && target_state.func_pkg_pub.get(member).copied().unwrap_or(false));
                if !visible {
                    diags.push(output_error(
                        format!("function `{alias}.{member}` is private"),
                        "an Output can only invoke code visible from its declaring module".to_string(),
                        format!("make `fn {member}` public to this package, or keep the Output beside it"),
                        *span,
                    ));
                    return None;
                }
                (target, member.clone(), member.clone())
            } else {
                diags.push(output_error(
                    format!("no module named `{alias}` is in scope"),
                    "qualified Output entries use ordinary imported module aliases".to_string(),
                    format!("import the module before `entry: {alias}.{member}`"),
                    *alias_span,
                ));
                return None;
            }
        }
        _ => {
            diags.push(output_error(
                "Output entry is not a function reference".to_string(),
                "text names, calls, and runtime reflection are not Output links".to_string(),
                "write a bare or qualified function reference, such as `entry: run`".to_string(),
                entry.span(),
            ));
            return None;
        }
    };
    let semantic_name = lowered_name;
    let signature = states[target]
        .funcs
        .get(&semantic_name)
        .or_else(|| states[target].funcs.get(&source_name))?;
    if signature.is_extern || signature.is_unsafe {
        diags.push(output_error(
            format!("Output entry `{source_name}` is not an ordinary safe Jet function"),
            "package tools invoke checked Jet functions without granting an FFI or unsafe authority boundary".to_string(),
            "wrap the boundary in an ordinary safe Jet function and reference that function".to_string(),
            entry.span(),
        ));
        return None;
    }
    let mut contract_diags = Vec::new();
    let params_ok = match kind {
        crate::AST::OutputKind::Executable if signature.params.len() == 1 => {
            let param_ty = &signature.params[0].1;
            match cli_entry_param_shape(
                &bundle.modules[target].items,
                param_ty,
                &states[target].trait_reg,
            ) {
                CliEntryShape::Struct | CliEntryShape::Enum => true,
                CliEntryShape::EnumBadVariants(bad) => {
                    contract_diags.extend(bad);
                    false
                }
                CliEntryShape::Invalid => false,
            }
        }
        crate::AST::OutputKind::Executable => signature.params.is_empty(),
        crate::AST::OutputKind::Service | crate::AST::OutputKind::Check => signature.params.is_empty(),
        _ => false,
    };
    let return_ok = signature.return_type.as_ref().is_none_or(|ty| {
        matches!(ty, Type::Named(name) if name == Syntax::TYPE_VOID)
            || is_fallible_void_entry_return(ty, &states[target])
    });
    if !params_ok || !return_ok {
        diags.extend(contract_diags);
        let contract = if kind == crate::AST::OutputKind::Executable {
            "an Executable takes zero or one typed CLI parameter and returns `Void` or `Void ?`"
        } else {
            "a Service or Check takes no parameters and returns `Void` or `Void ?`"
        };
        diags.push(output_error(
            format!("Output entry `{source_name}` has the wrong callable contract"),
            contract.to_string(),
            format!("change `fn {source_name}` to match the {kind:?} contract"),
            entry.span(),
        ));
        return None;
    }
    let module_alias = &bundle.modules[target].alias;
    let rust_path = if target == bundle.entry {
        format!("user_{semantic_name}")
    } else {
        format!("user_{module_alias}::user_{source_name}")
    };
    Some(crate::AST::ResolvedOutput {
        address,
        kind,
        output_name,
        module: target,
        source_path: bundle.modules[target].display.clone(),
        source_name: source_name.clone(),
        semantic_name: semantic_name.clone(),
        lowered_name: rust_path,
        params: signature.params.clone(),
        return_type: signature.return_type.clone(),
        reference: entry.span(),
        definition: states[target].func_spans.get(&semantic_name)
            .or_else(|| states[target].func_spans.get(&source_name)).copied().unwrap_or(entry.span()),
        authority: crate::AST::OutputCallableAuthority::SafeJet,
        effects: Vec::new(),
        selected: false,
    })
}

fn output_default(bundle: &ProgramBundle, field: &str, diags: &mut Vec<Diagnostic>) -> Option<String> {
    let value = bundle.modules[bundle.entry].items.iter().find_map(|item| {
        let Item::Const(value) = item else { return None };
        matches!(&value.ty, Some(Type::Named(name)) if name == Syntax::TYPE_OUTPUT_DEFAULTS)
            .then_some(&value.value)
    })?;
    let Expr::StructLit { fields, .. } = value else {
        diags.push(output_error(
            "`defaults:` needs one checked record".to_string(),
            "Output defaults map singular tool intents to Output addresses".to_string(),
            "write `defaults: .{ run: app }`".to_string(),
            value.span(),
        ));
        return None;
    };
    fields.iter().find_map(|(name, _, value)| {
        if name != field { return None; }
        match value {
            Expr::Ident(address, _) => Some(address.clone()),
            _ => {
                diags.push(output_error(
                    format!("default `{field}` is not an Output address"),
                    "defaults use checked Output references, not text or calls".to_string(),
                    format!("write `{field}: output_name`"),
                    value.span(),
                ));
                None
            }
        }
    })
}

pub(super) fn resolve_outputs(
    bundle: &mut ProgramBundle,
    states: &[ModuleState],
    mode: CompileMode,
    explicit: Option<&str>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut resolved = Vec::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for (item_idx, item) in module.items.iter().enumerate() {
            let Item::Const(value) = item else { continue };
            if !matches!(&value.ty, Some(Type::Named(name)) if name == Syntax::TYPE_OUTPUT) { continue; }
            let Expr::EnumLit { variant, args, span, .. } = &value.value else {
                diags.push(output_error("Output needs one closed kind".to_string(), "Output is the closed sum of the nine ratified kinds".to_string(), "write `.Executable.{ ... }`, `.Service.{ ... }`, or another Output kind".to_string(), value.value.span()));
                continue;
            };
            let Some(kind) = crate::AST::OutputKind::from_name(variant) else {
                diags.push(output_error(format!("`{variant}` is not an Output kind"), "Output has exactly nine ratified kinds".to_string(), format!("use one of: {}", Syntax::OUTPUT_KINDS.join(", ")), *span));
                continue;
            };
            let Some(fields) = output_fields(args, *span, diags) else { continue };
            let Some(name_expr) = fields.get(Syntax::OUTPUT_FIELD_NAME) else {
                diags.push(output_error("Output is missing `name:`".to_string(), "every Output has one stable user-facing name".to_string(), "add `name: \"...\"`".to_string(), *span));
                continue;
            };
            let Some(output_name) = output_string(name_expr) else {
                diags.push(output_error("Output `name:` must be fixed text".to_string(), "graph identity cannot depend on runtime interpolation".to_string(), "write a plain string literal".to_string(), name_expr.span()));
                continue;
            };
            if !kind.is_runnable() {
                if let Some(entry) = fields.get(Syntax::OUTPUT_FIELD_ENTRY) {
                    diags.push(output_error(format!("{kind:?} Output is not runnable"), "only Executable, Service, and Check link to functions".to_string(), "remove `entry:` from this Output".to_string(), entry.span()));
                }
                continue;
            }
            if let Some((field, value)) = args.iter().find_map(|arg| match arg {
                EnumLitArg::Named { label, expr }
                    if label != Syntax::OUTPUT_FIELD_NAME
                        && label != Syntax::OUTPUT_FIELD_ENTRY =>
                {
                    Some((label, expr))
                }
                _ => None,
            }) {
                diags.push(output_error(
                    format!("`{field}:` is not a runnable Output field"),
                    "Executable, Service, and Check Outputs contain only their stable name and checked entry reference".to_string(),
                    format!("remove `{field}:` from this Output"),
                    value.span(),
                ));
                continue;
            }
            let Some(entry) = fields.get(Syntax::OUTPUT_FIELD_ENTRY) else {
                diags.push(output_error(format!("{kind:?} Output is missing `entry:`"), "every runnable Output links to one checked function".to_string(), "add `entry: function_name`".to_string(), *span));
                continue;
            };
            if let Some(fact) = resolve_output_callable(module_idx, value.name.clone(), kind, output_name, entry, bundle, states, diags) {
                resolved.push((module_idx, item_idx, fact));
            }
        }
    }
    let has_legacy_run = bundle.modules[bundle.entry].items.iter().any(|item| {
        matches!(item, Item::Func(function) if function.name == "run")
    });
    let run_default = output_default(bundle, Syntax::OUTPUT_DEFAULT_RUN, diags);
    let run_default_index = run_default.as_ref().and_then(|address| {
        resolved.iter().position(|(_, _, fact)| {
            fact.kind == crate::AST::OutputKind::Executable && fact.address == *address
        })
    });
    if let Some(default) = run_default.as_ref().filter(|_| run_default_index.is_none()) {
        diags.push(output_error(
            format!("default `run` names incompatible Output `{default}`"),
            "the run default must name an Executable Output in this Package".to_string(),
            "point `defaults.run` at one checked Executable address".to_string(),
            Span::new(0, 0),
        ));
    }
    if mode == CompileMode::Run {
        if let Some(address) = explicit {
            if let Some((_, _, fact)) = resolved.iter_mut().find(|(_, _, fact)| {
                fact.address == address
                    && matches!(
                        fact.kind,
                        crate::AST::OutputKind::Executable | crate::AST::OutputKind::Service
                    )
            }) {
                fact.selected = true;
            } else {
                let mut choices = resolved
                    .iter()
                    .filter(|(_, _, fact)| {
                        matches!(
                            fact.kind,
                            crate::AST::OutputKind::Executable | crate::AST::OutputKind::Service
                        )
                    })
                    .map(|(_, _, fact)| fact.address.clone())
                    .collect::<Vec<_>>();
                choices.sort();
                diags.push(output_error(
                    format!("`{address}` is not an Output compatible with `jet run`"),
                    "an explicit run address must name one checked Executable or Service Output".to_string(),
                    format!("choose one of: {}", choices.join(", ")),
                    Span::new(0, 0),
                ));
            }
        } else if !has_legacy_run {
            let executable = resolved.iter().enumerate()
                .filter(|(_, (_, _, fact))| fact.kind == crate::AST::OutputKind::Executable)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if executable.len() == 1 {
                resolved[executable[0]].2.selected = true;
            } else if executable.len() > 1 {
                if let Some(index) = run_default_index {
                    resolved[index].2.selected = true;
                } else if run_default.is_none() {
                    let mut names = executable.iter().map(|index| resolved[*index].2.address.clone()).collect::<Vec<_>>();
                    names.sort();
                    diags.push(Diagnostic::error("E1321", "this Package has more than one runnable Executable".to_string(), "without `fn run`, a singular run selects only a sole compatible Output or a checked default".to_string(), format!("choose an explicit Output or add `defaults: .{{ run: {} }}`; candidates: {}", names[0], names.join(", ")), None));
                }
            }
        }
    } else if mode == CompileMode::Test {
        for (_, _, fact) in &mut resolved {
            fact.selected = fact.kind == crate::AST::OutputKind::Check;
        }
    }
    for (module, item, fact) in resolved {
        if let Item::Const(value) = &mut bundle.modules[module].items[item] { value.resolved_output = Some(fact); }
    }
}
