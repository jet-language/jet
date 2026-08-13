use super::*;

/// D-ONCE-DERIVE1=A / I3: compiler-owned capability requests lower to the
/// same parsed implementation surface as user-authored code. The generated
/// block is attached to a struct or enum so generic owners inherit their
/// declaration parameters through the ordinary in-type implementation path.
pub(in super::super) fn expand_builtin_derive_items(
    items: &mut Vec<Item>,
    diags: &mut Vec<Diagnostic>,
) {
    let auto = crate::Traits::TraitRegistry::auto_derives_for_items(items);
    let invalid_distinct_names: std::collections::HashSet<String> = items
        .iter()
        .filter_map(|item| {
            let Item::Distinct(d) = item else { return None };
            let crate::AST::Type::Named(base) = &d.base else {
                return None;
            };
            items.iter().any(|item| {
                matches!(item, Item::Distinct(other) if other.name == *base)
            }).then(|| d.name.clone())
        })
        .collect();
    let mut requests = Vec::new();

    for item in items.iter() {
        match item {
            Item::Struct(s) => {
                let comparable = has_derive(&s.derives, crate::Generics::COMPARABLE);
                let equatable = comparable
                    || has_derive(&s.derives, crate::Generics::EQUATABLE)
                    || auto.auto_equatable.contains(&s.name);
                if equatable || comparable {
                    requests.push((
                        s.name.clone(),
                        derive_source_for_fields(&s.name, &s.fields, equatable, comparable),
                        true,
                    ));
                }
            }
            Item::Enum(e) => {
                let comparable = has_derive(&e.derives, crate::Generics::COMPARABLE);
                let equatable = comparable
                    || has_derive(&e.derives, crate::Generics::EQUATABLE)
                    || auto.auto_equatable.contains(&e.name);
                if equatable || comparable {
                    requests.push((
                        e.name.clone(),
                        derive_source_for_enum(&e.name, &e.variants, equatable, comparable),
                        true,
                    ));
                }
            }
            Item::Distinct(d) => {
                if invalid_distinct_names.contains(&d.name) {
                    continue;
                }
                let comparable = has_derive(&d.derives, crate::Generics::COMPARABLE);
                requests.push((
                    d.name.clone(),
                    derive_source_for_distinct(&d.name, &d.base, comparable),
                    false,
                ));
            }
            // Unit-family members are virtual distinct declarations. Put their
            // capability implementations through this same parsed path before
            // codegen sees the family, so the virtual declarations do not retain
            // a private raw-Rust implementation route.
            Item::UnitFamily(family) => {
                for d in family.distinct_defs() {
                    let comparable = has_derive(&d.derives, crate::Generics::COMPARABLE);
                    requests.push((
                        d.name.clone(),
                        derive_source_for_distinct(&d.name, &d.base, comparable),
                        false,
                    ));
                }
            }
            _ => {}
        }
    }

    for (type_name, source, attach_to_type) in requests {
        if source.is_empty() {
            continue;
        }
        let Some(mut generated) = parse_generated_fragment(
            &source,
            format!("built-in derive generated invalid Jet for `{type_name}`"),
            "built-in derives must emit valid ordinary Jet".to_string(),
            items
                .iter()
                .find_map(|item| derive_trigger_span(item, &type_name))
                .unwrap_or_else(|| Span::new(0, 0)),
            diags,
        ) else {
            continue;
        };
        for item in generated.drain(..) {
            match item {
                Item::Func(function) => {
                    // Recursive enum equality uses a generated free helper so its
                    // structural `==` calls do not recurse through `equal` itself.
                    // Keep that helper in the ordinary item table; dropping it here
                    // would leave the attached implementation calling an unknown name.
                    if !items.iter().any(|item| {
                        matches!(item, Item::Func(existing) if existing.name == function.name)
                    }) {
                        items.push(Item::Func(function));
                    }
                }
                Item::Impl(implementation) => {
                    if has_trait_impl(items, &type_name, implementation.trait_name.as_deref()) {
                        continue;
                    }
                    if attach_to_type {
                        if let Some(target) = items.iter_mut().find_map(|item| match item {
                            Item::Struct(s) if s.name == type_name => Some(&mut s.trait_impls),
                            Item::Enum(e) if e.name == type_name => Some(&mut e.trait_impls),
                            _ => None,
                        }) {
                            let Some(trait_name) = implementation.trait_name else {
                                continue;
                            };
                            target.push(crate::AST::TraitImplBlock {
                                trait_name,
                                trait_span: implementation.trait_span.unwrap_or(implementation.type_span),
                                methods: implementation.methods,
                                assoc_type_impls: implementation.assoc_type_impls,
                            });
                        }
                    } else {
                        items.push(Item::Impl(implementation));
                    }
                }
                _ => {}
            }
        }
    }
}

