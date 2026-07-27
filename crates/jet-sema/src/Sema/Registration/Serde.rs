use super::*;

pub(in super::super) fn expand_builtin_serde_items(items: &mut Vec<Item>, diags: &mut Vec<Diagnostic>) {
    let mut generated_items = Vec::new();
    for item in items.iter_mut() {
        if let Item::Enum(e) = item {
            expand_builtin_enum_serde(e, diags, &mut generated_items);
            continue;
        }
        let Item::Struct(s) = item else { continue };
        let enc = s.derives.iter().any(|(n, _)| n == crate::Generics::ENCODE);
        let dec = s.derives.iter().any(|(n, _)| n == crate::Generics::DECODE);
        if !enc && !dec { continue; }

        // The synthetic container exists only to make the generated codec pass
        // through the ordinary parser/checker.  Its inherited parameters need
        // the same wire bounds that the final Rust impl receives; otherwise a
        // field of type `T` is (correctly) rejected as not encodable while the
        // generated Encode body is checked.
        let mut codec_params = s.type_params.clone();
        let wire_types = s.fields.iter()
            .filter(|f| !f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP))
            .map(|f| &f.ty)
            .collect::<Vec<_>>();
        for param in &mut codec_params {
            let reaches_wire = wire_types.iter()
                .any(|ty| crate::Generics::free_type_params(ty).contains(&param.name));
            if reaches_wire && enc && !param.bounds.iter().any(|b| b == crate::Generics::ENCODE) {
                param.bounds.push(crate::Generics::ENCODE.to_string());
            }
            if reaches_wire && dec && !param.bounds.iter().any(|b| b == crate::Generics::DECODE) {
                param.bounds.push(crate::Generics::DECODE.to_string());
            }
        }
        let params = crate::Generics::format_type_params(&codec_params);
        let target = format!("{}{}", s.name, serde_type_arg_names(&s.type_params));
        let mut source = String::new();
        if enc {
            source.push_str(&format!("impl {}.Encode {{\nfn encode{params}(self) => DataTree {{\n", s.name));
            let active: Vec<_> = s.fields.iter().filter(|f|
                !f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP)
            ).collect();
            let needs_mutation = active.iter().any(|f|
                f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_FLATTEN)
            );
            if !needs_mutation {
                source.push_str(&serde_ordered_object_source(&s.serde_markers, &active));
            } else {
                source.push_str("out: [String: DataTree] := []\n");
            for f in &s.fields {
                if f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP)
                { continue; }
                let key = serde_source_field_key(&s.serde_markers, f);
                if f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_FLATTEN) {
                    source.push_str(&format!(
                        "nested :: self.{}.encode()\nif nested == .Object(entries) {{ loop key, value; entries {{ out[key] = value }} }}\n",
                        f.name
                    ));
                } else if matches!(f.ty, Type::Option(_)) {
                    source.push_str(&format!(
                        "if self.{} == Val(value) {{ out[{:?}] = (~value).encode() }}\n",
                        f.name, key
                    ));
                } else {
                    source.push_str(&format!("out[{:?}] = self.{}.encode()\n", key, f.name));
                }
            }
                source.push_str("return DataTree.Object(out)\n");
            }
            source.push_str("}\n}\n");
        }
        if dec {
            source.push_str(&format!("impl {}.Decode {{\n", s.name));
            source.push_str(&format!("fn decode{params}(tree: DataTree) => {target} ? DecodeError {{\n"));
            let deny_unknown = s.serde_markers.iter().any(|m|
                m.name == crate::Syntax::ATTR_DENY_UNKNOWN_FIELDS
            );
            let has_flatten = s.fields.iter().any(|f|
                f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_FLATTEN)
            );
            if deny_unknown && !has_flatten {
                let keys = s.fields.iter()
                    .filter(|f| !f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP))
                    .map(|f| format!("{:?}", serde_source_field_key(&s.serde_markers, f)))
                    .collect::<Vec<_>>()
                    .join(", ");
                source.push_str(&format!(
                    "if (~tree) == .Object(entries) {{ loop key, value; entries {{ if ![{keys}].contains(key) {{ return Err(DecodeError.{{ path: ~key, reason: \"E2412: unknown field `{{key}}`\" }}) }} }} }}\n"
                ));
            }
            source.push_str(&format!("return Ok({target}.{{\n"));
            for f in s.fields.iter().filter(|f| f.computed.is_none()) {
                let value = if f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP) {
                    serde_source_default(f).unwrap_or_else(|| serde_source_zero(&f.ty))
                } else if f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_FLATTEN) {
                    format!("tree.decode<{}>()?", serde_type_source(&f.ty))
                } else {
                    let key = serde_source_field_key(&s.serde_markers, f);
                    let subtree = if matches!(f.ty, Type::Option(_)) {
                        format!("(tree.field({key:?}) ?? DataTree.Null)")
                    } else if let Some(default) = serde_source_default(f) {
                        format!("(tree.field({key:?}) ?? {default}.encode())")
                    } else {
                        format!("(tree.field({key:?})?)")
                    };
                    format!("{subtree}.decode<{}>()?", serde_type_source(&f.ty))
                };
                source.push_str(&format!("{}: {},\n", f.name, value));
            }
            source.push_str("})\n}\n}\n");
        }
        let trigger_span = s.derives.iter()
            .find(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE))
            .map(|(_, span)| *span)
            .unwrap_or(s.name_span);
        match parse_builtin_serde_fragment(&source, &s.name, trigger_span, diags) {
            Some(generated) => {
                generated_items.extend(generated.into_iter().filter_map(|item| match item {
                    Item::Impl(mut imp) => {
                        imp.is_generated_serde = true;
                        Some(Item::Impl(imp))
                    }
                    _ => None,
                }));
            }
            None => {}
        }
    }
    items.extend(generated_items);
}

