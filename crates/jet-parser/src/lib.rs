#![allow(non_snake_case)]
#![deny(warnings)]
#![allow(unreachable_patterns)]
// Re-export foundation + lexer so `crate::AST`, `crate::Lexer` etc. work in Parser/Formatter.
pub use jet_lexer::{
    CanonicalAST, Collections, Diagnostics, Generics, Lexer, Numeric, Policy, Registry, Syntax,
    TargetMachine, Traits, AST, SHA256,
};
mod FencedNames;
pub mod Formatter;
pub mod Parser;

#[cfg(test)]
mod generic_module_tests {
    use super::{
        Formatter, Lexer, Parser, Syntax, AST,
        AST::{Expr, GenericModuleParam, Item, ModuleArg, Pattern, Stmt, Type},
    };

    #[test]
    fn generic_module_slots_remain_unresolved_until_sema_without_casing_heuristics() {
        let src="module Weird<lower>(UPPER: Int) { fn ready() Bool -> { return true } }\nmodule Use :: Weird<String>(32)";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let program = Parser::parse(&tokens).unwrap();
        let Item::GenericModule(def) = &program.items[0] else {
            panic!("template")
        };
        assert!(
            matches!(&def.params[0],GenericModuleParam::Type{name,bound,..}if name=="lower"&&bound.is_none())
        );
        assert!(matches!(&def.params[1],GenericModuleParam::Value{name,..}if name=="UPPER"));
        let Item::ModuleAlias(alias) = &program.items[1] else {
            panic!("alias")
        };
        assert!(matches!(&alias.args[0], ModuleArg::Type(..)));
        assert!(matches!(&alias.args[1], ModuleArg::Value(..)));
    }

    #[test]
    fn generic_module_value_slots_parse_closed_identifier_led_expressions() {
        let src = "module retry(count: Int) { fn ready() Bool -> { return true } }\nmodule a :: retry(limit + 1)\nmodule b :: retry(compute())";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let program = Parser::parse(&tokens).unwrap();
        for item in &program.items[1..] {
            let Item::ModuleAlias(alias) = item else {
                panic!("alias")
            };
            assert!(matches!(&alias.args[0], ModuleArg::Value(..)));
        }
    }

