//! Stage 1a — `module name { … }` declarations (U3, unified-ecosystem §4).
//! Parser-level: the module shell (many per file, leading-`_` disable) and its
//! typed namespace contributions (`env.dev: Env { … }`). Contribution *values*
//! reuse the existing struct-literal expression parser.

use jet::ast::{Contribution, Item, Namespace};

fn parse_items(src: &str) -> Vec<Item> {
    let (toks, lex_diags) = jet::lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
    jet::parser::parse(&toks).expect("parse").items
}

#[test]
fn parses_module_shell_with_contribution() {
    let src = r#"
module dev {
    env.dev: Env {
        prompt: "wordstats",
    }
}
"#;
    let items = parse_items(src);
    assert_eq!(items.len(), 1);
    let Item::Module(m) = &items[0] else {
        panic!("expected a module item, got {:?}", items[0]);
    };
    assert_eq!(m.name, "dev");
    assert!(!m.disabled);
    assert_eq!(m.contributions.len(), 1);
    let Contribution {
        namespace, path, ..
    } = &m.contributions[0];
    assert_eq!(*namespace, Namespace::Env);
    assert_eq!(path, "dev");
}

#[test]
fn leading_underscore_disables_module() {
    let src = r#"
module _gaming {
    system.gaming: System {
        target: "x86_64-linux",
    }
}
"#;
    let items = parse_items(src);
    let Item::Module(m) = &items[0] else {
        panic!("expected a module item");
    };
    assert_eq!(m.name, "_gaming");
    assert!(m.disabled);
    assert_eq!(m.contributions[0].namespace, Namespace::System);
}

#[test]
fn many_modules_per_file() {
    let src = r#"
module laptop {
    system.laptop: System { target: "x86_64-linux" }
}
module installer {
    image.installer: Image { target: "x86_64-linux" }
}
"#;
    let items = parse_items(src);
    assert_eq!(items.len(), 2);
    let (Item::Module(a), Item::Module(b)) = (&items[0], &items[1]) else {
        panic!("expected two module items");
    };
    assert_eq!(a.name, "laptop");
    assert_eq!(a.contributions[0].namespace, Namespace::System);
    assert_eq!(b.name, "installer");
    assert_eq!(b.contributions[0].namespace, Namespace::Image);
}