fn has_derive(derives: &[(String, Span)], name: &str) -> bool {
    derives.iter().any(|(derive, _)| derive == name)
}

fn derive_trigger_span(item: &Item, type_name: &str) -> Option<Span> {
    match item {
        Item::Struct(s) if s.name == type_name => s
            .derives
            .iter()
            .find(|(name, _)| {
                name == crate::Generics::COMPARABLE || name == crate::Generics::EQUATABLE
            })
            .map(|(_, span)| *span)
            .or(Some(s.name_span)),
        Item::Enum(e) if e.name == type_name => e
            .derives
            .iter()
            .find(|(name, _)| {
                name == crate::Generics::COMPARABLE || name == crate::Generics::EQUATABLE
            })
            .map(|(_, span)| *span)
            .or(Some(e.name_span)),
        Item::Distinct(d) if d.name == type_name => d
            .derives
            .first()
            .map(|(_, span)| *span)
            .or(Some(d.name_span)),
        Item::UnitFamily(family) => family
            .distinct_defs()
            .into_iter()
            .find(|d| d.name == type_name)
            .and_then(|d| d.derives.first().map(|(_, span)| *span).or(Some(d.name_span))),
        _ => None,
    }
}

fn has_trait_impl(items: &[Item], type_name: &str, trait_name: Option<&str>) -> bool {
    let Some(trait_name) = trait_name else {
        return true;
    };
    items.iter().any(|item| match item {
        Item::Impl(i) => i.type_name == type_name && i.trait_name.as_deref() == Some(trait_name),
        Item::Struct(s) => {
            s.name == type_name && s.trait_impls.iter().any(|i| i.trait_name == trait_name)
        }
        Item::Enum(e) => {
            e.name == type_name && e.trait_impls.iter().any(|i| i.trait_name == trait_name)
        }
        _ => false,
    })
}

fn derive_source_for_fields(
    type_name: &str,
    fields: &[crate::AST::Field],
    equatable: bool,
    comparable: bool,
) -> String {
    let fields = fields
        .iter()
        .filter(|field| field.computed.is_none())
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    let mut source = String::new();
    if equatable {
        let equality = if fields.is_empty() {
            "true".to_string()
        } else {
            fields
                .iter()
                .map(|field| format!("self.{field} == rhs.{field}"))
                .collect::<Vec<_>>()
                .join(" && ")
        };
        source.push_str(&format!(
            "impl {type_name}.Equatable {{\nfn equal(self, rhs: {type_name}) => Bool {{\nreturn {equality}\n}}\n}}\n"
        ));
    }
    if comparable {
        source.push_str(&format!(
            "impl {type_name}.Comparable {{\nfn compare(self, rhs: {type_name}) => Ordering {{\n"
        ));
        for field in fields {
            source.push_str(&format!(
                "if self.{field} < rhs.{field} {{ return Ordering.Less }} else {{}}\nif self.{field} > rhs.{field} {{ return Ordering.Greater }} else {{}}\n"
            ));
        }
        source.push_str("return Ordering.Equal\n}\n}\n");
    }
    source
}