    #[test]
    fn generic_module_retired_mixed_angle_value_slot_teaches_parentheses() {
        let src = "module cache<K, capacity: Int> { fn size() Int :: capacity }";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("retired mixed parameter spelling");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E0003"
                    && diagnostic.what.contains("parentheses")
                    && diagnostic.fix.contains("capacity: Int")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn generic_module_retired_equals_alias_teaches_coloncolon() {
        let src = "module cache<K>(capacity: Int) { fn size() Int :: capacity }\nmodule old = cache<Int>(64)";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("retired equals alias spelling");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E0003"
                    && diagnostic.what.contains("::")
                    && diagnostic.fix.contains("module old :: Target")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn generic_module_retains_symbolic_fixed_length_and_nested_modules() {
        let src = "module buffer<T>(capacity: Int) { struct Data { items: [T#capacity] } module stats { fn size() Int -> { return capacity } } }";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let program = Parser::parse(&tokens).unwrap();
        let Item::GenericModule(def) = &program.items[0] else {
            panic!("template")
        };
        let Item::Struct(data) = &def.body[0] else {
            panic!("struct")
        };
        assert!(
            matches!(&data.fields[0].ty, Type::FixedList { len, .. } if len.symbol_name() == Some("capacity"))
        );
        assert!(matches!(&def.body[1], Item::CodeModule(module) if module.name == "stats"));
    }

    #[test]
    fn shared_prefix_accepts_inferred_brace_values() {
        let source = "fn run() { value :: shared { count: 1 } }\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        Parser::parse(&tokens).expect("shared must accept an inferred brace value");
    }

    #[test]
    fn script_statements_survive_items_and_formatter() {
        let source = "print(\"first\")\nfn helper() {}\nprint(\"last\")\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let program = Parser::parse(&tokens).expect("script should parse");
        assert_eq!(program.script_body.len(), 2);
        assert!(
            program.script_body[0].span().start < program.script_body[1].span().start,
            "loose statements must retain authored source order"
        );
        assert!(program.items.iter().any(|item| matches!(
            item,
            Item::Func(function) if function.name == "helper"
        )));
        let formatted = Formatter::format_source(source).expect("script should format");
        assert!(formatted.contains("print(\"first\")"), "{formatted}");
        assert!(formatted.contains("print(\"last\")"), "{formatted}");
        assert!(
            !formatted.contains("fn run"),
            "formatter must keep script syntax: {formatted}"
        );
    }

    #[test]
    fn result_handler_desugars_to_ok_and_err_value_tests() {
        let source = "fn pick(value: Int !String) String -> value ? ok -> ok ! error -> error\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let program = Parser::parse(&tokens).expect("Result handler should parse");
        let Item::Func(function) = &program.items[0] else {
            panic!("function")
        };
        let Stmt::Return(
            Some(Expr::If {
                cond, else_value, ..
            }),
            _,
        ) = &function.body[0]
        else {
            panic!("Result handler should lower as a value if")
        };
        assert!(matches!(
            cond.as_ref(),
            Expr::PatternTest {
                pattern: Pattern::Ok { binding, .. },
                ..
            } if binding == "ok"
        ));
        let Expr::If { cond: err_cond, .. } = else_value.as_ref() else {
            panic!("Result handler must retain its failure branch")
        };
        assert!(matches!(
            err_cond.as_ref(),
            Expr::PatternTest {
                pattern: Pattern::Err { binding, .. },
                ..
            } if binding == "error"
        ));
    }

    #[test]
    fn result_handler_accepts_multiline_and_effectful_branches() {
        let source = "fn run(value: Int !String) ! {\n    value ? ok -> print(ok) // success\n    ! error -> print(error)\n}\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let program = Parser::parse(&tokens).expect("multiline Result handler should parse");
        let Item::Func(function) = &program.items[0] else {
            panic!("function")
        };
        assert!(matches!(
            &function.body[0],
            Stmt::Expr(Expr::If { cond, else_value, .. })
                if matches!(
                    cond.as_ref(),
                    Expr::PatternTest {
                        subject,
                        pattern: Pattern::Ok { binding, .. },
                        ..
                    } if matches!(subject.as_ref(), Expr::Ident(name, _) if name == "value")
                        && binding == "ok"
                ) && matches!(
                    else_value.as_ref(),
                    Expr::If {
                        cond: err_cond,
                        then_body,
                        then_value,
                        ..
                    } if matches!(
                        err_cond.as_ref(),
                        Expr::PatternTest {
                            pattern: Pattern::Err { binding, .. },
                            ..
                        } if binding == "error"
                    ) && then_body.is_empty()
                        && matches!(
                            then_value.as_ref(),
                            Expr::Call(call) if call.name == "print"
                        )
                )
        ));
    }

    #[test]
    fn result_handler_allows_nested_value_handlers() {
        let source = "fn pick(value: Int !String) String -> value ? ok -> ok ? inner -> inner ! inner_error -> inner_error ! error -> error\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        Parser::parse(&tokens).expect("nested Result handlers should parse");
    }

    #[test]
    fn result_handler_stays_distinct_from_neighboring_postfix_and_branch_forms() {
        let source = r#"
fn optional() ?Success -> None
fn fallible() !Error -> Err("bad")
fn result() ?Success !Error -> Ok(1)
fn run(value: Int!, optional: String) {
    propagated :: value?
    noted :: value?("context")
    chained :: optional?.len
    negated :: !true
    if value == {
        .Ok(ok) -> print(ok)
        .Err(error) -> print(error)
    }
    fallback :: value ?? 0
}
"#;
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        Parser::parse(&tokens).expect("neighboring Result syntax should keep its meanings");
    }

    #[test]
    fn result_handler_reports_fixed_shape_errors_at_the_offending_token() {
        let cases = [
            (
                "fn pick(value: Int !String) String -> value ? ok -> ok\n",
                "a Result handler needs a `!` failure branch",
                "E0003",
            ),
            (
                "fn pick(value: Int !String) String -> value ? same -> same ! same -> same\n",
                "a Result handler cannot bind both payloads to the same name",
                "E0003",
            ),
            (
                "fn pick(value: Int !String) String -> value ? _ -> value ! error -> error\n",
                "a Result handler success branch needs a payload binding",
                "E0003",
            ),
            (
                "fn pick(value: Int !String) String -> value ? ok => ok ! error -> error\n",
                "this uses a retired arrow spelling",
                "E0070",
            ),
            (
                "fn pick(value: Int !String) String -> value ? ok -> ok ! error -> error ! again -> again\n",
                "a Result handler cannot have two failure branches",
                "E0003",
            ),
        ];
        for (source, expected, code) in cases {
            let (tokens, lexer_diagnostics) = Lexer::lex(source);
            assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
            let diagnostics = Parser::parse(&tokens).expect_err("invalid handler should fail");
            let diagnostic = diagnostics
                .iter()
                .find(|diagnostic| diagnostic.what == expected)
                .unwrap_or_else(|| panic!("missing {expected:?} in {diagnostics:?}"));
            assert_eq!(diagnostic.code, code);
            assert!(diagnostic.span.is_some(), "diagnostic needs an exact span");
            assert!(!diagnostic.why.is_empty());
            assert!(!diagnostic.fix.is_empty());
        }
    }

    #[test]
    fn callable_policy_marker_keeps_one_typed_wrapper_chain() {
        let source = "#Policy(retry(3), trace(\"users.load\"))\nfn load_user(id: Int) Int -> { return id }\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let program = Parser::parse(&tokens).expect("callable policy marker should parse");
        let Item::Func(function) = &program.items[0] else {
            panic!("function")
        };
        let marker = function
            .markers
            .iter()
            .find(|marker| marker.name == Syntax::MARKER_POLICY)
            .expect("policy marker");
        assert!(matches!(&marker.args[0], AST::Expr::Call(call) if call.name == "retry"));
        assert!(matches!(&marker.args[1], AST::Expr::Call(call) if call.name == "trace"));
    }

    #[test]
    fn retired_memory_policy_names_point_to_effect_denials() {
        for (source, replacement) in [
            ("#Policy(no_alloc)\nfn run() {}\n", "!Mem.Alloc"),
            ("#Policy(zero_rc)\nfn run() {}\n", "!Mem.Rc"),
            (
                "#Policy(arena_bounded(65536))\nfn run() {}\n",
                "!Mem.Alloc(above: 65536)",
            ),
        ] {
            let (tokens, lexer_diagnostics) = Lexer::lex(source);
            assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
            let diagnostics = Parser::parse(&tokens).expect_err("memory floor words are retired");
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E0355" && diagnostic.fix.contains(replacement)
            }));
        }
    }

    #[test]
    fn duplicate_callable_policy_markers_are_rejected() {
        let source = "#Policy(trace(\"one\"))\n#Policy(trace(\"two\"))\nfn run() {}\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("duplicate policy marker");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0355"));
    }
}

