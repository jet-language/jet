impl<'a> Checker<'a> {
        pub(crate) fn check_core_json_lit(
            &mut self,
            variant: &str,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) -> Option<Type> {
            let json = json_ty();
            let expected = match variant {
                "Null" => Vec::new(),
                "Bool" => vec![Type::Bool],
                "Int" => vec![Type::Int],
                "Float" => vec![Type::Float],
                "Text" => vec![Type::String],
                "Array" => vec![Type::List(Box::new(json.clone()))],
                "Object" => vec![Type::Map {
                    key: Box::new(Type::String),
                    value: Box::new(json.clone()),
                }],
                _ => {
                    let candidates = ["Null", "Bool", "Int", "Float", "Text", "Array", "Object"]
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>();
                    let mut fix = "check the variant name".to_string();
                    if let Some(s) = suggest_field(variant, &candidates) {
                        fix = format!("did you mean `{}`?", s);
                    }
                    self.diags.push(Diagnostic::error(
                        "E0304",
                        format!("`{}` has no variant `{}`", Syntax::TYPE_DATA, variant),
                        "the dynamic `Data` value exposes Null/Bool/Int/Float/Text/Array/Object"
                            .to_string(),
                        fix,
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(json);
                }
            };
            if args.len() != expected.len() {
                self.diags.push(Diagnostic::error(
                    "E0306",
                    format!(
                        "`{}.{}` expects {} value{}, got {}",
                        Syntax::TYPE_DATA,
                        variant,
                        expected.len(),
                        if expected.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "each `Data` variant has a fixed payload (Bool/Int/Float→scalar, Text→String, Array→[Data], Object→Map)".to_string(),
                    "check the variant payload".to_string(),
                    Some(span),
                ));
            }
            for (i, arg) in args.iter_mut().enumerate() {
                if let Some(want) = expected.get(i) {
                    self.expect_core_arg_consuming(variant, i, want, arg);
                } else {
                    self.infer(&mut arg.expr);
                }
            }
            Some(json)
        }
    
        /// D-DBDRIVER1: `DbValue.Null` / `.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)`
        /// — the tagged SQL parameter/column value construction. Mirrors
        /// `check_core_json_lit` exactly (same dynamic-value mechanism, SQL-shaped
        /// variants); `Int` stays `Type::Int` (64-bit), never widened through `Float`.
        pub(crate) fn check_core_dbvalue_lit(
            &mut self,
            variant: &str,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) -> Option<Type> {
            let dbvalue = Type::Named(Syntax::TYPE_DB_VALUE.to_string());
            let expected = match variant {
                "Null" => Vec::new(),
                "Int" => vec![Type::Int],
                "Float" => vec![Type::Float],
                "Text" => vec![Type::String],
                "Bool" => vec![Type::Bool],
                _ => {
                    let candidates = ["Null", "Int", "Float", "Text", "Bool"]
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>();
                    let mut fix = "check the variant name".to_string();
                    if let Some(s) = suggest_field(variant, &candidates) {
                        fix = format!("did you mean `{}`?", s);
                    }
                    self.diags.push(Diagnostic::error(
                        "E0304",
                        format!("`{}` has no variant `{}`", Syntax::TYPE_DB_VALUE, variant),
                        "`DbValue` is the tagged SQL parameter/column value: Null/Int/Float/Text/Bool"
                            .to_string(),
                        fix,
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(dbvalue);
                }
            };
            if args.len() != expected.len() {
                self.diags.push(Diagnostic::error(
                    "E0306",
                    format!(
                        "`{}.{}` expects {} value{}, got {}",
                        Syntax::TYPE_DB_VALUE,
                        variant,
                        expected.len(),
                        if expected.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "each `DbValue` variant has a fixed payload (Int→64-bit int, Float→float, Text→String, Bool→bool, Null→none)".to_string(),
                    "check the variant payload".to_string(),
                    Some(span),
                ));
            }
            for (i, arg) in args.iter_mut().enumerate() {
                if let Some(want) = expected.get(i) {
                    self.expect_core_arg_consuming(variant, i, want, arg);
                } else {
                    self.infer(&mut arg.expr);
                }
            }
            Some(dbvalue)
        }
    
}