fn expand_builtin_enum_serde(
    e: &mut crate::AST::EnumDef,
    diags: &mut Vec<Diagnostic>,
    generated_items: &mut Vec<Item>,
) {
    let enc = e.derives.iter().any(|(n, _)| n == crate::Generics::ENCODE);
    let dec = e.derives.iter().any(|(n, _)| n == crate::Generics::DECODE);
    if !enc && !dec { return; }
    let mut codec_params = e.type_params.clone();
    let wire_types = e.variants.iter().flat_map(|v| match &v.payload {
        crate::AST::VariantPayload::Unit => Vec::new(),
        crate::AST::VariantPayload::Single(t, _) => vec![t],
        crate::AST::VariantPayload::Named(fs) => fs.iter().map(|f| &f.ty).collect(),
    }).collect::<Vec<_>>();
    for param in &mut codec_params {
        let reaches_wire = wire_types.iter().any(|ty|
            crate::Generics::free_type_params(ty).contains(&param.name));
        if reaches_wire && enc { param.bounds.push(crate::Generics::ENCODE.to_string()); }
        if reaches_wire && dec { param.bounds.push(crate::Generics::DECODE.to_string()); }
    }
    let params = crate::Generics::format_type_params(&codec_params);
    let target = format!("{}{}", e.name, serde_type_arg_names(&e.type_params));
    let tag = e
        .serde_markers
        .iter()
        .find(|m| m.name == crate::Syntax::ATTR_TAG)
        .and_then(marker_static_string);
    let untagged = e.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_UNTAGGED);
    let mut source = String::new();
    if enc {
        source.push_str(&format!("impl {}.Encode {{\nfn encode{params}(self) => DataTree {{\nif self == {{\n", e.name));
        for v in &e.variants {
            let wire = serde_enum_variant_key(v);
            let (pattern, payload) = serde_enum_pattern_and_value(v);
            let value = if untagged {
                match &v.payload {
                    crate::AST::VariantPayload::Unit => "DataTree.Null".to_string(),
                    crate::AST::VariantPayload::Single(..) => "(~v0).encode()".to_string(),
                    crate::AST::VariantPayload::Named(fs) => format!("DataTree.Object([{}])", serde_enum_named_pairs(fs)),
                }
            } else if let Some(tag_key) = &tag {
                match &v.payload {
                    crate::AST::VariantPayload::Unit => format!("DataTree.Object([{tag_key:?}: DataTree.Text({wire:?})])"),
                    crate::AST::VariantPayload::Named(fs) => {
                        let pairs = serde_enum_named_pairs(fs);
                        format!("DataTree.Object([{tag_key:?}: DataTree.Text({wire:?}){}{}])", if pairs.is_empty(){""}else{" ,"}, pairs)
                    }
                    crate::AST::VariantPayload::Single(..) => format!(
                        "DataTree.Object([{tag_key:?}: DataTree.Text({wire:?}), \"value\": {payload}])"
                    ),
                }
            } else {
                match &v.payload {
                    crate::AST::VariantPayload::Unit => format!("DataTree.Text({wire:?})"),
                    crate::AST::VariantPayload::Single(..) => format!("DataTree.Object([{wire:?}: {payload}])"),
                    crate::AST::VariantPayload::Named(fs) => format!("DataTree.Object([{wire:?}: DataTree.Object([{}])])", serde_enum_named_pairs(fs)),
                }
            };
            source.push_str(&format!("{pattern} -> {{ return {value} }}\n"));
        }
        source.push_str("}\n}\n}\n");
    }
    if dec {
        source.push_str(&format!("impl {}.Decode {{\nfn decode{params}(tree: DataTree) => {target} ? DecodeError {{\n", e.name));
        if untagged {
            for v in &e.variants {
                source.push_str(&serde_enum_decode_attempt(&target, v, "tree", true));
            }
        } else if let Some(tag_key) = &tag {
            source.push_str(&format!("tag_tree := tree.field({tag_key:?})?\ntag_value := tag_tree.text()?\n"));
            for v in &e.variants {
                let wire = serde_enum_variant_key(v);
                let payload_source = if matches!(v.payload, crate::AST::VariantPayload::Single(..)) {
                    "(tree.field(\"value\")?)"
                } else {
                    "tree"
                };
                source.push_str(&format!(
                    "if tag_value == {wire:?} {{ {} }}\n",
                    serde_enum_decode_return(&target, v, payload_source)
                ));
            }
        } else {
            let mut object_arms = String::new();
            for (variant_index, v) in e.variants.iter().enumerate() {
                match &v.payload {
                    crate::AST::VariantPayload::Unit => {
                        let wire = serde_enum_variant_key(v);
                        source.push_str(&format!("if (~tree) == .Text(variant_name) {{ if variant_name == {wire:?} {{ return Ok({target}.{}) }} }}\n", v.name));
                    }
                    _ => {
                        let wire = serde_enum_variant_key(v);
                        let candidate = format!("candidate_{variant_index}");
                        object_arms.push_str(&format!(
                            "{candidate} := (~tree).field({wire:?}) ?? DataTree.Null\n"
                        ));
                        match &v.payload {
                            crate::AST::VariantPayload::Single(t, _) => {
                                let decoded = format!("decoded_{variant_index}");
                                object_arms.push_str(&format!("{decoded} := {candidate}.decode<{}>()\nif {decoded} == Ok(decoded_value) {{ return Ok({target}.{}(decoded_value)) }}\n", serde_type_source(t), v.name));
                            }
                            crate::AST::VariantPayload::Named(_) => {
                                object_arms.push_str(&format!("{}\n", serde_enum_decode_return(&target, v, &candidate)));
                            }
                            crate::AST::VariantPayload::Unit => {}
                        }
                    }
                }
            }
            source.push_str(&object_arms);
        }
        source.push_str("return Err(DecodeError.{ path: \"\", reason: \"no matching enum variant\" })\n}\n}\n");
    }
    let trigger_span = e.derives.iter()
        .find(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE))
        .map(|(_, span)| *span)
        .unwrap_or(e.name_span);
    match parse_builtin_serde_fragment(&source, &e.name, trigger_span, diags) {
        Some(generated) => {
            generated_items.extend(generated.into_iter().filter_map(|item| match item {
                Item::Impl(mut imp) => {
                    imp.is_generated_serde = true;
                    Some(Item::Impl(imp))
                }
                _ => None,
            }));
        }
        None => {}
    }
}