#[cfg(test)]
mod compare_tests {
    use super::{Lexer, Parser};

    #[test]
    fn separated_le_and_gt_still_reject_as_an_invalid_expression() {
        let (tokens, lex_diagnostics) = Lexer::lex("fn run() { return a <= > b }");
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
        assert!(Parser::parse(&tokens).is_err());
    }
}

#[cfg(test)]
mod loop_header_tests {
    use super::{Lexer, Parser, AST};

    #[test]
    fn yielding_loop_keeps_numeric_stride_after_source() {
        let source = "fn run() {\n    values :: loop i in 0..10, 2 -> i\n}\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let program = Parser::parse(&tokens).expect("genuine stride should parse");
        let function = program
            .items
            .iter()
            .find_map(|item| match item {
                AST::Item::Func(function) if function.name == "run" => Some(function),
                _ => None,
            })
            .expect("run function");
        let binding = function
            .body
            .iter()
            .find_map(|stmt| match stmt {
                AST::Stmt::Val(binding) => Some(binding),
                _ => None,
            })
            .expect("loop binding");
        let AST::Expr::CallValue { callee, .. } = &binding.init else {
            panic!("expected collecting loop call, got {:?}", binding.init);
        };
        let AST::Expr::Lambda(lambda) = callee.as_ref() else {
            panic!("expected collecting loop lambda, got {callee:?}");
        };
        let AST::LambdaBody::Block(body) = &lambda.body else {
            panic!("expected collecting loop block");
        };
        let Some(AST::Stmt::For { kind, .. }) = body.first() else {
            panic!("expected one source loop, got {body:?}");
        };
        let AST::ForKind::Range { step, .. } = kind else {
            panic!("expected range source, got {kind:?}");
        };
        let Some(AST::Expr::Int(value, ..)) = step.as_ref() else {
            panic!("expected numeric stride, got {step:?}");
        };
        assert_eq!(*value, 2);
    }

    #[test]
    fn source_loops_use_in_for_bindings_and_keep_stride_comma() {
        let source = r#"
fn run() {
    loop x in xs { print(x) }
    loop (key, value) in map { print(key, value) }
    loop n in 0..10, 2 { print(n) }
    filtered :: loop x in xs if x > 0 -> x
}
"#;
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        Parser::parse(&tokens).expect("canonical source-loop forms should parse");
    }

    #[test]
    fn retired_source_loop_comma_teaches_in() {
        let source = "fn run() { loop x, xs { print(x) } }\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("retired source-loop comma");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E0383"
                    && diagnostic.what.contains("retired comma")
                    && diagnostic.why.contains("`in`")
                    && diagnostic.fix.contains("loop item in items")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn expression_membership_teaches_contains() {
        let source = "fn run() { found :: x in xs }\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("membership is not an operator");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E0384"
                    && diagnostic.what.contains("membership")
                    && diagnostic.fix.contains(".contains(x)")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn in_is_reserved_for_source_loops() {
        let source = "fn in() {}\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("`in` must not be an identifier");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E0003"
                    && diagnostic.what.contains("reserved")
                    && diagnostic.fix.contains("in")
            }),
            "{diagnostics:?}"
        );
    }
}

#[cfg(test)]
mod raw_head_fmt_tests {
    use super::Formatter;

    #[test]
    fn typed_head_bodies_keep_raw_backslashes() {
        let src = r#"fn run() {
    digits :: Regex{"\d+"}
    text :: "a\nb"
    loc :: URL.{"https://x/{name}"}
}
"#;
        let once = Formatter::format_source(src).expect("typed head should format");
        assert!(
            once.contains(r#"Regex{"\d+"}"#),
            "formatter decoded a typed-head slash:\n{once}"
        );
        assert!(
            once.contains(r#""a\nb""#),
            "formatter lost the plain-string newline escape:\n{once}"
        );
        assert!(
            once.contains("{name}"),
            "formatter dropped a typed-head hole:\n{once}"
        );
        let twice = Formatter::format_source(&once).expect("typed head should reformat");
        assert_eq!(once, twice, "typed-head formatting must be idempotent");
    }
}