fn derive_source_for_enum(
    type_name: &str,
    variants: &[crate::AST::Variant],
    equatable: bool,
    comparable: bool,
) -> String {
    let mut source = String::new();
    if equatable {
        let recursive = variants.iter().any(|variant| match &variant.payload {
            crate::AST::VariantPayload::Unit => false,
            crate::AST::VariantPayload::Single(ty, _) => {
                matches!(ty, crate::AST::Type::Named(name) if name == type_name)
            }
            crate::AST::VariantPayload::Named(fields) => fields.iter().any(|field| {
                matches!(&field.ty, crate::AST::Type::Named(name) if name == type_name)
            }),
        });
        if recursive {
            // A recursive payload must be compared through a helper outside the
            // Equatable hook.  The ordinary `==` spelling is deliberately rejected
            // inside `equal` because it dispatches back to this same hook; the
            // helper keeps the structural recursion while leaving the hook body a
            // plain call.
            let helper = format!("_jet_derive_equal_{type_name}");
            source.push_str(&format!(
                "fn {helper}(left: {type_name}, right: {type_name}) => Bool {{\n"
            ));
            append_enum_dispatch(
                &mut source,
                variants,
                "left",
                "right",
                |source, left, right, _, _, same| {
                    if same {
                        source.push_str(&format!("return {}\n", equality_expression(left, right)));
                    } else {
                        source.push_str("return false\n");
                    }
                },
            );
            source.push_str(&format!(
                "return false\n}}\nimpl {type_name}.Equatable {{\nfn equal(self, rhs: {type_name}) => Bool {{\nreturn {helper}(self, rhs)\n}}\n}}\n"
            ));
        } else {
            source.push_str(&format!(
                "impl {type_name}.Equatable {{\nfn equal(self, rhs: {type_name}) => Bool {{\n"
            ));
            append_enum_dispatch(
                &mut source,
                variants,
                "self",
                "rhs",
                |source, left, right, _, _, same| {
                    if same {
                        source.push_str(&format!("return {}\n", equality_expression(left, right)));
                    } else {
                        source.push_str("return false\n");
                    }
                },
            );
            source.push_str("return false\n}\n}\n");
        }
    }
    if comparable {
        source.push_str(&format!(
            "impl {type_name}.Comparable {{\nfn compare(self, rhs: {type_name}) => Ordering {{\n"
        ));
        append_enum_dispatch(
            &mut source,
            variants,
            "self",
            "rhs",
            |source, left, right, left_index, right_index, same| {
                if same {
                    append_comparison(source, left, right);
                } else if left_index < right_index {
                    source.push_str("return Ordering.Less\n");
                } else {
                    source.push_str("return Ordering.Greater\n");
                }
            },
        );
        source.push_str("return Ordering.Equal\n}\n}\n");
    }
    source
}

fn append_enum_dispatch(
    source: &mut String,
    variants: &[crate::AST::Variant],
    left_subject: &str,
    right_subject: &str,
    mut body: impl FnMut(&mut String, &[String], &[String], usize, usize, bool),
) {
    source.push_str(&format!("if {left_subject} == {{\n"));
    for (left_index, left_variant) in variants.iter().enumerate() {
        let left = payload_bindings(&left_variant.payload, "left");
        source.push_str(".");
        source.push_str(&left_variant.name);
        append_pattern_slots(source, &left);
        source.push_str(&format!(" -> {{\nif {right_subject} == {{\n"));
        for (right_index, right_variant) in variants.iter().enumerate() {
            let right = payload_bindings(&right_variant.payload, "right");
            source.push_str(".");
            source.push_str(&right_variant.name);
            append_pattern_slots(source, &right);
            source.push_str(" -> {");
            body(
                source,
                &left,
                &right,
                left_index,
                right_index,
                left_index == right_index,
            );
            source.push_str("}\n");
        }
        source.push_str("}\n}\n");
    }
    source.push_str("}\n");
}