fn parse_builtin_serde_fragment(
    source: &str,
    type_name: &str,
    trigger_span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Vec<Item>> {
    let (tokens, lex_diags) = crate::Lexer::lex(source);
    let parsed = if lex_diags.is_empty() {
        crate::Parser::parse(&tokens)
    } else {
        Err(lex_diags)
    };
    match parsed {
        Ok(generated) => Some(generated.items),
        Err(errors) => {
            let detail = errors
                .first()
                .map(|d| format!("{} at {:?}", d.what, d.span))
                .unwrap_or_else(|| "generated codec source was invalid".to_string());
            diags.push(Diagnostic::error(
                "E2710",
                format!("built-in codec derive generated invalid Jet for `{type_name}`"),
                format!(
                    "generated source did not pass the ordinary lexer and parser: {detail}; generated source:\n{source}"
                ),
                "report this compiler bug; built-in derives must emit valid ordinary Jet".to_string(),
                Some(trigger_span),
            ));
            None
        }
    }
}

#[cfg(test)]
mod serde_source_tests {
    use super::*;

    #[test]
    fn builtin_codecs_remain_parsed_top_level_impls() {
        let source = "#Codable\nstruct Point { x: Int }\n";
        let (tokens, lex_diags) = crate::Lexer::lex(source);
        assert!(lex_diags.is_empty());
        let mut program = crate::Parser::parse(&tokens).expect("source parses");
        let mut diags = Vec::new();
        expand_builtin_serde_items(&mut program.items, &mut diags);
        assert!(diags.is_empty(), "generated source must parse: {diags:?}");

        let point = program.items.iter().find_map(|item| match item {
            Item::Struct(s) if s.name == "Point" => Some(s),
            _ => None,
        }).expect("real type remains");
        assert!(point.trait_impls.is_empty(), "no parsed block may be transplanted into the type");
        assert!(!program.items.iter().any(|item| match item {
            Item::Struct(s) => s.name.starts_with("__JetSerde"),
            Item::Enum(e) => e.name.starts_with("__JetSerde"),
            _ => false,
        }));
        let protocols: Vec<_> = program.items.iter().filter_map(|item| match item {
            Item::Impl(i) if i.type_name == "Point" => i.trait_name.as_deref(),
            _ => None,
        }).collect();
        assert_eq!(protocols, vec!["Encode", "Decode"]);
    }

    #[test]
    fn malformed_builtin_codec_points_at_derive_trigger() {
        let trigger = Span::new(17, 26);
        let mut diags = Vec::new();
        assert!(parse_builtin_serde_fragment(
            "impl Broken.Encode { fn encode(self) => DataTree {",
            "Broken",
            trigger,
            &mut diags,
        ).is_none());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E2710");
        assert_eq!(diags[0].span, Some(trigger));
    }
}

fn serde_enum_variant_key(v: &crate::AST::Variant) -> String {
    v.serde_markers.iter().find(|m| m.name == crate::Syntax::ATTR_RENAME)
        .and_then(marker_static_string).unwrap_or_else(|| v.name.clone())
}

fn marker_static_string(marker: &crate::AST::Marker) -> Option<String> {
    if let Some(crate::AST::CtValue::Str(value)) = &marker.ct {
        return Some(value.clone());
    }
    marker.args.first().and_then(|expression| match expression {
        crate::AST::Expr::Str(parts, _) => parts.first().and_then(|part| match part {
            crate::AST::StrPart::Lit(value) => Some(value.clone()),
            crate::AST::StrPart::Interp(..) => None,
        }),
        _ => None,
    })
}

fn serde_enum_pattern_and_value(v: &crate::AST::Variant) -> (String, String) {
    match &v.payload {
        crate::AST::VariantPayload::Unit => (format!(".{}", v.name), String::new()),
        crate::AST::VariantPayload::Single(..) => (format!(".{}(v0)", v.name), "(~v0).encode()".to_string()),
        crate::AST::VariantPayload::Named(fs) => {
            let names = (0..fs.len()).map(|i| format!("v{i}")).collect::<Vec<_>>();
            (format!(".{}({})", v.name, names.join(", ")), String::new())
        }
    }
}

fn serde_enum_named_pairs(fs: &[crate::AST::VariantField]) -> String {
    fs.iter().enumerate().map(|(i, f)| format!("{:?}: (~v{i}).encode()", f.name)).collect::<Vec<_>>().join(", ")
}

fn serde_enum_decode_attempt(target: &str, v: &crate::AST::Variant, src: &str, guarded: bool) -> String {
    if guarded { format!("if {src}.decode<{}>() == Ok(v0) {{ {} }}\n", serde_enum_payload_type(v), serde_enum_decode_return(target, v, src)) }
    else { serde_enum_decode_return(target, v, src) }
}

fn serde_enum_decode_return(target: &str, v: &crate::AST::Variant, src: &str) -> String {
    let cons = serde_enum_decode_constructor(target, v, src);
    if matches!(v.payload, crate::AST::VariantPayload::Named(_)) {
        format!("decoded_variant: {target} := {cons}\nreturn Ok(decoded_variant)")
    } else {
        format!("return Ok({cons})")
    }
}

fn serde_enum_payload_type(v: &crate::AST::Variant) -> String {
    match &v.payload {
        crate::AST::VariantPayload::Unit => "DataTree".to_string(),
        crate::AST::VariantPayload::Single(t, _) => serde_type_source(t),
        crate::AST::VariantPayload::Named(_) => "DataTree".to_string(),
    }
}

fn serde_enum_decode_constructor(target: &str, v: &crate::AST::Variant, src: &str) -> String {
    match &v.payload {
        crate::AST::VariantPayload::Unit => format!("{target}.{}", v.name),
        crate::AST::VariantPayload::Single(t, _) => format!("{target}.{}({src}.decode<{}>()?)", v.name, serde_type_source(t)),
        crate::AST::VariantPayload::Named(fs) => format!(".{}.{{ {} }}", v.name, fs.iter().map(|f| format!("{}: ((~{src}).field({:?})?).decode<{}>()?", f.name, f.name, serde_type_source(&f.ty))).collect::<Vec<_>>().join(", ")),
    }
}

fn serde_source_field_key(container: &[crate::AST::Marker], f: &crate::AST::Field) -> String {
    if let Some(marker) = f.serde_markers.iter().find(|m| m.name == crate::Syntax::ATTR_RENAME) {
        if let Some(value) = marker_static_string(marker) {
            return value;
        }
    }
    let style = container.iter().find(|m| m.name == crate::Syntax::ATTR_RENAME_ALL)
        .and_then(|m| m.args.first()).and_then(|e| match e { crate::AST::Expr::Ident(n, _) => Some(n.as_str()), _ => None });
    match style {
        Some("camel") => crate::Syntax::to_camel_acronym(&f.name),
        Some("kebab") => crate::Syntax::to_snake_acronym(&f.name).replace('_', "-"),
        Some("screaming") => crate::Syntax::to_shouty_acronym(&f.name),
        Some("pascal") => crate::Syntax::to_pascal_acronym(&f.name),
        Some("snake") => crate::Syntax::to_snake_acronym(&f.name),
        _ => f.name.clone(),
    }
}

/// Render direct object literals for every optional-field path. Direct literals
/// lower straight to DataTree's ordered pair vector; routing an omitted field
/// through Jet's ordinary key-sorted Map would lose declaration order. Each
/// leaf keeps the remaining fields in source order and leaves absent options
/// off the wire.
fn serde_ordered_object_source(
    container: &[crate::AST::Marker],
    fields: &[&crate::AST::Field],
) -> String {
    fn emit(
        container: &[crate::AST::Marker],
        fields: &[&crate::AST::Field],
        index: usize,
        pairs: &mut Vec<String>,
        out: &mut String,
    ) {
        let Some(field) = fields.get(index) else {
            out.push_str(&format!("return DataTree.Object([{}])\n", pairs.join(", ")));
            return;
        };
        let key = serde_source_field_key(container, field);
        if matches!(field.ty, Type::Option(_)) {
            let value = format!("serde_value_{index}");
            out.push_str(&format!("if self.{} == Val({value}) {{\n", field.name));
            pairs.push(format!("{key:?}: (~{value}).encode()"));
            emit(container, fields, index + 1, pairs, out);
            pairs.pop();
            out.push_str("} else {\n");
            emit(container, fields, index + 1, pairs, out);
            out.push_str("}\n");
        } else {
            pairs.push(format!("{key:?}: self.{}.encode()", field.name));
            emit(container, fields, index + 1, pairs, out);
            pairs.pop();
        }
    }

    let mut out = String::new();
    emit(container, fields, 0, &mut Vec::new(), &mut out);
    out
}

fn serde_source_default(f: &crate::AST::Field) -> Option<String> {
    let marker = f.serde_markers.iter().find(|m| m.name == crate::Syntax::ATTR_DEFAULT)?;
    match (marker.args.first(), marker.ct.as_ref()) {
        (Some(_), Some(value)) => serde_ct_source(value),
        (Some(expr), None) => serde_source_literal(expr),
        (None, _) => None,
    }
}

fn serde_ct_source(value: &crate::AST::CtValue) -> Option<String> {
    use crate::AST::CtValue;
    Some(match value {
        CtValue::Int(v) => v.to_string(),
        CtValue::Float(v) => format!("{v:?}"),
        CtValue::Bool(v) => v.to_string(),
        CtValue::Char(v) => format!("{v:?}"),
        CtValue::Str(v) => format!("{v:?}"),
        CtValue::BigInt(v) => format!("BigInt({:?})", v.to_string_rep()),
        CtValue::Bytes(values) => format!(
            "[{}]",
            values.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
        ),
        CtValue::List(values) => format!(
            "[{}]",
            values.iter().map(serde_ct_source).collect::<Option<Vec<_>>>()?.join(", ")
        ),
        CtValue::Map(values) => format!(
            "[{}]",
            values.iter().map(|(key, value)| Some(format!(
                "{}: {}",
                serde_ct_source(&key.to_value())?,
                serde_ct_source(value)?
            ))).collect::<Option<Vec<_>>>()?.join(", ")
        ),
        CtValue::Struct { type_name, fields } => format!(
            "{type_name}.{{ {} }}",
            fields.iter().map(|(name, value)| Some(format!(
                "{name}: {}",
                serde_ct_source(value)?
            ))).collect::<Option<Vec<_>>>()?.join(", ")
        ),
        CtValue::Enum { type_name, variant, args } => {
            if args.is_empty() {
                format!("{type_name}.{variant}")
            } else if args.iter().all(|(label, _)| label.is_none()) {
                format!(
                    "{type_name}.{variant}({})",
                    args.iter().map(|(_, value)| serde_ct_source(value)).collect::<Option<Vec<_>>>()?.join(", ")
                )
            } else {
                format!(
                    "{type_name}.{variant}.{{ {} }}",
                    args.iter().map(|(label, value)| Some(format!(
                        "{}: {}",
                        label.as_ref()?,
                        serde_ct_source(value)?
                    ))).collect::<Option<Vec<_>>>()?.join(", ")
                )
            }
        }
        CtValue::Some(value) => format!("Val({})", serde_ct_source(value)?),
        CtValue::None(_) => "None".to_string(),
        CtValue::ResOk(value) => format!("Ok({})", serde_ct_source(value)?),
        CtValue::ResErr(value) => format!("Err({})", serde_ct_source(value)?),
        CtValue::Unit | CtValue::Closure(_) => return None,
    })
}

fn serde_source_literal(e: &crate::AST::Expr) -> Option<String> {
    match e {
        crate::AST::Expr::Int(v, _, _, _) => Some(v.to_string()),
        crate::AST::Expr::Float(v, _, _) => Some(v.to_string()),
        crate::AST::Expr::Bool(v, _) => Some(v.to_string()),
        crate::AST::Expr::Char(v, _) => Some(format!("{v:?}")),
        crate::AST::Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            crate::AST::StrPart::Lit(v) => Some(format!("{v:?}")),
            _ => None,
        },
        crate::AST::Expr::ListLit(values, _) => Some(format!("[{}]", values.iter().map(serde_source_literal).collect::<Option<Vec<_>>>()?.join(", "))),
        crate::AST::Expr::MapLit(values, _) => Some(format!("[{}]", values.iter().map(|(k,v)| Some(format!("{}: {}", serde_source_literal(k)?, serde_source_literal(v)?))).collect::<Option<Vec<_>>>()?.join(", "))),
        _ => None,
    }
}

