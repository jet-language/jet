#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export foundation + lexer so `crate::AST`, `crate::Lexer` etc. work in Parser/Formatter.
pub use jet_lexer::{
    CanonicalAST, Collections, Diagnostics, Generics, Lexer, Numeric, Syntax, TargetProfile,
    Traits, AST, SHA256,
};
pub mod Formatter;
pub mod Parser;

#[cfg(test)]
mod generic_module_tests {
    use super::{AST::{GenericModuleParam, Item, ModuleArg}, Lexer, Parser};

    #[test]
    fn generic_module_slots_remain_unresolved_until_sema_without_casing_heuristics() {
        let src="module Weird<UPPER: Int, lower> { fn ready() -> Bool { return true } }\nmodule Use = Weird<32, String>";
        let (tokens,lex)=Lexer::lex(src);assert!(lex.is_empty(),"{lex:?}");let program=Parser::parse(&tokens).unwrap();
        let Item::GenericModule(def)=&program.items[0]else{panic!("template")};
        assert!(matches!(&def.params[0],GenericModuleParam::Annotated{name,..}if name=="UPPER"));
        assert!(matches!(&def.params[1],GenericModuleParam::Bare{name,..}if name=="lower"));
        let Item::ModuleAlias(alias)=&program.items[1]else{panic!("alias")};
        assert!(matches!(&alias.args[0],ModuleArg::Value(..)));assert!(matches!(&alias.args[1],ModuleArg::Type(..)));
    }

    #[test]
    fn generic_module_value_slots_parse_closed_identifier_led_expressions() {
        let src = "module Retry<count: Int> { fn ready() -> Bool { return true } }\nmodule A = Retry<limit + 1>\nmodule B = Retry<compute()>";
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let program = Parser::parse(&tokens).unwrap();
        for item in &program.items[1..] {
            let Item::ModuleAlias(alias) = item else { panic!("alias") };
            assert!(matches!(&alias.args[0], ModuleArg::Value(..)));
        }
    }
}