fn payload_bindings(payload: &crate::AST::VariantPayload, prefix: &str) -> Vec<String> {
    let count = match payload {
        crate::AST::VariantPayload::Unit => 0,
        crate::AST::VariantPayload::Single(..) => 1,
        crate::AST::VariantPayload::Named(fields) => fields.len(),
    };
    (0..count).map(|index| format!("{prefix}_{index}")).collect()
}

fn append_pattern_slots(
    source: &mut String,
    bindings: &[String],
) {
    if bindings.is_empty() {
        return;
    }
    source.push('(');
    for (index, binding) in bindings.iter().enumerate() {
        if index > 0 {
            source.push_str(", ");
        }
        source.push_str(binding);
    }
    source.push(')');
}

fn equality_expression(left: &[String], right: &[String]) -> String {
    if left.is_empty() {
        return "true".to_string();
    }
    left.iter()
        .zip(right)
        .map(|(left, right)| format!("{left} == {right}"))
        .collect::<Vec<_>>()
        .join(" && ")
}

fn append_comparison(source: &mut String, left: &[String], right: &[String]) {
    for (left, right) in left.iter().zip(right) {
        source.push_str(&format!(
            "if {left} < {right} {{ return Ordering.Less }} else {{}}\nif {left} > {right} {{ return Ordering.Greater }} else {{}}\n"
        ));
    }
    source.push_str("return Ordering.Equal\n");
}

fn derive_source_for_distinct(
    type_name: &str,
    base: &crate::AST::Type,
    comparable: bool,
) -> String {
    // Generated equality for a Float-backed distinct value is compiler
    // machinery, not a user-authored float comparison. The ordered pair has
    // the same IEEE result as `==` (including NaN and signed zero) while it
    // keeps the generated fragment out of the user-facing float-equality lint.
    let equality = if matches!(base, crate::AST::Type::Float | crate::AST::Type::Float32) {
        "self.raw() <= rhs.raw() && self.raw() >= rhs.raw()"
    } else {
        "self.raw() == rhs.raw()"
    };
    let mut source = format!(
        "impl {type_name}.Equatable {{\nfn equal(self, rhs: {type_name}) => Bool {{\nreturn {equality}\n}}\n}}\n"
    );
    if comparable {
        source.push_str(&format!(
            "impl {type_name}.Comparable {{\nfn compare(self, rhs: {type_name}) => Ordering {{\nif self.raw() < rhs.raw() {{ return Ordering.Less }} else {{}}\nif self.raw() > rhs.raw() {{ return Ordering.Greater }} else {{}}\nreturn Ordering.Equal\n}}\n}}\n"
        ));
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_and_user_derive_fragments_share_the_parser() {
        let (tokens, lex_diags) = crate::Lexer::lex("#Comparable struct Point { value: Int }");
        assert!(lex_diags.is_empty());
        let mut built_in_program =
            crate::Parser::parse(&tokens).expect("built-in source parses");
        let mut built_in_diags = Vec::new();
        let mut user_diags = Vec::new();
        expand_builtin_derive_items(&mut built_in_program.items, &mut built_in_diags);
        let user = parse_generated_fragment(
            "impl Point.Comparable { fn compare(self, rhs: Point) => Ordering { return Ordering.Equal } }",
            "user derive generated invalid Jet".to_string(),
            "fix the derive body".to_string(),
            Span::new(0, 1),
            &mut user_diags,
        );
        assert!(built_in_diags.is_empty());
        assert!(user_diags.is_empty());
        assert!(built_in_program.items.iter().any(|item| matches!(
            item,
            Item::Struct(s)
                if s.trait_impls
                    .iter()
                    .any(|implementation| implementation.trait_name == Syntax::MARKER_COMPARABLE)
        )));
        assert!(matches!(user.unwrap().as_slice(), [Item::Impl(_)]));
    }
}
