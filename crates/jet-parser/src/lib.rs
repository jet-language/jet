#![allow(non_snake_case)]
#![deny(warnings)]
#![allow(unreachable_patterns)]
// Re-export foundation + lexer so `crate::AST`, `crate::Lexer` etc. work in Parser/Formatter.
pub use jet_lexer::{
    CanonicalAST, Collections, Diagnostics, Generics, Lexer, Numeric, Syntax, TargetProfile,
    Traits, Policy, AST, SHA256,
};
mod FencedNames;
pub mod Formatter;
pub mod Parser;

#[cfg(test)]
mod generic_module_tests {
    use super::{AST::{GenericModuleParam, Item, ModuleArg, Type}, Lexer, Parser};

    #[test]
    fn generic_module_slots_remain_unresolved_until_sema_without_casing_heuristics() {
        let src="module Weird<UPPER: Int, lower> { fn ready() => Bool { return true } }\nmodule Use = Weird<32, String>";
        let (tokens,lex)=Lexer::lex(src);assert!(lex.is_empty(),"{lex:?}");let program=Parser::parse(&tokens).unwrap();
        let Item::GenericModule(def)=&program.items[0]else{panic!("template")};
        assert!(matches!(&def.params[0],GenericModuleParam::Annotated{name,..}if name=="UPPER"));
        assert!(matches!(&def.params[1],GenericModuleParam::Bare{name,..}if name=="lower"));
        let Item::ModuleAlias(alias)=&program.items[1]else{panic!("alias")};
        assert!(matches!(&alias.args[0],ModuleArg::Value(..)));assert!(matches!(&alias.args[1],ModuleArg::Type(..)));
    }

    #[test]
    fn generic_module_value_slots_parse_closed_identifier_led_expressions() {
        let src = "module retry<count: Int> { fn ready() => Bool { return true } }\nmodule a = retry<limit + 1>\nmodule b = retry<compute()>";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let program = Parser::parse(&tokens).unwrap();
        for item in &program.items[1..] {
            let Item::ModuleAlias(alias) = item else { panic!("alias") };
            assert!(matches!(&alias.args[0], ModuleArg::Value(..)));
        }
    }

    #[test]
    fn generic_module_retains_symbolic_fixed_length_and_nested_modules() {
        let src = "module buffer<T, capacity: Int> { struct Data { items: [T#capacity] } module stats { fn size() => Int { return capacity } } }";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let program = Parser::parse(&tokens).unwrap();
        let Item::GenericModule(def) = &program.items[0] else { panic!("template") };
        let Item::Struct(data) = &def.body[0] else { panic!("struct") };
        assert!(matches!(&data.fields[0].ty, Type::FixedList { len_symbol: Some((name, _)), .. } if name == "capacity"));
        assert!(matches!(&def.body[1], Item::CodeModule(module) if module.name == "stats"));
    }
}