fn serde_source_zero(ty: &Type) -> String {
    match ty {
        Type::Int | Type::IntN { .. } => "0".to_string(),
        Type::Float | Type::Float32 => "0.0".to_string(),
        Type::Bool => "false".to_string(),
        Type::String => "\"\"".to_string(),
        Type::Option(_) => "None".to_string(),
        Type::List(_) | Type::Map { .. } => "[]".to_string(),
        _ => format!("{}.{{}}", serde_type_source(ty)),
    }
}

fn serde_type_arg_names(params: &[crate::AST::TypeParam]) -> String {
    if params.is_empty() { String::new() } else {
        format!("<{}>", params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "))
    }
}

fn serde_type_source(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".to_string(), Type::Float => "Float".to_string(),
        Type::Bool => "Bool".to_string(), Type::String => "String".to_string(),
        Type::Char => "Char".to_string(), Type::Named(n) => n.clone(),
        Type::List(t) => format!("[{}]", serde_type_source(t)),
        Type::Map { key, value, .. } => format!("[{}: {}]", serde_type_source(key), serde_type_source(value)),
        Type::Option(t) => format!("{}?", serde_type_source(t)),
        Type::Result { ok, err } => format!("{} ? {}", serde_type_source(ok), serde_type_source(err)),
        Type::Apply { name, args } => format!("{}<{}>", name, args.iter().map(serde_type_source).collect::<Vec<_>>().join(", ")),
        Type::IntN { signed, bits } => format!("{}{}", if *signed { "I" } else { "U" }, bits),
        Type::Float32 => "F32".to_string(),
        Type::FixedList { elem, len, .. } => format!("[{}#{}]", serde_type_source(elem), len),
        Type::Shared(t) => format!("shared {}", serde_type_source(t)),
        Type::Tagged { marker, inner } => format!("#{} {}", marker, serde_type_source(inner)),
        Type::Tuple(fields) => format!("({})", fields.iter().map(|(n,t)| format!("{}: {}", n, serde_type_source(t))).collect::<Vec<_>>().join(", ")),
        Type::TraitObject(names) => format!("dyn {}", names.join(" + ")),
        Type::Fn { .. } => "fn()".to_string(),
        Type::Union(members) => members
            .iter()
            .map(serde_type_source)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}
