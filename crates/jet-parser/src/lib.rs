#![allow(non_snake_case)]
#![deny(warnings)]
#![allow(unreachable_patterns)]
// Re-export foundation + lexer so `crate::AST`, `crate::Lexer` etc. work in Parser/Formatter.
pub use jet_lexer::{
    CanonicalAST, Collections, Diagnostics, Generics, Lexer, Numeric, Syntax, TargetMachine,
    Traits, Policy, AST, SHA256,
};
mod FencedNames;
pub mod Formatter;
pub mod Parser;

#[cfg(test)]
mod generic_module_tests {
    use super::{AST, AST::{GenericModuleParam, Item, ModuleArg, Type}, Formatter, Lexer, Parser};

    #[test]
    fn generic_module_slots_remain_unresolved_until_sema_without_casing_heuristics() {
        let src="module Weird<lower>(UPPER: Int) { fn ready() => Bool { return true } }\nmodule Use :: Weird<String>(32)";
        let (tokens,lex)=Lexer::lex(src);assert!(lex.is_empty(),"{lex:?}");let program=Parser::parse(&tokens).unwrap();
        let Item::GenericModule(def)=&program.items[0]else{panic!("template")};
        assert!(matches!(&def.params[0],GenericModuleParam::Type{name,bound,..}if name=="lower"&&bound.is_none()));
        assert!(matches!(&def.params[1],GenericModuleParam::Value{name,..}if name=="UPPER"));
        let Item::ModuleAlias(alias)=&program.items[1]else{panic!("alias")};
        assert!(matches!(&alias.args[0],ModuleArg::Type(..)));assert!(matches!(&alias.args[1],ModuleArg::Value(..)));
    }

    #[test]
    fn generic_module_value_slots_parse_closed_identifier_led_expressions() {
        let src = "module retry(count: Int) { fn ready() => Bool { return true } }\nmodule a :: retry(limit + 1)\nmodule b :: retry(compute())";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let program = Parser::parse(&tokens).unwrap();
        for item in &program.items[1..] {
            let Item::ModuleAlias(alias) = item else { panic!("alias") };
            assert!(matches!(&alias.args[0], ModuleArg::Value(..)));
        }
    }

    #[test]
    fn generic_module_retired_mixed_angle_value_slot_teaches_parentheses() {
        let src = "module cache<K, capacity: Int> { fn size() => Int = capacity }";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("retired mixed parameter spelling");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0003"
                && diagnostic.what.contains("parentheses")
                && diagnostic.fix.contains("capacity: Int")
        }), "{diagnostics:?}");
    }

    #[test]
    fn generic_module_retains_symbolic_fixed_length_and_nested_modules() {
        let src = "module buffer<T>(capacity: Int) { struct Data { items: [T#capacity] } module stats { fn size() => Int { return capacity } } }";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let program = Parser::parse(&tokens).unwrap();
        let Item::GenericModule(def) = &program.items[0] else { panic!("template") };
        let Item::Struct(data) = &def.body[0] else { panic!("struct") };
        assert!(matches!(&data.fields[0].ty, Type::FixedList { len_expr: Some(expr), .. } if matches!(expr.as_ref(), super::AST::Expr::Ident(name, _) if name == "capacity")));
        assert!(matches!(&def.body[1], Item::CodeModule(module) if module.name == "stats"));
    }

    #[test]
    fn script_statements_survive_items_and_formatter() {
        let source = "print(\"first\")\nfn helper() {}\nprint(\"last\")\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let program = Parser::parse(&tokens).expect("script should parse");
        assert_eq!(program.script_body.len(), 2);
        assert!(program.items.iter().any(|item| matches!(
            item,
            Item::Func(function) if function.name == "helper"
        )));
        let formatted = Formatter::format_source(source).expect("script should format");
        assert!(formatted.contains("print(\"first\")"), "{formatted}");
        assert!(formatted.contains("print(\"last\")"), "{formatted}");
        assert!(!formatted.contains("fn run"), "formatter must keep script syntax: {formatted}");
    }

    #[test]
    fn callable_policy_marker_keeps_one_typed_wrapper_chain() {
        let source = "#Policy(retry(3), trace(\"users.load\"))\nfn load_user(id: Int) => Int { return id }\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let program = Parser::parse(&tokens).expect("callable policy marker should parse");
        let Item::Func(function) = &program.items[0] else { panic!("function") };
        let marker = function
            .markers
            .iter()
            .find(|marker| marker.name == "Policy")
            .expect("policy marker");
        assert!(matches!(&marker.args[0], AST::Expr::Call(call) if call.name == "retry"));
        assert!(matches!(&marker.args[1], AST::Expr::Call(call) if call.name == "trace"));
    }

    #[test]
    fn retired_scoped_policy_shape_is_not_a_callable_policy_alias() {
        let source = "#Policy(no_alloc)\nfn run() {}\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let program = Parser::parse(&tokens).expect("legacy scoped policy is separate during migration");
        assert_eq!(program.policy_declarations.len(), 1);
    }

    #[test]
    fn duplicate_callable_policy_markers_are_rejected() {
        let source = "#Policy(trace(\"one\"))\n#Policy(trace(\"two\"))\nfn run() {}\n";
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("duplicate policy marker");
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0355"));
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
