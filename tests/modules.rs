//! Stage 1a — `module name { … }` declarations (U3, unified-ecosystem §4).
//! Parser-level: the module shell (many per file, leading-`_` disable) and its
//! typed namespace contributions (`env.dev: Env { … }`). Contribution *values*
//! reuse the existing struct-literal expression parser.

use jet::ast::{Call, Contribution, Expr, Item, Namespace, StrPart};

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
fn parses_nested_sources_and_imports() {
    // U8: `sources:` / `imports:` nest inside the module body, as siblings of
    // the `env.dev: Env { … }` contribution (owner, 2026-06-16; amends U4).
    let src = r#"
module dev {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 }
    imports: find("./modules")
    env.dev: Env {
        prompt: "wordstats",
    }
}
"#;
    let items = parse_items(src);
    let Item::Module(m) = &items[0] else {
        panic!("expected a module item, got {:?}", items[0]);
    };

    // One named source; its `provider@target` ref is recovered by slicing the
    // source at the recorded span (the parser is token-based, the ref is not a
    // single token — modeval validates it via classify_provider_ref).
    assert_eq!(m.sources.len(), 1);
    assert_eq!(m.sources[0].name, "default");
    assert_eq!(
        &src[m.sources[0].ref_span.start..m.sources[0].ref_span.end],
        "github@NixOS/nixpkgs/nixos-24.05"
    );

    // One import: `find("./modules")`, parsed as an ordinary call expression.
    assert_eq!(m.imports.len(), 1);
    let Expr::Call(Call { name, args, .. }) = &m.imports[0] else {
        panic!("expected a call expression, got {:?}", m.imports[0]);
    };
    assert_eq!(name, "find");
    assert_eq!(args.len(), 1);
    let Expr::Str(parts, _) = &args[0].expr else {
        panic!("expected a string argument, got {:?}", args[0].expr);
    };
    let [StrPart::Lit(path)] = parts.as_slice() else {
        panic!("expected a single literal string part, got {parts:?}");
    };
    assert_eq!(path, "./modules");

    // The typed contribution still parses alongside the new fields.
    assert_eq!(m.contributions.len(), 1);
    assert_eq!(m.contributions[0].namespace, Namespace::Env);
    assert_eq!(m.contributions[0].path, "dev");
}

#[test]
fn module_without_sources_or_imports_has_empty_fields() {
    let src = r#"
module dev {
    env.dev: Env { prompt: "x" }
}
"#;
    let items = parse_items(src);
    let Item::Module(m) = &items[0] else {
        panic!("expected a module item");
    };
    assert!(m.sources.is_empty());
    assert!(m.imports.is_empty());
    assert_eq!(m.contributions.len(), 1);
}

#[test]
fn leading_underscore_disables_module() {
    let src = r#"
module _gaming {
    system.gaming: System {
        target: linux.x64,
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
    system.laptop: System { target: linux.x64 }
}
module installer {
    image.installer: Image { from: system.laptop, target: linux.arm64 }
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
