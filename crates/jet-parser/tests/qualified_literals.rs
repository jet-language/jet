use jet_parser::AST::{Expr, Item, Stmt, Type};
use jet_parser::{Lexer, Parser};

#[test]
fn qualified_generic_literals_keep_their_type_arguments() {
    let source = r#"
fn run() {
    imported :: testing.TypedHistoryCase<List<Int>>{value: 1}
    nested :: Outer.Record<List<Int>>{value: 2}
    plain :: testing.TypedHistoryCase{value: 3}
    call :: testing.decode<Int>(raw)
    comparison :: a < b < (c)
}
"#;
    let (tokens, diagnostics) = Lexer::lex(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let program = Parser::parse(&tokens).expect("qualified generic literals parse");
    let run = program.items.iter().find_map(|item| match item {
        Item::Func(function) if function.name == "run" => Some(function),
        _ => None,
    }).expect("run");
    let binding = |name| run.body.iter().find_map(|statement| match statement {
        Stmt::Val(value) if value.name == name => Some(&value.init),
        _ => None,
    });
    let expected = vec![Type::Apply { name: "List".to_string(), args: vec![Type::Int] }];
    assert!(matches!(binding("imported"), Some(Expr::StructLit {
        type_name, type_args, import_ns: Some(namespace), ..
    }) if type_name == "TypedHistoryCase" && namespace == "testing" && type_args == &expected));
    assert!(matches!(binding("nested"), Some(Expr::StructLit {
        type_name, type_args, import_ns: None, ..
    }) if type_name == "Outer.Record" && type_args == &expected));
    assert!(matches!(binding("plain"), Some(Expr::StructLit { type_args, .. }) if type_args.is_empty()));
    assert!(matches!(binding("call"), Some(Expr::MethodCall { type_args, .. }) if type_args == &vec![Type::Int]));
    assert!(!matches!(binding("comparison"), Some(Expr::MethodCall { .. } | Expr::StructLit { .. })));
}
