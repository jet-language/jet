use jet_parser::AST::{Expr, Item, Pattern, Stmt, Type, UnOp};
use jet_parser::{Lexer, Parser};

#[test]
fn result_handler_lookahead_keeps_neighboring_forms_unambiguous() {
    let source = r#"
fn optional() ?Success -> None
fn fallible() !Error -> Err("bad")

fn run(value: Int !Error, optional: String) {
    propagated :: value
    noted :: value?("context")
    chained :: optional?.len
    negated :: !true
    handled :: value ? ok -> if {
        !true -> ok
        else -> ok
    }
    ! error -> error
}
"#;
    let (tokens, lexer_diagnostics) = Lexer::lex(source);
    assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
    let program = Parser::parse(&tokens).expect("neighboring forms should parse");

    let function = |name| {
        program.items.iter().find_map(|item| match item {
            Item::Func(function) if function.name == name => Some(function),
            _ => None,
        })
    };
    assert!(matches!(
        function("optional").and_then(|function| function.return_type.as_ref()),
        Some(Type::Option(inner))
            if matches!(inner.as_ref(), Type::Named(name) if name == "Success")
    ));
    assert!(matches!(
        function("fallible").and_then(|function| function.return_type.as_ref()),
        Some(Type::Result { err, .. })
            if matches!(err.as_ref(), Type::Named(name) if name == "Error")
    ));

    let run = function("run").expect("run");
    let binding = |name| {
        run.body.iter().find_map(|stmt| match stmt {
            Stmt::Val(binding) if binding.name == name => Some(&binding.init),
            _ => None,
        })
    };
    assert!(matches!(binding("propagated"), Some(Expr::Ident(name, _)) if name == "value"));
    assert!(matches!(
        binding("noted"),
        Some(Expr::Try(_, _, _, Some(_)))
    ));
    assert!(matches!(binding("chained"), Some(Expr::OptField { .. })));
    assert!(matches!(
        binding("negated"),
        Some(Expr::Unary(UnOp::Not, inner, _))
            if matches!(inner.as_ref(), Expr::Bool(true, _))
    ));

    let Some(Expr::If {
        cond,
        then_value,
        else_value,
        ..
    }) = binding("handled")
    else {
        panic!("handler must remain an exhaustive if expression");
    };
    assert!(matches!(
        cond.as_ref(),
        Expr::PatternTest {
            pattern: Pattern::Ok { binding, .. },
            ..
        } if binding == "ok"
    ));
    assert!(matches!(
        then_value.as_ref(),
        Expr::If { cond, .. }
            if matches!(
                cond.as_ref(),
                Expr::Unary(UnOp::Not, inner, _)
                    if matches!(inner.as_ref(), Expr::Bool(true, _))
            )
    ));
    assert!(matches!(
        else_value.as_ref(),
        Expr::If {
            cond: err_cond,
            then_value: failure,
            ..
        } if matches!(
            err_cond.as_ref(),
            Expr::PatternTest {
                pattern: Pattern::Err { binding, .. },
                ..
            } if binding == "error"
        ) && matches!(failure.as_ref(), Expr::Ident(name, _) if name == "error")
    ));
}
