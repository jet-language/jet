#[test]
fn parse_option_fn() {
    let src = r#"
fn find_even(limit: Int) -> (Int?) {
    loop i in 1..limit {
        if i % 2 == 0 {
            return value(i);
        }
    }
    return null;
}
fn main() {}
"#;
    let (toks, _) = jet::Lexer::lex(src);
    let prog = jet::Parser::parse(&toks).expect("parse");
    assert_eq!(prog.items.len(), 2);
}

#[test]
fn parse_pipe_switch_arms_as_subject_tests() {
    let src = r#"
fn main() {
    fruit :: "orange"
    frozen :: false
    if fruit {
        apple -> { print("Apple Juice") }
        orange || frozen != true -> { print("Orange Juice") }
        tangerine || yuzu -> { print("Citrus Juice") }
        else -> { print("Water") }
    }
}
"#;
    let (toks, lex_diags) = jet::Lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
    let prog = jet::Parser::parse(&toks).expect("parse");
    let jet::AST::Item::Func(func) = &prog.items[0] else {
        panic!("expected function");
    };
    let jet::AST::Stmt::Switch {
        subject,
        arms,
        else_body,
        ..
    } = &func.body[2]
    else {
        panic!("expected switch");
    };
    assert!(matches!(subject, jet::AST::Expr::Ident(name, _) if name == "fruit"));
    assert_eq!(arms.len(), 3);
    assert!(else_body.is_some());
    assert!(matches!(
        &arms[0].cond,
        jet::AST::Expr::Binary(jet::AST::BinOp::Eq, _, _, _)
    ));
    assert!(matches!(
        &arms[1].cond,
        jet::AST::Expr::Binary(jet::AST::BinOp::Or, _, _, _)
    ));
}

#[test]
fn parse_bracket_collection_types_and_semicolon_list_items() {
    let src = r#"
pub fn shell() -> [JSON] {
    return [
        JSON.Null;
    ];
}

fn use_collections(items: [String], counts: [String, Int]) {}
"#;
    let (toks, lex_diags) = jet::Lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
    let prog = jet::Parser::parse(&toks).expect("parse");
    let jet::AST::Item::Func(shell) = &prog.items[0] else {
        panic!("expected shell function");
    };
    assert!(matches!(shell.return_type, Some(jet::AST::Type::List(_))));
    let jet::AST::Item::Func(use_collections) = &prog.items[1] else {
        panic!("expected use_collections function");
    };
    assert!(matches!(
        use_collections.params[0].ty,
        jet::AST::Type::List(_)
    ));
    assert!(matches!(
        use_collections.params[1].ty,
        jet::AST::Type::Map { .. }
    ));
}

#[test]
fn parse_numeric_field_emits_e0049() {
    let src = "fn main() { x :: p.0 }";
    let (toks, _) = jet::Lexer::lex(src);
    let (_prog, diags) = jet::Parser::parse_for_check(&toks).expect("recoverable parse");
    assert!(
        diags.iter().any(|d| d.code == "E0049"),
        "expected E0049 for `.0` access, got: {diags:?}"
    );
}
