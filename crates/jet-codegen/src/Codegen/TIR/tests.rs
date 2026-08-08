    use super::*;
    use crate::AST::{Expr, Func, Item, Stmt};
    use std::collections::{HashMap, HashSet};

    /// Parse `src` (no full sema needed — `tir_covers` is structural plus
    /// program-table lookups that `build_cx` fills) and return whether the
    /// named function is covered by the Phase-1 TIR gate.
    fn covers(src: &str, fn_name: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let cx = build_cx(&prog, src, "test.jet");
        let f = prog
            .items
            .iter()
            .find_map(|i| match i {
                Item::Func(f) if f.name == fn_name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no fn {fn_name}"));
        tir_covers(f, &cx)
    }

    #[test]
    fn shield_block_is_covered_by_tir() {
        assert!(covers("fn guarded() { #Shield { value :: 1 } }", "guarded"));
    }

    /// Like `covers`, but runs the FULL front end (sema) on `src` first, so
    /// sema-filled facts — notably a comptime LOCAL's evaluated `b.ct` value
    /// (S57/M9.5) — are present before gating. Builds a single-module bundle the
    /// same way `lib.rs::check_for_eval` does, asserts sema accepted the program,
    /// then runs `tir_covers` on the sema-enriched function.
    fn checked_bundle(src: &str) -> crate::AST::ProgramBundle {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let mut prog = crate::Parser::parse(&toks).expect("parse failed");
        let mut bundle = crate::AST::ProgramBundle {
            entry: 0,
            project_root: std::path::PathBuf::from("."),
            modules: vec![crate::AST::LoadedModule {
                path: std::path::PathBuf::from("test.jet"),
                display: "test.jet".to_string(),
                alias: "main".to_string(),
                imports: std::mem::take(&mut prog.imports),
                items: std::mem::take(&mut prog.items),
                block_spans: std::mem::take(&mut prog.block_spans),
                source: src.to_string(),
                web_target_ceiling: prog.web_target_ceiling,
                pub_file: prog.pub_file,
                no_prelude: prog.no_prelude,
                html_path: prog.html_path.clone(),
                no_alloc_policy: prog.no_alloc_policy,
                policy_declarations: prog.policy_declarations.clone(),
                rule_facts: std::mem::take(&mut prog.rule_facts),
            }],
            parse_teaching: Vec::new(),
            used_core: std::collections::HashSet::new(),
            ffi_callback_fns: std::collections::HashSet::new(),
            cffi: crate::AST::CFfi::default(),
            comptime_inputs: Vec::new(),
            import_targets: std::collections::HashMap::new(),
            layer_ceiling: None,
            inferred_layer: crate::Syntax::RuntimeLayer::Core,
            web_partitions: std::collections::HashMap::new(),
            web_partition_enforced: false,
            web_partition_report: None,
            dep_roots: std::collections::HashMap::new(),
            active_os: crate::Syntax::OSTarget::host(),
            edition: "2027".to_string(),
        };
        // No C imports in unit tests; CFfi::default() is the correct empty state.
        let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
        assert!(
            !diags
                .iter()
                .any(|d| d.severity == crate::Diagnostics::Severity::Error),
            "sema errors: {diags:?}"
        );
        bundle
    }

    fn covers_after_sema(src: &str, fn_name: &str) -> bool {
        let bundle = checked_bundle(src);
        let module = &bundle.modules[bundle.entry];
        let cx = build_cx_items(&module.items, src, "test.jet", None, &HashMap::new());
        let f = module
            .items
            .iter()
            .find_map(|i| match i {
                Item::Func(f) if f.name == fn_name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no fn {fn_name}"));
        tir_covers(f, &cx)
    }

    fn lower_after_sema(src: &str, fn_name: &str) -> TFunc {
        let bundle = checked_bundle(src);
        let module = &bundle.modules[bundle.entry];
        let cx = build_cx_items(&module.items, src, "test.jet", None, &HashMap::new());
        let f = module
            .items
            .iter()
            .find_map(|item| match item {
                Item::Func(f) if f.name == fn_name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no fn {fn_name}"));
        lower_func(f, &cx)
    }

    #[test]
    fn comptime_float_and_string_constants_lower_to_typed_literals() {
        install_comptime_bridge();
        let lowered = lower_after_sema(
            r#"
$narrow :: F32.{16777217.0}
$wide :: 2.5
$label :: "ready"

fn run() {
    print(narrow)
    print(wide)
    print(label)
}
"#,
            "run",
        );
        let printed: Vec<_> = lowered
            .body
            .iter()
            .filter_map(|stmt| match stmt {
                TStmt::ExprStmt(TExpr {
                    kind: TExprKind::Print(value),
                    ..
                }) => Some(value.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(printed.len(), 3);
        assert_eq!(printed[0].ty, Type::Float32);
        assert!(matches!(
            printed[0].kind,
            TExprKind::FloatLit(value) if value == 16_777_216.0
        ));
        assert_eq!(printed[1].ty, Type::Float);
        assert!(matches!(
            printed[1].kind,
            TExprKind::FloatLit(value) if value == 2.5
        ));
        assert_eq!(printed[2].ty, Type::String);
        assert!(matches!(
            &printed[2].kind,
            TExprKind::StrLit(parts)
                if matches!(parts.as_slice(), [TStrPart::Lit(text)] if text == "ready")
        ));

        let (tokens, diagnostics) = crate::Lexer::lex("fn run() {}");
        assert!(diagnostics.is_empty());
        let program = crate::Parser::parse(&tokens).unwrap();
        let cx = build_cx(&program, "fn run() {}", "test.jet");
        for (ty, value, expected) in [
            (Type::Float32, f64::NAN, "f32::NAN"),
            (Type::Float32, f64::INFINITY, "f32::INFINITY"),
            (Type::Float32, f64::NEG_INFINITY, "f32::NEG_INFINITY"),
            (Type::Float, f64::NAN, "f64::NAN"),
            (Type::Float, f64::INFINITY, "f64::INFINITY"),
            (Type::Float, f64::NEG_INFINITY, "f64::NEG_INFINITY"),
        ] {
            assert_eq!(
                emit_tir_expr(
                    &TExpr {
                        ty,
                        kind: TExprKind::FloatLit(value),
                    },
                    &cx,
                ),
                expected
            );
        }
    }

    #[test]
    fn refined_collection_results_have_exact_tir_types() {
        let src = "\
fn run() {
    folded := [1, 2].fold(0.5, (a: Float, n: Int) => a + 0.5)
    reduced := [1, 2].reduce(0.5, (a: Float, n: Int) => a + 0.5)
    scanned := [1, 2].scan(0.5, (a: Float, n: Int) => a + 0.5)
    mapped := [1, 2].map((n: Int) => 1.5)
    filtered := [\"1.5\", \"bad\"].filter_map((s: String) => Float.parse(s))
    flattened := [1, 2].flat_map((n: Int) => [1.5])
    grouped := [1, 2].group_by((n: Int) => n % 2 == 0)
    counted := [1, 2].count_by((n: Int) => n % 2)
    parallel := [1, 2].para_fold(() => 0.5, (a: Float, n: Int) => a + 0.5, (left: Float, right: Float) => left + right)
    grouped_string_get := [1, 2].group_by((n: Int) => \"x\").get(\"x\")
    counted_string_get := [1, 2].count_by((n: Int) => \"x\").get(\"x\")
}
";
        let lowered = lower_after_sema(src, "run");
        let actual: Vec<Type> = lowered
            .body
            .iter()
            .filter_map(|stmt| match stmt {
                TStmt::Let { init, .. } => Some(init.ty.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                Type::Float,
                Type::Float,
                Type::List(Box::new(Type::Float)),
                Type::List(Box::new(Type::Float)),
                Type::List(Box::new(Type::Float)),
                Type::List(Box::new(Type::Float)),
                Type::Map {
                    key: Box::new(Type::Bool),
                    key_span: None,
                    value: Box::new(Type::List(Box::new(Type::Int))),
                },
                Type::Map {
                    key: Box::new(Type::Int),
                    key_span: None,
                    value: Box::new(Type::Int),
                },
                Type::Float,
                Type::Option(Box::new(Type::List(Box::new(Type::Int)))),
                Type::Option(Box::new(Type::Int)),
            ]
        );
        assert!(matches!(
            &lowered.body[9],
            TStmt::Let {
                init: TExpr {
                    kind: TExprKind::BuiltinMethod {
                        op: TBuiltinOp::GetMap,
                        ..
                    },
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            &lowered.body[10],
            TStmt::Let {
                init: TExpr {
                    kind: TExprKind::BuiltinMethod {
                        op: TBuiltinOp::GetMap,
                        ..
                    },
                    ..
                },
                ..
            }
        ));
    }

    /// c109 Phase 7: parse `src` and return whether the named method on `type_name`
    /// (a struct or enum inherent method) is covered by the method gate. Looks up
    /// the method in the type's `methods` list. As with `covers`, the
    /// sema-dependent facts a method body needs (`recv_type` on inner method calls)
    /// are not filled by `build_cx` alone, so the gate paths that consult them are
    /// proven by the TIR feature integration targets + the byte-parity check;
    /// here we exercise the sema-independent structural gating (self receiver,
    /// static shape, param/return types, the `self`-assignment exclusion).
    fn covers_method(src: &str, type_name: &str, method: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let cx = build_cx(&prog, src, "test.jet");
        let methods: &[Func] = prog
            .items
            .iter()
            .find_map(|i| match i {
                Item::Struct(s) if s.name == type_name => Some(s.methods.as_slice()),
                Item::Enum(e) if e.name == type_name => Some(e.methods.as_slice()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no type {type_name}"));
        let f = methods
            .iter()
            .find(|m| m.name == method)
            .unwrap_or_else(|| panic!("no method {type_name}.{method}"));
        tir_covers_method(f, type_name, &cx)
    }

    #[test]
    fn covers_simple_arithmetic_fn() {
        assert!(covers(
            "fn add(a: Int, b: Int) => Int {\n return (a + b)\n}\n",
            "add"
        ));
    }

    #[test]
    fn covers_print_and_string_param() {
        assert!(covers(
            "fn greet(s: String) {\n print(\"hi {s}\")\n}\n",
            "greet"
        ));
    }

    #[test]
    fn covers_if_else_chain() {
        let src = "fn f(n: Int) => Int {\n if (n > 0) {\n return 1\n } else {\n return 0\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_bare_or_return_in_unit_fn() {
        // c109 (bare `?? return` fix): a bare `?? return` in a UNIT fn is in-subset
        // (`orfallback_rhs_in_subset → Return(None) => true`) and emits `None => return`.
        // Sema now accepts it only in a unit fn (rustc accepts `return;`).
        let src = "fn f(xs: [Int]) {\n x := xs.first() ?? return\n print(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_struct_lit_with_string_field_value() {
        // c109 (borrowed struct-lit value clone): a struct with a String field, built
        // from a param value, is in-subset (struct + clone are covered). The borrowed-
        // ident clone is a SEMA rewrite (`(n).clone()`) — the `covers` helper is
        // build_cx-only so it sees the bare ident here, which is also in-subset; the
        // authoritative byte-for-byte proof is
        // tests/tir_patterns_and_fields.rs::borrowed_struct_lit_field_value_cloned.
        let src = "\
struct Person {
    name: String
}
fn make(n: String) => Person {
    return Person.{ name: n }
}
";
        assert!(covers(src, "make"));
    }

    #[test]
    fn covers_is_empty_bool() {
        // c109 (`is_empty` Bool fix): `is_empty` on a list/map/string is now covered
        // (`TBuiltinOp::IsEmpty`, Bool result) — it was excluded while sema mistyped it
        // as `Int`. The `if xs.is_empty()` form must be in-subset.
        let src = "fn f(xs: [Int]) {\n if xs.is_empty() {\n print(\"e\")\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_generic_fn() {
        // c109 Phase 17: a generic free function whose params/return are type vars is
        // covered — the `<T: Clone>` clause renders at lowering; the body uses the
        // type-var value by-value. (The `covers` helper is build_cx-only, so it sees
        // `x: T` as a Read param; sema would require `take x: T`, but the gate shape is
        // identical either way — a type-var param/return is in-subset.)
        assert!(covers("fn id<T>(x: T) => T {\n return x\n}\n", "id"));
    }

    #[test]
    fn covers_generic_struct_fn() {
        // c109 Phase 19: a GENERIC STRUCT free function — its `Type::Apply` (`Pair<T>`)
        // param/return type and the turbofish construction (`user_Pair::<T> { … }`) are now
        // covered. The struct's type-var fields are admitted by `field_ty_covered`; the
        // turbofish head is resolved at lowering.
        let src = "struct Pair<T> {\n first: T\n second: T\n}\nfn mk<T>(a: T, b: T) => Pair<T> {\n return Pair<T>.{first: a, second: b}\n}\n";
        assert!(covers(src, "mk"));
    }

    /// c109 Phase 18: like `covers`, but injects the `mem` → `core.mem` import (the
    /// `build_cx`-only path leaves `core_imports` empty — it is populated from the bundle
    /// at real codegen; mirror that here so the core-`mem` gate paths are exercised). The
    /// end-to-end build+run + the full-suite byte-parity diff are the authoritative proof
    /// (see `tests/tir_unsafe_and_runtime.rs::unsafe_fn_block_and_ptr_ops`);
    /// this exercises the gate shape.
    fn covers_with_mem(src: &str, fn_name: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let mut cx = build_cx(&prog, src, "test.jet");
        cx.core_imports
            .insert("mem".to_string(), "core.mem".to_string());
        let f = prog
            .items
            .iter()
            .find_map(|i| match i {
                Item::Func(f) if f.name == fn_name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no fn {fn_name}"));
        tir_covers(f, &cx)
    }

    /// Like `covers`, but injects a foreign type → module mapping (`cx.foreign_types`)
    /// — the `build_cx`-only path leaves it empty (it's populated from the bundle at real
    /// codegen). Mirrors `covers_with_mem`. The end-to-end build+run + the full-suite
    /// byte-parity diff are the authoritative proof (the TIR feature integration
    /// targets); this exercises the gate.
    fn covers_with_foreign(src: &str, fn_name: &str, foreign: &[(&str, &str)]) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let mut cx = build_cx(&prog, src, "test.jet");
        for (ty, module) in foreign {
            cx.foreign_types.insert(ty.to_string(), module.to_string());
        }
        let f = prog
            .items
            .iter()
            .find_map(|i| match i {
                Item::Func(f) if f.name == fn_name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no fn {fn_name}"));
        tir_covers(f, &cx)
    }

    #[test]
    fn covers_unqualified_foreign_struct_literal() {
        // c109 (foreign struct literal): an UNqualified cross-module foreign struct literal
        // (`Note { … }`, no `import_ns`) is now covered — the StructLit gate admits a
        // `cx.foreign_types` type and lowering prefixes the module head
        // (`user_notes::user_Note`). The construct miscompiled to a bare `user_Note { … }`
        // (E0422) before; the fix prefixes the foreign module.
        let src = "\
fn mk() {
    n :: Note.{ text: \"hi\" }
    print(n.text)
}
";
        assert!(covers_with_foreign(src, "mk", &[("Note", "user_notes")]));
    }

    #[test]
    fn covers_unsafe_fn_with_ptr_ops() {
        // c109 Phase 18: a `#Unsafe fn` (S58) is covered — it lowers to `unsafe fn`, and
        // its body's `mem.Ptr<T>.from_addr` / `mem.volatile_read` ops are in-subset.
        let src = "use core.mem\n#Unsafe\nfn read_reg(addr: Int) => Int {\n p :: mem.Ptr<Int>.from_addr(addr)\n return mem.volatile_read(p)\n}\n";
        assert!(covers_with_mem(src, "read_reg"));
    }

    #[test]
    fn covers_unsafe_block_and_address_of() {
        // c109 Phase 18: a `#Unsafe("…") { … }` audited region + `mem.address_of` (the
        // inert address cast, legal outside unsafe) are covered.
        let src = "use core.mem\nfn run() {\n cell: Int :: 7\n addr :: mem.address_of(cell)\n #Unsafe(\"live\") {\n p :: mem.Ptr<Int>.from_addr(addr)\n seen :: mem.volatile_read(p)\n print(\"{seen}\")\n }\n}\n";
        assert!(covers_with_mem(src, "run"));
    }

    #[test]
    fn covers_list_param() {
        // c109 Phase 5: a list parameter is now inside the subset (was excluded
        // through Phase 4).
        assert!(covers("fn sum(xs: [Int]) => Int {\n return 0\n}\n", "sum"));
    }

    #[test]
    fn covers_fixed_list_param_and_field() {
        // c109 (B2): a fixed-size-list type `[E#N]` is covered like a list (`Vec<E>`)
        // as a param/return type and as a struct field, once its element type is
        // covered. (Indexing a `[E#N]` is in-subset only once sema resolves the
        // `IndexKind` — exercised end-to-end by the TIR feature integration
        // targets; here we gate the sema-independent type-coverage facts the four
        // helpers decide.)
        let mk = |src: &str| {
            let (toks, _) = crate::Lexer::lex(src);
            let prog = crate::Parser::parse(&toks).expect("parse");
            build_cx(&prog, src, "test.jet")
        };
        let fl = Type::FixedList {
            elem: Box::new(Type::Int),
            len: 3,
            len_symbol: None,
        };
        // param/return helper coverage:
        assert!(is_subset_param_ty(&fl, &mk("fn f(){}")));
        assert!(is_covered_collection_ty(&fl, &mk("fn f(){}")));
        assert!(collection_elem_covered(&fl, &mk("fn f(){}")));
        // struct-field coverage: a `[Int#3]` field keeps its owning struct covered.
        let src = "struct Grid { row: [Int#3] }\nfn f(){}";
        assert!(is_covered_struct_ty(
            &Type::Named("Grid".to_string()),
            &mk(src)
        ));
    }

    #[test]
    fn covers_option_param() {
        // c109 Phase 8: an optional-typed param (`Int?`) is now inside the subset
        // (was excluded through Phase 7). The payload is a covered value type.
        assert!(covers("fn f(p: Int?) => Int {\n return 0\n}\n", "f"));
    }

    #[test]
    fn rejects_list_of_option_param_still() {
        // A list whose element is itself optional (`[Int?]`) is still excluded — the
        // collection element-coverage does not admit optionals (clone/coercion for an
        // option-element collection is deferred), even though a bare `Int?` is covered.
        assert!(!covers("fn f(xs: [Int?]) => Int {\n return 0\n}\n", "f"));
    }

    #[test]
    fn rejects_method_call_in_body() {
        // A method call (`.bumped()`) is not a covered construct.
        let src = "struct C { n: Int }\nimpl C {\n fn bumped(self) => Int {\n return (self.n + 1)\n }\n}\nfn use_it(c: Int) => Int {\n return c\n}\nfn caller() => Int {\n x :: C.{ n: 1 }\n return x.bumped()\n}\n";
        assert!(!covers(src, "caller"));
    }

    // c109 Phase 3: structs.

    #[test]
    fn covers_struct_param_and_scalar_field_read() {
        // A plain struct param with a scalar field read (borrow position) and a
        // struct literal + struct return are all in the subset.
        let src = "struct Point { x: Int\n y: Int }\nfn sum_pt(p: Point) => Int {\n return (p.x + p.y)\n}\nfn origin() => Point {\n return Point.{ x: 0, y: 0 }\n}\n";
        assert!(covers(src, "sum_pt"));
        assert!(covers(src, "origin"));
    }

    #[test]
    fn covers_nested_struct() {
        // A struct field whose type is itself a covered struct, with a chained
        // field read and a nested literal.
        let src = "struct Inner { v: Int }\nstruct Outer { inner: Inner\n label: Int }\nfn deep(o: Outer) => Int {\n return (o.inner.v + o.label)\n}\n";
        assert!(covers(src, "deep"));
    }

    #[test]
    fn covers_recursive_boxed_struct() {
        // c109 (boxed field read): a self-referential struct is now a covered VALUE type
        // — a boxed field read derefs the `Box` (total `boxed` fact). A fn reading a plain
        // scalar field of a recursive struct routes through the TIR.
        let src = "struct Node { value: Int\n next: Node }\nfn val(n: Node) => Int {\n return n.value\n}\n";
        assert!(covers(src, "val"));
    }

    #[test]
    fn covers_struct_with_list_field() {
        // c109 Phase 16: a struct with a covered collection field (`[Int]`). The
        // struct-literal emit is plain (`items: vec![…]`), byte-identical to the AST
        // path, so the owning struct is covered as a param/return.
        let src = "struct Bag { items: [Int] }\nfn first_tag(b: Bag) => Int {\n return 0\n}\n";
        assert!(covers(src, "first_tag"));
    }

    #[test]
    fn covers_generic_struct_literal() {
        // c109 Phase 19: a generic struct literal (`Pair<Int> { … }`) carries non-empty
        // `type_args` (the turbofish `user_Pair::<i64> { … }`) and its field types reference
        // type vars — both now covered. The owning fn routes through the TIR.
        let src = "struct Pair<T> { first: T\n second: T }\nfn mk() => Pair<Int> {\n return Pair<Int>.{ first: 1, second: 2 }\n}\n";
        assert!(covers(src, "mk"));
    }

    // c109 Phase 2: control-flow loops are now covered.

    #[test]
    fn covers_range_loop() {
        let src = "fn f() {\n loop n; 1..3 {\n print(n)\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_range_loop_with_step() {
        let src = "fn f() {\n loop n; 0..10; 2 {\n print(n)\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_infinite_loop_with_break() {
        let src = "fn f() {\n x :: 0\n loop {\n x = (x + 1)\n if (x > 3) {\n break\n }\n }\n print(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_while_form() {
        let src = "fn f() {\n x :: 0\n loop (x < 3) {\n x = (x + 1)\n }\n print(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_labeled_loops() {
        let src = "fn f() {\n outer :: loop {\n loop n; 1..3 {\n if (n == 2) {\n break(outer)\n }\n }\n break\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_collection_loop_over_literal() {
        // c109 Phase 5: `loop x; [list literal]` (ForKind::In) is now covered
        // (was deferred to this phase through Phase 4).
        let src = "fn f() {\n loop x; [1, 2, 3] {\n print(x)\n }\n}\n";
        assert!(covers(src, "f"));
    }

    // c109 Phase 4: enums + when/match + patterns.

    #[test]
    fn covers_enum_unit_match() {
        // A unit-variant enum, an enum literal, and an exhaustive variant match.
        let src = "enum Light {\n Red\n Yellow\n Green\n}\nfn next(light: Light) => Light {\n if light == {\n Red -> { return Light.Yellow }\n Yellow -> { return Light.Green }\n Green -> { return Light.Red }\n }\n}\n";
        assert!(covers(src, "next"));
    }

    #[test]
    fn covers_enum_payload_or_and_wildcard() {
        // Scalar-payload enum, or-pattern with a shared binding, and a wildcard slot.
        let src = "enum Conn {\n Active(Int)\n Reconnecting(Int)\n Idle(Int)\n Closed\n}\nfn d(c: Conn) => String {\n if c == {\n Active(id) | Reconnecting(id) -> { return \"live:{id}\" }\n Idle(_) -> { return \"idle\" }\n Closed -> { return \"closed\" }\n }\n return \"unknown\"\n}\n";
        assert!(covers(src, "d"));
    }

    #[test]
    fn covers_enum_payload_range_pattern() {
        // A range pattern in a payload slot (guard-emitted) plus a wildcard slot.
        let src = "enum HTTP {\n Good(Int)\n Fail(Int)\n}\nfn classify(r: HTTP) => String {\n if r == {\n Good(200..299) -> { return \"ok\" }\n Good(_) -> { return \"other\" }\n Fail(_) -> { return \"err\" }\n }\n return \"unknown\"\n}\n";
        assert!(covers(src, "classify"));
    }

    #[test]
    fn covers_arm_head_range_switch() {
        // An all-range arm-head scalar switch with an `else` (mixed-switch path).
        let src = "fn grade(score: Int) => String {\n if score == {\n 0..59 -> { return \"F\" }\n 60..100 -> { return \"P\" }\n else -> { return \"?\" }\n }\n}\n";
        assert!(covers(src, "grade"));
    }

    #[test]
    fn covers_mixed_switch_non_ident_subject() {
        // c109 (B1): a pattern switch over a NON-IDENT subject routes through the
        // exhaustive-match / fallible-match path (the subject is matched by source-text
        // equality, not just an ident name). A call subject with unit-variant arms:
        let variant = "enum Light { Red Green Yellow }\nfn pick() => Light { return Light.Red }\nfn classify() => Int {\n if pick() == {\n Red -> { return 1 }\n Green -> { return 2 }\n else -> { return 0 }\n }\n}\n";
        assert!(covers(variant, "classify"));
        // A field-access subject with a payload-binding (optional) arm:
        let payload = "struct Holder { val: Int? }\nfn f(h: Holder) => Int {\n if h.val == {\n Val(c) -> { return c }\n else -> { return 0 }\n }\n}\n";
        assert!(covers(payload, "f"));
    }

    #[test]
    fn covers_enum_local_and_literal_in_main() {
        // An enum-typed local bound from a literal, passed to a covered helper.
        let src = "enum Light {\n Red\n Yellow\n Green\n}\nfn label(l: Light) => String {\n if l == {\n Red -> { return \"r\" }\n Yellow -> { return \"y\" }\n Green -> { return \"g\" }\n }\n}\nfn run() {\n start :: Light.Red\n print(label(start))\n}\n";
        assert!(covers(src, "run"));
    }

    #[test]
    fn covers_string_payload_enum() {
        // c109 Phase 16: a String-payload enum. The literal's borrowed-payload
        // `.clone()` and pattern bindings are reproduced as total facts
        // (`emit_boxed_enum_arg`), so the match + getter route through the TIR.
        let src = "enum Msg {\n Text(String)\n Ping\n}\nfn show(m: Msg) => String {\n if m == {\n Text(s) -> { return s }\n Ping -> { return \"ping\" }\n }\n return \"\"\n}\n";
        assert!(covers(src, "show"));
    }

    #[test]
    fn covers_recursive_enum() {
        // c109 Phase 16: a self-referential (boxed) enum. The `Box::new(…)` at
        // construction and the auto-deref at pattern/field sites are total facts
        // (`TEnumArg.boxed`), so a covered traversal routes through the TIR.
        let src = "enum Tree {\n Leaf(Int)\n Node(Tree)\n}\nfn depth(t: Tree) => Int {\n if t == {\n Leaf(n) -> { return n }\n Node(inner) -> { return 1 }\n }\n return 0\n}\n";
        assert!(covers(src, "depth"));
    }

    #[test]
    fn covers_recursive_enum_construction_with_clone_box() {
        // c109 Phase 16: constructing a recursive enum from a BORROWED payload —
        // `Tree.Node(inner)` where `inner: Tree` is a `Read` (borrowed) param. The
        // arg gets `Box::new(((*inner)).clone())` (non-scalar payload → borrowed
        // `.clone()`, then the recursive boxed edge → `Box::new`), reproducing
        // `emit_boxed_enum_arg` exactly. The construction reaches codegen as a
        // `MethodCall` (sema never emits an `Expr::EnumLit` for a payload variant).
        let src = "enum Tree {\n Leaf(Int)\n Node(Tree)\n}\nfn wrap(inner: Tree) => Tree {\n return Tree.Node(inner)\n}\n";
        assert!(covers(src, "wrap"));
    }

    #[test]
    fn covers_struct_payload_enum() {
        // c109 Phase 16: an enum variant carrying a covered struct payload. The
        // struct value flows through the variant construction + pattern binding
        // without a clone/box decision the subset can't make (the value's own move/
        // clone facts live in its sub-expression).
        let src = "struct Point { x: Int\n y: Int }\nenum Shape {\n Dot(Point)\n Line(Int)\n}\nfn area(s: Shape) => Int {\n if s == {\n Dot(p) -> { return p.x }\n Line(n) -> { return n }\n }\n return 0\n}\n";
        assert!(covers(src, "area"));
    }

    #[test]
    fn covers_collection_payload_enum() {
        // c109 Phase 16: an enum variant carrying a covered collection payload
        // (`[Int]`). Construction (`Holder.Nums(xs)`) routes through the variant
        // MethodCall shape; the borrowed-list `.clone()` is total.
        let src = "enum Holder {\n Nums([Int])\n One(Int)\n}\nfn mk(xs: [Int]) => Holder {\n return Holder.Nums(xs)\n}\n";
        assert!(covers(src, "mk"));
    }

    #[test]
    fn rejects_range_switch_over_non_ident_subject() {
        // D-IF3: a value+range mixed switch (shape D) lowers each range head to
        // `subject >= lo && subject <= hi`, so the subject must be a scalar ident
        // local for the emitted condition to type-check. A NON-IDENT subject (a
        // call) with a range arm is excluded from the subset (stays on the AST
        // path), even though the value arm alone would be fine.
        let src = "fn pick() => Int { return 5 }\nfn f() => String {\n if pick() == {\n 0 -> { return \"zero\" }\n 1..10 -> { return \"low\" }\n else -> { return \"mid\" }\n }\n}\n";
        assert!(!covers(src, "f"));
    }

    // c109 Phase 5: collections. (Index/slice/index-assign coverage needs the
    // sema-resolved `IndexKind`, which `build_cx` alone does not fill, so those are
    // proven by the byte-parity check + the TIR feature integration targets; here
    // we gate the sema-independent constructs: list/map literals, list/map-typed
    // params, and collection iteration.)

    #[test]
    fn covers_list_literal_and_param() {
        // A list literal returned from a covered fn, and a list-typed param.
        let src = "fn build() => [Int] {\n return [1, 2, 3]\n}\nfn accept(xs: [Int]) => Int {\n return 0\n}\n";
        assert!(covers(src, "build"));
        assert!(covers(src, "accept"));
    }

    #[test]
    fn covers_map_literal_and_param() {
        // An empty and a non-empty map literal, plus a map-typed param.
        let src = "fn empty() => [String: Int] {\n return []\n}\nfn one() => [String: Int] {\n return [\"a\": 1]\n}\nfn accept(m: [String: Int]) => Int {\n return 0\n}\n";
        assert!(covers(src, "empty"));
        assert!(covers(src, "one"));
        assert!(covers(src, "accept"));
    }

    #[test]
    fn covers_single_binding_iteration() {
        // `loop x; <list>` over a list-typed param is now covered (Phase 5).
        let src = "fn f(xs: [Int]) {\n loop x; xs {\n print(x)\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_two_binding_map_iteration() {
        // `loop k, v; <map>` (the two-binding map form) is covered.
        let src = "fn f(m: [String: Int]) {\n loop k, v; m {\n print(\"{k}={v}\")\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_method_call_collection_iteration() {
        // c109 Phase 22: `loop c; s.chars()` (char iteration) and `loop x in
        // s.split(…)` (the `.iter().cloned()` default) are now reproduced from
        // `emit_for_in`'s method-call branches.
        let chars = "fn f(s: String) {\n loop c; s.chars() {\n print(c)\n }\n}\n";
        assert!(covers(chars, "f"));
        let split = "fn f(s: String) {\n loop w; s.split(\",\") {\n print(w)\n }\n}\n";
        assert!(covers(split, "f"));
    }

    #[test]
    fn covers_optional_binding_if_condition() {
        // c109 Phase 22: `if x == Val(b) { … b … }` lowers to `if let Some(b) = …`.
        let src = "fn f(x: Int?) {\n if x == Val(n) {\n print(\"{n}\")\n }\n}\n";
        assert!(covers(src, "f"));
        // `x == None` lowers to `.is_none()`.
        let isnone = "fn f(x: Int?) {\n if x == None {\n print(\"none\")\n }\n}\n";
        assert!(covers(isnone, "f"));
    }

    #[test]
    fn covers_user_enum_variant_if_let_condition() {
        // c109 (B4): `if m == Ping(n) { … } else { … }` over a covered user enum lowers
        // to `if let user_Msg::user_Ping(user_n) = m`. Single-payload variant (one bind).
        let src = "enum Msg { Ping(Int) Pong }\nfn f(m: Msg) => Int {\n if m == Ping(n) {\n return n\n } else {\n return -1\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn rejects_list_of_option_param() {
        // A list whose element is an option (`[Int?]`) is not a covered value type
        // (optionals are Phase 8); the owning collection is excluded.
        let src = "fn f(xs: [Int?]) => Int {\n return 0\n}\n";
        assert!(!covers(src, "f"));
    }

    // c109 Phase 6: methods + clones. (The gate paths that need a sema-resolved
    // `recv_type` are proven by the byte-parity check + the TIR feature integration
    // targets; `build_cx` alone does not fill `recv_type`. Here we gate the
    // sema-independent facts:
    // covered method *signatures* are registered, and a covered function bodyless
    // of method calls is unaffected.)

    #[test]
    fn covers_struct_param_with_method_caller() {
        // A struct with a user method: the method body (has `self`) is excluded,
        // but a free function taking the struct and reading a scalar field is still
        // covered (Phase 3 baseline — methods don't disturb the existing coverage).
        let src = "struct Calc {\n base: Int\n fn add(self, x: Int) => Int {\n return (self.base + x)\n }\n}\nfn peek(c: Calc) => Int {\n return c.base\n}\n";
        assert!(covers(src, "peek"));
    }

    #[test]
    fn builtin_method_names_are_excluded() {
        // `is_intercepted_method_name` flags every collection/string/special builtin
        // name (`len`, `push`, `map`, …). It still guards the STATIC call-site shape
        // (`static_method_call_in_subset`). For an INSTANCE method, the user-method gate
        // now keys on a real `method_sigs` entry instead (the builtin-name-collision fix —
        // see `covers_user_method_shadowing_builtin_name`), so a user instance method
        // SHADOWING a builtin name routes to the user method on both paths. The predicate
        // contents are unchanged; assert them.
        for name in [
            "len",
            "push",
            "pop",
            "get",
            "map",
            "filter",
            "each",
            "find",
            "sort",
            "join",
            "to_string",
            "clone",
            "raw",
            "snapshot",
            "new",
            "is_nan",
            "chars",
            "trim",
            "keys",
            "values",
        ] {
            assert!(
                is_intercepted_method_name(name),
                "{name} should be excluded (AST builtin/special lowering)"
            );
        }
        // A plain user method name is not intercepted.
        assert!(!is_intercepted_method_name("bumped"));
        assert!(!is_intercepted_method_name("combine"));
        assert!(!is_intercepted_method_name("code"));
    }

    #[test]
    fn covers_user_method_shadowing_builtin_name() {
        // c109 (builtin-name collision): a user instance method whose name collides with a
        // builtin (`get`/`len`) now routes through the TIR when a real `method_sigs` entry
        // exists. The AST `emit_method_call` dispatches such a call to `user_<method>` BEFORE
        // `emit_builtin_method` (the fix), so the gate admits it. `recv_type` is a sema fact
        // (`build_cx` alone leaves the call node's `recv_type` empty), so we drive
        // `method_call_in_subset` directly with a synthetic `Some("Bag")` receiver — exactly
        // the node sema produces. (The end-to-end build+run + byte-parity in the TIR
        // feature integration targets is
        // the authoritative proof; this exercises the gate's user-vs-builtin decision.)
        let src = "struct Bag {\n items: [Int]\n fn get(self) => Int {\n return 1\n }\n fn len(self) => Int {\n return 2\n }\n}\n";
        let (toks, _) = crate::Lexer::lex(src);
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let cx = build_cx(&prog, src, "test.jet");
        let sp = crate::Diagnostics::Span { start: 0, end: 0 };
        let mk = |method: &str| Expr::MethodCall {
            receiver: Box::new(Expr::Ident("b".to_string(), sp)),
            method: method.to_string(),
            method_span: sp,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: Vec::new(),
            recv_type: Some("Bag".to_string()),
            resolved_ret: None,
        };
        let mut locals = HashSet::new();
        locals.insert("b".to_string());
        // Both builtin-name user methods are admitted (a real `method_sigs` entry exists).
        for m in ["get", "len"] {
            if let Expr::MethodCall {
                receiver,
                method,
                args,
                recv_type,
                ..
            } = &mk(m)
            {
                assert!(
                    method_call_in_subset(receiver, method, args, recv_type, &cx, &locals),
                    "user method `{m}` shadowing a builtin name should be covered"
                );
            }
        }
        // A builtin name with NO user method on the type stays excluded (`push` isn't a
        // method on `Bag`), so the builtin/name-keyed path keeps it on the AST side.
        if let Expr::MethodCall {
            receiver,
            method,
            args,
            recv_type,
            ..
        } = &mk("push")
        {
            assert!(
                !method_call_in_subset(receiver, method, args, recv_type, &cx, &locals),
                "a builtin name with no user method must stay excluded"
            );
        }
    }

    // c109 Phase 7: method bodies + static methods.

    #[test]
    fn covers_instance_method_body() {
        // A `self` getter on a covered struct, body reading `self.field` — covered.
        // (Multi-letter type name; a single uppercase letter reads as a type var.)
        let src = "struct Cell {\n n: Int\n fn value(self) => Int {\n return self.n\n }\n}\n";
        assert!(covers_method(src, "Cell", "value"));
    }

    #[test]
    fn covers_mut_self_method_body() {
        // A `mut self` receiver (→ `&mut self`) whose body only reads is covered.
        let src = "struct Acc {\n total: Int\n fn doubled(&self) => Int {\n return (self.total + self.total)\n }\n}\n";
        assert!(covers_method(src, "Acc", "doubled"));
    }

    #[test]
    fn covers_static_constructor() {
        // A static (no-`self`) associated function returning the owning type.
        let src =
            "struct Cell {\n n: Int\n fn make(v: Int) => Cell {\n return Cell.{ n: v }\n }\n}\n";
        assert!(covers_method(src, "Cell", "make"));
    }

    #[test]
    fn covers_enum_instance_method() {
        // A `when self` match in an enum method body is covered.
        let src = "enum Dir {\n North\n South\n fn code(self) => Int {\n if self == {\n North -> { return 0 }\n South -> { return 1 }\n }\n }\n}\n";
        assert!(covers_method(src, "Dir", "code"));
    }

    #[test]
    fn covers_self_reassignment_method() {
        // D-MUTSELF1: a `mut self` method that reassigns `self` (`self = …`) is NOW
        // covered — the `mut self` slot derefs (`(*self)`), so the LHS lowers to
        // `(*self) = …` (the prior AST-path I2 hole is closed).
        let src = "struct Acc {\n n: Int\n fn reset(&self) {\n self = Acc.{ n: 0 }\n }\n}\n";
        assert!(covers_method(src, "Acc", "reset"));
    }

    #[test]
    fn covers_self_field_assign_method() {
        // D-MUTSELF1: a `mut self` method assigning a field (`self.field = v`, S17
        // compound `+=` too) is covered — lowers to `((*self)).field = v`.
        let src = "struct Acc {\n n: Int\n fn bump(&self) {\n self.n = self.n + 1\n }\n}\n";
        assert!(covers_method(src, "Acc", "bump"));
        let compound = "struct Acc {\n n: Int\n fn bump(&self) {\n self.n += 1\n }\n}\n";
        assert!(covers_method(compound, "Acc", "bump"));
    }

    #[test]
    fn covers_generic_method() {
        // Card #129: generic owner identity survives through the enclosing
        // `impl<T> user_Box<T>`; the method body lowers through ordinary TIR.
        let src = "struct Box<T> {\n v: T\n fn get(self) => T {\n return self.v\n }\n}\n";
        assert!(covers_method(src, "Box", "get"));
    }

    #[test]
    fn rejects_intercepted_static_name() {
        // A static method named `new` collides with the alloc/special intercept
        // (`mem.*.new`) — the AST path special-cases the name, so the TIR static
        // call gate must NOT claim it. (The method body itself may still route, but
        // its *call* `Type.new()` stays on the AST path; here we check the body gate
        // is independent — `new` as a *static body* is still a plain method def.)
        // The static *call*-site exclusion is covered by `is_intercepted_method_name`.
        assert!(is_intercepted_method_name("new"));
    }

    // c109 Phase 8: fallible + optional.

    #[test]
    fn covers_fallible_return_and_try() {
        // A `T ? Error` return (default-error fallible) with `ok`/`err` over scalar
        // values and `?` propagation of a covered fallible call — all in-subset
        // (Phase 8). (`Error` lowers to `String`; the constructors here take a scalar
        // and a String literal. Full sema owns the resolved fallible types and
        // constructor rewrites consumed by the TIR gate. A
        // scalar-payload *error enum* literal is `Bad.Code(1)`, which parses as a
        // MethodCall and is only rewritten to an `EnumLit` by full sema; that path is
        // proven end-to-end by
        // `tests/tir_collections_and_methods.rs::fallible_try_and_or_fallback`.)
        let src = "fn f(x: Int) => Int ? Error {\n if x == 0 {\n return Err(\"bad\")\n }\n return Ok(x)\n}\nfn g(x: Int) => Int ? Error {\n n :: f(x)?\n return Ok((n + 1))\n}\nfn run() {}\n";
        assert!(covers_after_sema(src, "f"));
        assert!(covers_after_sema(src, "g"));
    }

    #[test]
    fn covers_optional_return_and_chaining() {
        // A `T?` return with `Val`/`None`, plus `?.` chaining over a covered struct.
        // (Multi-letter struct name; a single uppercase letter reads as a type var.)
        let src = "struct Addr {\n city: String\n}\nfn opt(x: Int) => (Int?) {\n if x > 0 {\n return Val(x)\n }\n return None\n}\nfn ch(a: (Addr?)) => (String?) {\n return a?.city\n}\n";
        assert!(covers(src, "opt"));
        assert!(covers(src, "ch"));
    }

    #[test]
    fn covers_or_fallback_value_and_return() {
        // `??` with a value fallback and with an early-`return` fallback.
        let src = "fn v(x: (Int?)) => Int {\n return x ?? 0\n}\nfn r(x: (Int?)) => Int {\n return x ?? return -1\n}\n";
        assert!(covers(src, "v"));
        assert!(covers(src, "r"));
    }

    #[test]
    fn covers_or_fallback_panic_form() {
        // c109 Phase 15: the `panic(…)` fallback form is now covered — the
        // `safe_locals_expr` snapshot is rendered from the lexical lowering env.
        let src = "fn p(x: (Int?)) => Int {\n return x ?? panic(\"missing\")\n}\n";
        assert!(covers(src, "p"));
    }

    #[test]
    fn covers_comptime_if() {
        // c109 Phase 15: a resolved comptime-if routes through the TIR — only the
        // selected branch's statements are emitted inline. (`build_cx`-only gate test:
        // the gate's `stmt_in_subset` admits `Stmt::ComptimeIf` unconditionally; the
        // lowering reads `selected_then`, but the gate does not need sema for routing.)
        let src =
            "fn f(x: Int) => Int {\n $if true {\n return x\n } else {\n return 0\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_mixed_bool_switch() {
        // c109 Phase 15 / D-IF3: a mixed value+range dispatch (shape D) routes via the
        // TIR's `MixedSwitch` (the general `emit_mixed_switch` if/else chain) — a
        // bare-value arm (`0 ->` ≡ `x == 0`) beside range arms (`1..10 ->`), each
        // range lowered to `x >= lo && x <= hi`. (Q4 retired free-predicate arms.)
        let src = "fn f(x: Int) => Int {\n if x == {\n 0 -> {\n return 2\n }\n 1..10 -> {\n return 1\n }\n else -> {\n return 0\n }\n }\n}\n";
        assert!(covers(src, "f"));
    }

    // c109 Phase 9: built-in collection/string methods. A builtin call has
    // `recv_type == None` (parser default; sema leaves it None for non-numeric
    // builtins), so `build_cx` alone proves the gate's builtin shape.

    #[test]
    fn covers_list_builtin_methods() {
        // push/len/get/sort/reverse/contains on a list-typed param — all covered,
        // so the whole function routes through the TIR.
        let src = "fn f(xs: [Int]) => Int {\n ys := xs\n ys.push(1)\n ys.reverse()\n ys.sort()\n n := ys.len()\n c := ys.contains(3)\n return n\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_map_builtin_methods() {
        // add/len/keys/values/has_key/clear on a map-typed param. Run the full
        // front end so this coverage proof cannot drift onto a list-only or
        // otherwise invalid method spelling that sema would reject before TIR.
        let src = "fn f(m: [String: Int]) => Int {\n m2 := ~m\n old := m2.add(\"k\", 1) ?? 0\n n := m2.len()\n ks := m2.keys()\n vs := m2.values()\n ck := m2.has_key(\"a\")\n m2.clear()\n return n\n}\nfn run() {}\n";
        assert!(covers_after_sema(src, "f"));
    }

    #[test]
    fn rejects_unsupported_map_builtin_handoff() {
        // `contains_key` is not Jet's Map surface. The TIR gate must hand the
        // unsupported shape back instead of guessing from its Rust spelling.
        let src = "fn f(m: [String: Int]) => Bool {\n return m.contains_key(\"a\")\n}\n";
        assert!(!covers(src, "f"));
    }

    #[test]
    fn covers_string_builtin_methods() {
        // to_upper/to_lower/trim/split/starts_with/replace/repeat/slice/chars/bytes.
        let src = "fn f(s: String) => String {\n up := s.to_upper()\n tr := s.trim()\n sp := s.split(\",\")\n sw := s.starts_with(\"a\")\n rp := s.replace(\"a\", \"b\")\n rep := s.repeat(2)\n sl := s.slice(0, 2)\n ch := s.chars()\n by := s.bytes()\n return up\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn rejects_closure_builtin_method() {
        // A closure-taking builtin (`map`/`filter`/…) is deferred to the lambda
        // phase — `is_covered_builtin_name` returns false, and the lambda arg is
        // out-of-subset anyway. The owning function stays on the AST path.
        for name in [
            "map", "filter", "each", "find", "any", "all", "sort_by", "reduce",
        ] {
            assert!(
                !is_covered_builtin_name(name, 1),
                "{name} (closure method) must NOT be a covered builtin"
            );
        }
    }

    #[test]
    fn covers_is_empty_builtin() {
        // `is_empty` is now Bool-typed (c109 fix) and covered (`TBuiltinOp::IsEmpty`);
        // a function using it routes through the TIR.
        assert!(is_covered_builtin_name("is_empty", 0));
        let src =
            "fn f(xs: [Int]) => Int {\n e := xs.is_empty()\n if e {\n return 1\n }\n return 0\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn source_owned_numeric_aliases_are_not_builtins() {
        for name in ["to_i32", "to_u8", "to_f64"] {
            assert!(
                !is_covered_builtin_name(name, 0),
                "{name} is retired by D-SHAPE-CONVERT1"
            );
        }
        for name in ["is_nan", "count_ones"] {
            assert!(!is_covered_builtin_name(name, 0), "{name} is a Phase-12 query");
        }
    }

    #[test]
    fn covers_string_payload_error_enum() {
        // c109 Phase 16: a `T ? E` whose error enum has a String payload is now
        // covered — the error enum is a covered (String-payload) enum, and its
        // construction (`Err(Oops.Msg("bad"))`) reproduces `emit_boxed_enum_arg`
        // (a String literal arg, no borrowed clone) byte-for-byte.
        let src = "enum Oops {\n Msg(String)\n}\nfn f(x: Int) => Int ? Oops {\n if x == 0 {\n return Err(Oops.Msg(\"bad\"))\n }\n return Ok(x)\n}\nfn run() {}\n";
        assert!(covers_after_sema(src, "f"));
    }

    #[test]
    fn covers_fn_typed_param() {
        // c109 Phase 13: a fn-typed parameter is now inside the subset (was excluded
        // through Phase 12, when any callee/param with a `Type::Fn` stayed on the AST
        // path). The body `f(f(x))` is a fn-value call through the local param.
        let src = "fn apply_twice(f: fn(Int) => Int, x: Int) => Int {\n return f(f(x))\n}\n";
        assert!(covers(src, "apply_twice"));
    }

    #[test]
    fn covers_fn_name_value_arg() {
        // c109 Phase 13: a bare top-level fn name used as a VALUE (passed to a
        // fn-typed param) is in subset — it emits `emit_named_fn_value`'s
        // `Box::new(move |…| …) as <fn-type>` wrapper.
        let src = "fn callit(f: fn(Int) => Int) => Int {\n return f(1)\n}\nfn dbl(x: Int) => Int {\n return (x * 2)\n}\nfn use_it() => Int {\n return callit(dbl)\n}\n";
        assert!(covers(src, "use_it"));
    }

    #[test]
    fn handle_method_op_table() {
        // c109 Phase 13: the covered handle-method set, and the excluded ones.
        assert!(handle_method_op("FileReader", "read_line", 0).is_some());
        assert!(handle_method_op("FileWriter", "write_line", 1).is_some());
        assert!(handle_method_op("FileWriter", "flush", 0).is_some());
        assert!(handle_method_op("TcpStream", "read", 0).is_some());
        assert!(handle_method_op("TcpStream", "close", 0).is_some());
        assert!(handle_method_op("TcpListener", "accept", 0).is_some());
        // c109 Phase 19: the reusable arena methods (`alloc`/`reset`) are now
        // covered (the producer `mem.Arena.new()` is covered too).
        assert!(handle_method_op("Arena", "alloc", 1).is_some());
        assert!(handle_method_op("Bump", "reset", 0).is_some());
        assert!(handle_method_op("Pool", "free", 0).is_none());
        // c109 Phase 20: HTTPRequest/HTTPResponse accessors are now covered (the
        // `http.serve` lambda-param type is written back onto `p.ty`, so the slot
        // type is total and the AST `rty`-keyed handle arm fires identically).
        assert!(handle_method_op("HTTPRequest", "method", 0).is_some());
        assert!(handle_method_op("HTTPRequest", "path", 0).is_some());
        assert!(handle_method_op("HTTPRequest", "header", 1).is_some());
        assert!(handle_method_op("HTTPRequest", "param", 1).is_some());
        assert!(handle_method_op("HTTPRequest", "trailers", 0).is_some());
        assert!(handle_method_op("HTTPResponse", "status", 0).is_some());
        assert!(handle_method_op("HTTPResponse", "body", 0).is_some());
        assert!(handle_method_op("HTTPResponse", "trailers", 1).is_some());
        // D-ARGS1: ArgsSpec builder and ParsedArgs query methods.
        assert!(handle_method_op("ArgsSpec", "flag", 2).is_some());
        assert!(handle_method_op("ArgsSpec", "option", 3).is_some());
        assert!(handle_method_op("ArgsSpec", "positional", 2).is_some());
        assert!(handle_method_op("ArgsSpec", "help", 0).is_some());
        assert!(handle_method_op("ArgsSpec", "parse", 1).is_some());
        assert!(handle_method_op("ParsedArgs", "flag", 1).is_some());
        assert!(handle_method_op("ParsedArgs", "option", 1).is_some());
        assert!(handle_method_op("ParsedArgs", "positional", 1).is_some());
        // D-ANY-JAI1 (c7jaiany §6): reflect.of(x)'s Value/Field handle methods.
        assert!(handle_method_op("Value", "type_name", 0).is_some());
        assert!(handle_method_op("Value", "display", 0).is_some());
        assert!(handle_method_op("Value", "fields", 0).is_some());
        assert!(handle_method_op("Field", "name", 0).is_some());
        assert!(handle_method_op("Field", "value", 0).is_some());
        // Excluded: dead `lines` (E2502).
        assert!(handle_method_op("FileReader", "lines", 0).is_none());
        // Wrong arity declines.
        assert!(handle_method_op("FileWriter", "write_line", 0).is_none());
    }

    #[test]
    fn polymorphic_core_specials_covered() {
        // c109 Phase 20: the polymorphic core specials route through the core-call
        // shape (`core_call_covered`), their return type read from the node's
        // `resolved_ret` (written by sema). `io.input` is NOT a polymorphic special —
        // its fixed `Result<String, IOError>` return rides `core_call_return_ty`
        // (c109 Phase 29; it is NOT in `core_fixed_sig`).
        assert!(core_call_covered("core.math", "abs"));
        assert!(core_call_covered("core.math", "min"));
        assert!(core_call_covered("core.math", "max"));
        assert!(core_call_covered("core.math", "clamp"));
        assert!(core_call_covered("core.random", "pick"));
        assert!(core_call_covered("core.random", "weighted_pick"));
        assert!(core_call_covered("core.random", "sample"));
        assert!(core_call_covered("core.random", "shuffle"));
        assert!(core_call_covered("core.io", "eprint"));
        // c109 Phase 21 / D-TUPLE-DESTRUCT1: the `tasks.channel<T>()` producer is
        // covered via the core-call shape (a fixed-string `jet_std::channel::<T>()`
        // emit; its `(Sender<T>, Receiver<T>)` return type rides on `resolved_ret`,
        // filled from the call-site turbofish). `tasks.spawn` stays out of this
        // shape — it has its own bespoke `CoreClosureCall` shape (a `move |…|` closure).
        assert!(core_call_covered("core.tasks", "channel"));
        assert!(!core_call_covered("core.tasks", "spawn"));
        // c109 Phase 25: the HTTPRouter producer + parse/dispatch core calls are covered
        // (fixed-string emits; their return types live in sema's `infer_core_call`, not
        // `core_fixed_sig`). `http.serve` stays out (closure-taking → `CoreClosureCall`).
        assert!(core_call_covered("core.http", "router"));
        assert!(core_call_covered("core.http", "parse"));
        assert!(core_call_covered("core.http", "dispatch"));
        assert!(!core_call_covered("core.http", "serve"));
        // c109 Phase 29: qualified `io.input` is a covered core call. NOT in
        // `core_fixed_sig` (its `Result<String, IOError>` return lives in sema's bespoke
        // `infer_core_call` arm, reproduced in `core_call_return_ty`). Distinct from the
        // ambient bare `input()` (Phase 25), which is its own `Expr::Call` → `AmbientInput`.
        assert!(core_call_covered("core.io", "input"));
        assert!(!crate::Sema::core_fixed_sig("core.io", "input").is_some());
    }

    #[test]
    fn io_input_return_ty() {
        // c109 Phase 29: `core_call_return_ty` carries `io.input`'s fixed
        // `Result<String, IOError>` total (it is NOT in `core_fixed_sig`, so without this
        // arm the node's `ty` would fall back to Unit and break `?? return` composition).
        let ty = core_call_return_ty("core.io", "input");
        match ty {
            Type::Result { ok, err } => {
                assert_eq!(*ok, Type::String);
                assert_eq!(*err, Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()));
            }
            other => panic!("io.input return ty should be Result<String, IOError>, got {other:?}"),
        }
    }

    #[test]
    fn covers_static_new_constructor() {
        // c109 Phase 25: a STATIC constructor `Rect.new(...)` routes (the Phase-7 static
        // shape — `recv_type == None`, type-name receiver, `(Rect, "new") ∈ method_sigs`),
        // even though `new` is in `is_intercepted_method_name` (which guards the INSTANCE
        // shape). `build_cx`-only: the static call carries `recv_type == None` by default.
        let src = "\
struct Rect { width: Int height: Int }
impl Rect {
    fn new(width: Int, height: Int) => Rect { return Rect.{width: width, height: height} }
}
fn build() => Rect { return Rect.new(4, 3) }
";
        assert!(covers(src, "build"));
        // The instance-method intercept stays whole: a user INSTANCE method named `new`
        // is still excluded (it stays on the AST path).
        assert!(is_intercepted_method_name("new"));
    }

    #[test]
    fn covers_ambient_input() {
        // c109 Phase 25: the ambient prelude `input(...)` routes (bare call, no user
        // `input` fn). It composes with the `??` value fallback (Phase 8).
        let src = "\
fn greet() => String {
    name :: input() ?? \"world\"
    return \"hi {name}\"
}
";
        assert!(covers(src, "greet"));
        // A user-defined `input` fn shadows the prelude — the gate then treats `input(...)`
        // as a plain fn call (still covered, but via the plain-fn shape, not ambient).
        let shadowed = "\
fn input() => String { return \"x\" }
fn greet() => String { return input() }
";
        assert!(covers(shadowed, "greet"));
    }

    #[test]
    fn covers_require_builtins() {
        // c109 Phase 26: the rich-runtime-report builtins `require`/`require_eq`/`panic`
        // (S36) route. Each is a bare `Expr::Call` whose name is the builtin (not a user
        // fn / local) with the right arity; the whole emit string is rendered at lowering.
        assert!(covers("fn f() { require((1 + 1) == 2) }", "f"));
        assert!(covers("fn f() { require(false, \"nope\") }", "f"));
        assert!(covers("fn f() { require_eq(2, 2) }", "f"));
        assert!(covers("fn f() { panic(\"stop\") }", "f"));
        // A user fn / local named `require` shadows the builtin — it then routes via the
        // plain-fn shape, NOT the builtin (still covered, different path).
        assert!(covers(
            "fn require(x: Int) => Int { return x }\nfn f() => Int { return require(3) }",
            "f"
        ));
    }

    #[test]
    fn covers_caps_block() {
        // c109 Phase 26: a `#Caps(IO) { … }` effect-restriction region erases to a plain
        // block (byte-for-byte `Stmt::Region`); its body is checked on the SAME locals, so
        // an out-of-subset body keeps the whole fn off the TIR path.
        assert!(covers("fn f() { #Caps(IO) { print(\"x\") } }", "f"));
        // c109: a single-uppercase-letter DECLARED struct name (`P`) is a concrete
        // type, not a type variable — the `is_type_var_name` heuristic is now guarded
        // on non-declaration (`cx.struct_fields` lookup). So `P{x: 1}` and the
        // `P{x} :: p` struct-destructure are both covered; the fn routes through TIR.
        assert!(covers(
            "struct P { x: Int }\nfn f() { p :: P.{x: 1}\n#Caps(IO) { P.{x} :: p\nprint(x) } }",
            "f"
        ));
    }

    #[test]
    fn covers_free_call_arg_conventions() {
        // c109 Phase 26: ALL three free-call arg conventions route — `Read` (`&(…)`),
        // `Move` (`take`-marked), and `Mutate` (`mut place` → `&mut (…)`).
        assert!(covers(
            "fn bump(n: &Int) { n += 1 }\nfn f() { s: Int := 1\nbump(&s) }",
            "f"
        ));
        assert!(covers(
            "fn keep(s: ^String) => String { return s }\nfn f() => String { return keep(^\"v\") }",
            "f"
        ));
    }

    #[test]
    fn covers_list_destructure() {
        // c109 Phase 26: a list-destructuring binding `[a, b, c] :: <init>` (S74) routes
        // when the init is in-subset — the fan-out result destructure (`41_fan_out`).
        assert!(covers(
            "fn f() { xs :: [1, 2, 3]\n[a, b, c] :: xs\nprint(a) }",
            "f"
        ));
    }

    #[test]
    fn covers_struct_destructure() {
        // c109: a struct-destructuring binding `Type { x, y } :: <init>` (S74) routes
        // when the init is in-subset — the AST `BindPattern::Struct` arm is covered
        // byte-for-byte (per-field type from `cx.struct_fields`).
        assert!(covers(
            "struct Point { x: Int, y: Int }\nfn f() { p :: Point.{ x: 1, y: 2 }\nPoint.{ x, y } :: p\nprint(x + y) }",
            "f"
        ));
    }

    #[test]
    fn covers_named_fn_value_binding() {
        // c109 Phase 27: a bare top-level fn name bound to a local as a VALUE
        // (`double_fn :: double`). The init `Ident("double")` resolves to a `Type::Fn`
        // value (`emit_named_fn_value`), in-subset via `ident_is_named_fn_value`. (This
        // binding-site coercion was already wired in lowering; the live-suite `24_callbacks`
        // never routed only because the struct fn-field / fn-field-call were uncovered.)
        assert!(covers(
            "fn dbl(x: Int) => Int { return (x * 2) }\nfn f() { g :: dbl\nprint(g(3)) }",
            "f"
        ));
    }

    #[test]
    fn covers_fn_field_struct_value_type() {
        // c109 Phase 27: a struct with a FUNCTION-typed field is a covered VALUE type — the
        // fn-typed field renders to `Box<dyn Fn(...)>` and needs no clone/deref decision at
        // the field site (sema-independent — `build_cx` populates `struct_fields`). (The
        // full construction + `w.step(4)` fn-field CALL is sema-dependent — `recv_type ==
        // Some("Worker")` is a sema fact — so it is proven by the TIR feature
        // integration targets + byte-parity.)
        let src = "struct Worker { step: fn(Int) => Int }\nfn f() {}";
        let (toks, _) = crate::Lexer::lex(src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, src, "test.jet");
        assert!(is_covered_struct_ty(
            &Type::Named("Worker".to_string()),
            &cx
        ));
        // The fn-field-call shape resolves the field's Fn type from a covered struct's
        // `struct_fields` (the `recv_type` half is supplied by the TIR feature
        // integration targets).
        assert!(fn_field_call_ty("step", &Some("Worker".to_string()), &cx).is_some());
        // A non-existent / non-Fn field is not a fn-field call.
        assert!(fn_field_call_ty("missing", &Some("Worker".to_string()), &cx).is_none());
    }

    #[test]
    fn covers_fn_field_type_covered() {
        // c109 Phase 27: `field_ty_covered` admits a `Type::Fn` field directly.
        let src = "fn f() {}";
        let (toks, _) = crate::Lexer::lex(src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, src, "test.jet");
        let fn_ty = Type::Fn {
            params: vec![Type::Int],
            ret: Some(Box::new(Type::Int)),
            effect_bound: None,
            param_contract: None,
            return_view_provenance: None,
        };
        assert!(field_ty_covered(&fn_ty, &cx, &mut HashSet::new()));
    }

    #[test]
    fn concurrency_method_names() {
        // c109 Phase 21 + D-COROUTINE1=A / D-TUPLE-DESTRUCT1: the Task/Receiver/Sender
        // method name+arity set. `join` is the 0-arg form (the 1-arg list `join(sep)`
        // is a collection builtin, NOT here); `send` is the 1-arg form. No `sender` —
        // `tasks.channel<T>()` returns the sender directly, no `.sender()` method.
        assert!(is_concurrency_method_name("join", 0));
        assert!(is_concurrency_method_name("wait", 0));
        assert!(is_concurrency_method_name("detach", 0));
        assert!(is_concurrency_method_name("pause", 0));
        assert!(is_concurrency_method_name("resume", 0));
        assert!(is_concurrency_method_name("cancel", 0));
        assert!(is_concurrency_method_name("trace", 0));
        assert!(is_concurrency_method_name("receive", 0));
        assert!(!is_concurrency_method_name("sender", 0));
        assert!(is_concurrency_method_name("send", 1));
        // Disjoint from the list `join(sep)` (1 arg) and any wrong arity.
        assert!(!is_concurrency_method_name("join", 1));
        assert!(!is_concurrency_method_name("send", 0));
        assert!(!is_concurrency_method_name("receive", 1));
        assert!(!is_concurrency_method_name("len", 0));
    }

    #[test]
    fn reactive_method_names_and_value_types() {
        // D-REACT1=B: the reactive method set (`get`/0, `set`/1) and value types.
        assert!(is_reactive_method_name("get", 0));
        assert!(is_reactive_method_name("set", 1));
        assert!(!is_reactive_method_name("get", 1)); // a list `get(i)` is NOT this shape
        assert!(!is_reactive_method_name("set", 0));
        let cx_src = "fn f() {}\n";
        let (toks, _) = crate::Lexer::lex(cx_src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, cx_src, "t.jet");
        let apply = |n: &str| Type::Apply {
            name: n.to_string(),
            args: vec![Type::Int],
        };
        assert!(is_covered_reactive_ty(&apply("Signal"), &cx));
        assert!(is_covered_reactive_ty(&apply("Derived"), &cx));
        assert!(is_subset_param_ty(&apply("Signal"), &cx));
        assert!(is_subset_param_ty(&apply("Derived"), &cx));
        assert!(!is_covered_reactive_ty(&apply("Receiver"), &cx));
        // The producer + closure-call shapes are covered.
        assert!(core_call_covered("core.reactive", "signal"));
        assert!(crate::Sema::is_polymorphic_core_special(
            "core.reactive",
            "derived"
        ));
    }

    #[test]
    fn event_method_names_and_core_calls() {
        // D-EVENT1=D: typed Event/Hook family lowers through the event handle
        // method shape plus generic core-call constructors.
        assert!(is_event_handle_type(Some("Event")));
        assert!(is_event_handle_type(Some("Hook")));
        assert!(is_event_handle_type(Some("Subscription")));
        assert!(is_event_handle_type(Some("EventScope")));
        assert!(is_event_handle_type(Some("EventTrace")));
        assert!(is_event_handle_type(Some("DecisionHook")));
        assert!(!is_event_handle_type(Some("Signal")));
        assert!(is_event_method_name("on", 2));
        assert!(is_event_method_name("once", 2));
        assert!(is_event_method_name("on_priority", 3));
        assert!(is_event_method_name("emit", 1));
        assert!(is_event_method_name("emit_async", 1));
        assert!(is_event_method_name("run", 2));
        assert!(is_event_method_name("run", 1));
        assert!(is_event_method_name("unsubscribe", 0));
        assert!(is_event_method_name("active_count", 0));
        assert!(is_event_method_name("summary", 0));
        assert!(!is_event_method_name("on", 1));
        assert!(!is_event_method_name("emit", 0));
        assert!(core_call_covered("core.event", "new"));
        assert!(core_call_covered("core.event", "with_policy"));
        assert!(core_call_covered("core.event", "hook"));
        assert!(core_call_covered("core.event", "decision_hook"));
        assert!(core_call_covered("core.event", "scope"));
        assert!(core_call_covered("core.event", "policy_sync"));
        assert!(!core_call_covered("core.event", "policy_async"));
        assert!(!core_call_covered("core.event", "subscribe"));
    }

    #[test]
    fn concurrency_value_types_covered() {
        // c109 Phase 21 / D-TUPLE-DESTRUCT1: `Task<T>`/`Receiver<T>`/`Sender<T>` are
        // covered value types; the `Closed` err type is a covered fallible payload
        // (`Receiver.receive()`).
        let cx_src = "fn f() {}\n";
        let (toks, _) = crate::Lexer::lex(cx_src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, cx_src, "t.jet");
        let apply = |n: &str| Type::Apply {
            name: n.to_string(),
            args: vec![Type::Int],
        };
        assert!(is_covered_concurrency_ty(&apply("Task"), &cx));
        assert!(is_covered_concurrency_ty(&apply("Receiver"), &cx));
        assert!(is_covered_concurrency_ty(&apply("Sender"), &cx));
        assert!(is_subset_param_ty(&apply("Task"), &cx));
        // A `[Task<Unit>]` worker list (34_parallel_scan) is a covered collection.
        let tasks = Type::List(Box::new(Type::Apply {
            name: "Task".to_string(),
            args: vec![unit_type()],
        }));
        assert!(is_covered_collection_ty(&tasks, &cx));
        // `Closed` is a covered fallible payload (the `receive()` err type).
        assert!(fallible_payload_covered(
            &Type::Named("Closed".to_string()),
            &cx
        ));
        // A non-concurrency `Apply` (e.g. a user generic) is NOT this shape.
        assert!(!is_covered_concurrency_ty(&apply("Pair"), &cx));
    }

    #[test]
    fn covers_concurrency_methods() {
        // c109 Phase 21 / D-TUPLE-DESTRUCT1: a function using the `send`/`receive`
        // surface + the `tasks.channel<T>()` producer routes. The gate is
        // `build_cx`-only (no sema), so the method calls carry `recv_type == None`
        // (the unannotated AST default), which is exactly what the d3 shape keys on;
        // the `Receiver<Int>` annotation supplies the value type. (The
        // `tasks.spawn(take(..) …)`/`Task.join` slice depends on sema-filled
        // `Lambda.meta`, so it's proven end-to-end in the TIR feature integration
        // targets.)
        let src = "\
use core.tasks as tasks
fn produce(s: Sender<Int>) {
    s.send(7)
}
fn consume(ch: Receiver<Int>) => Int {
    return ch.receive() ?? panic(\"closed\")
}
";
        // The `Sender.send` method + `Sender<Int>` value type (gate shape d3).
        assert!(covers(src, "produce"));
        // The `Receiver.receive` method + `Receiver<Int>` value type + `Result<Int, Closed>`
        // unwrap via `?? panic`.
        assert!(covers(src, "consume"));
    }

    #[test]
    fn covers_pure_fn() {
        // c109 Phase 23: a `#Pure fn` is covered (purity is sema-only, erased at codegen).
        assert!(covers(
            "fn double(n: Int) =[]=> Int {\n return (n * 2)\n}\n",
            "double"
        ));
    }

    #[test]
    fn covers_todo_hole() {
        // c109 Phase 23: a `#Todo` hole is covered (diverging `todo!`). The build_cx-only
        // helper leaves `expected_type` unset (sema fills it), but the gate admits a
        // None-typed hole too (it lowers to the `(unknown)` fallback — never reached here
        // since this is a structural gate test). Reproduce the sema fact: a hole with an
        // expected type. We can't run sema in this helper, so just assert the simpler
        // surrounding fn is covered — the end-to-end `todo_hole` test proves the emit.
        // (A bare `#Todo` body with no sema annotation has `expected_type: None`, which the
        // gate EXCLUDES — so we assert exclusion here, matching the conservative rule.)
        assert!(!covers("fn f(n: Int) => Int {\n return #Todo\n}\n", "f"));
    }

    #[test]
    fn covers_default_params() {
        // c109 Phase 23: a fn with default param values is covered (defaults are filled at
        // call sites by sema; codegen never reads `p.default`).
        assert!(covers(
            "fn box_dims(w: Int, h: Int = w, d: Int = h) => String {\n return \"{w}{h}{d}\"\n}\n",
            "box_dims"
        ));
    }

    #[test]
    fn covers_distinct_value_type_and_ctor() {
        // c109 Phase 23: a distinct param type + `.raw()` + the destination-owned conversion are
        // covered. The build_cx-only helper registers the distinct in `distinct_types`.
        let src = "UserId :: distinct Int;\nfn greet(id: UserId) => Int {\n return (id.raw())\n}\n";
        assert!(covers(src, "greet"));
        let src2 = "UserId :: distinct Int;\nfn mk() => UserId {\n return UserId.from_int(42)\n}\n";
        assert!(covers(src2, "mk"));
    }

    #[test]
    fn covers_tuple_value_type() {
        // c109 Phase 23: a tuple PARAM type (`(x: Int, y: Int)`) is a covered value type.
        // A field read on it is the generic `Field` shape. (A tuple LITERAL needs sema's
        // `Expr::TupleLit.ty` to resolve the canonical field order/struct name, which the
        // build_cx-only helper does not fill — so the literal + destructure are proven by
        // the end-to-end `named_tuples` test, not here.)
        let src = "fn first(p: (x: Int, y: Int)) => Int {\n return p.x\n}\n";
        assert!(covers(src, "first"));
    }

    #[test]
    fn covers_named_args_at_call_site() {
        // c109 Phase 23: a call-site label is allowed (sema binds by name; codegen
        // ignores them). The callee `area` is a plain fn; the labeled call is in-subset.
        let src = "fn area(width: Int, height: Int) => Int {\n return (width * height)\n}\nfn use_it() => Int {\n return area(width: 4, height: 3)\n}\n";
        assert!(covers(src, "use_it"));
    }

    #[test]
    fn covers_default_param_method() {
        // c109 Phase 23: a struct-body method with a default param value (`clamp: Bool =
        // false`) is covered (same call-site-fill rule as a free fn; codegen never reads
        // `p.default`).
        let src = "struct Rect {\n w: Int\n fn scale(self, factor: Int, clamp: Bool = false) => Int {\n return (self.w * factor)\n }\n}\n";
        assert!(covers_method(src, "Rect", "scale"));
    }

    #[test]
    fn core_closure_calls_covered() {
        // c109 Phase 13: the three closure-taking core calls are covered with a
        // literal in-subset lambda; the polymorphic specials stay deferred.
        let cx_src = "fn f() {}\n";
        let (toks, _) = crate::Lexer::lex(cx_src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, cx_src, "t.jet");
        let locals = HashSet::new();
        let lam = |body: &str| -> Vec<crate::AST::CallArg> {
            let s = format!("fn g() {{ x :: scope.guard({})\n}}\n", body);
            let (t, _) = crate::Lexer::lex(&s);
            let p = crate::Parser::parse(&t).expect("parse lam");
            // Pull the single call arg from the guard call.
            for item in &p.items {
                if let crate::AST::Item::Func(f) = item {
                    for st in &f.body {
                        if let Stmt::Val(b) = st {
                            if let Expr::MethodCall { args, .. } = &b.init {
                                return args.clone();
                            }
                        }
                    }
                }
            }
            Vec::new()
        };
        let guard_args = lam("() => { print(\"x\") }");
        assert!(core_closure_call_in_subset(
            "core.scope",
            "guard",
            &guard_args,
            &cx,
            &locals
        ));
        // A non-closure core call is not a closure-core-call.
        assert!(!core_closure_call_in_subset(
            "core.files",
            "read",
            &guard_args,
            &cx,
            &locals
        ));
    }

    #[test]
    fn covers_json_construction_and_collection() {
        // D-ENC-DYN1=A+: dynamic `DataTree` construction (`DataTree.Text`/
        // `DataTree.Bool`/`DataTree.Array`/`DataTree.Null`) + a `[DataTree]` list value
        // type. A fn that builds and returns `DataTree` values routes (the dynamic value
        // type is a covered foreign value type;
        // construction is the `JSONLit` shape). The if-let MATCHING + index-assign need
        // full sema (the `DataTree` pattern / `IndexKind`), proven by the TIR feature
        // integration targets + the whole-suite byte-parity diff; here we gate the
        // sema-independent construction.
        let src = "\
fn build() => DataTree {
    items: [DataTree] := []
    items.push(DataTree.Text(\"jet\"))
    items.push(DataTree.Bool(true))
    items.push(DataTree.Null)
    return DataTree.Array(items)
}
";
        assert!(covers(src, "build"));
    }

    #[test]
    fn covers_json_value_param_and_array() {
        // A `DataTree` param + list value type + `DataTree.Array` construction.
        let src = "\
fn wrap(x: DataTree) => DataTree {
    items: [DataTree] := []
    items.push(x)
    return DataTree.Array(items)
}
";
        assert!(covers(src, "wrap"));
    }

    #[test]
    fn covers_enum_field_struct() {
        // c109 Phase 24: a struct with an ENUM field (`note_type: NoteType`) is now a
        // covered struct — `field_ty_covered` admits a covered enum field. So a fn that
        // takes/reads such a struct routes (previously the enum field excluded the struct).
        let src = "\
enum NoteType { User Feedback }
struct Note {
    name: String
    note_type: NoteType
}
fn name_of(n: Note) => String {
    return n.name
}
";
        assert!(covers(src, "name_of"));
    }

    #[test]
    fn covers_local_enum_with_foreign_payload_value_type() {
        // c109 Phase 24: a local enum whose variant payload is itself a covered enum is
        // covered (`enum_payload_ty_covered` admits a covered enum). (A FOREIGN-enum
        // payload needs the cross-module `foreign_types` table, proven by the TIR
        // feature integration targets;
        // here a LOCAL nested enum exercises the same payload-covered path.)
        let src = "\
enum Kind { A B }
enum Query {
    Tag(String)
    OfKind(Kind)
}
fn mk(k: Kind) => Query {
    return Query.OfKind(k)
}
";
        assert!(covers(src, "mk"));
    }

    #[test]
    fn covers_comptime_const_in_interpolation() {
        // c109 Phase 24: a comptime const inlines its value at the use site
        // (`cx.consts`), so a fn interpolating a const routes.
        let src = "\
$header :: \"<html>\"
fn wrap(s: String) => String {
    return \"{header}: {s}\"
}
";
        assert!(covers(src, "wrap"));
    }

    #[test]
    fn covers_comptime_local_binding() {
        // c109 (S57/M9.5): a comptime LOCAL `$name :: expr` in a function body
        // routes once sema fills `b.ct`. The runtime `init` (`build()`) is NOT in-subset
        // on its own merits, but the comptime path never emits it — it emits the
        // sema-evaluated literal — so the gate admits it on `b.ct.is_some()`. Needs the
        // full sema pass, hence `covers_after_sema`.
        let src = "\
fn build() => [Int] {
    xs: [Int] := []
    loop i; 1..3 {
        xs.push(i * 10)
    }
    return xs
}
fn run() {
    $xs :: build()
    print(\"{xs}\")
}
";
        assert!(covers_after_sema(src, "run"));
    }

    #[test]
    fn covers_shared_auto_clone_in_free_call_arg() {
        // c109 Phase 6b: a fn with a `Shared<T>` param (`is_covered_shared_ty`) passing
        // that handle to a FREE call inside a loop — sema sets `a.flags.shared_auto_clone`
        // (auto-clone across the loop boundary) — now routes. The gate admits the Arc form
        // on the plain-`Call` path (it lowers via `lower_one_call_arg`'s `arc_clone`). Both
        // `noop` (a `Shared<T>` param) and `loop_user` (the auto-clone call site) are
        // covered. Needs the full sema pass (the flag is sema-resolved), hence
        // `covers_after_sema`.
        let src = "\
fn noop(h: Shared<Int>) {
    print(0)
}
fn loop_user(h: Shared<Int>) {
    loop {
        noop(h)
    }
}
fn run() {
    print(0)
}
";
        assert!(covers_after_sema(src, "noop"));
        assert!(covers_after_sema(src, "loop_user"));
    }

    #[test]
    fn covers_optional_struct_field() {
        // c109 Phase 24: a struct with an OPTIONAL field (`note: String?`) is now covered
        // (`field_ty_covered` admits a covered-payload Option). A fn building it routes.
        let src = "\
struct PR {
    file_path: String
    note: String?
}
fn mk(p: String) => PR {
    return PR.{file_path: p, note: None}
}
";
        assert!(covers(src, "mk"));
    }

    #[test]
    fn covers_numeric_bounds_const() {
        // c109 Phase 28: per-type bounds constants reach codegen as a `Field` whose
        // receiver is a numeric type name (`U8.MAX`, `I32.MIN`, `Float.INFINITY`).
        // Gated structurally (numeric type name + a known const member), no sema fact.
        let src = "\
fn bounds() {
    print(U8.MAX)
    print(I32.MIN)
    print(Float.INFINITY)
}
";
        assert!(covers(src, "bounds"));
    }

    #[test]
    fn rejects_unknown_numeric_member() {
        // A numeric type name with a NON-bounds member is NOT a bounds const — it
        // stays excluded (a non-local non-enum ident receiver), so the fn stays on
        // the AST path. (Sema would reject it too; the gate is conservative.)
        let src = "\
fn bad() {
    print(U8.NOPE)
}
";
        assert!(!covers(src, "bad"));
    }

    #[test]
    fn covers_overflow_opt_builtins() {
        // c109 Phase 28: the overflow opt-outs `wrapping(e)`/`saturating(e)`/
        // `checked(e)` over an integer `Expr::Binary`. Gated structurally (the
        // builtin name + a `+`/`-`/`*`/`/` Binary arg), no sema fact required.
        let src = "\
fn ops(a: U8, b: U8) {
    print(wrapping(a + b))
    print(saturating(a * b))
}
";
        assert!(covers(src, "ops"));
    }

    #[test]
    fn rejects_overflow_opt_nonbinary() {
        // `wrapping(x)` whose argument is NOT an integer `Expr::Binary` is not the
        // covered shape — the gate excludes it (sema never produces it, but the gate
        // stays strict).
        let src = "\
fn nope(x: U8) {
    print(wrapping(x))
}
";
        assert!(!covers(src, "nope"));
    }

    #[test]
    fn covers_generic_optional_return() {
        // c109 Phase 30: a generic fn with a `T?` return whose payload is a type var
        // (`largest<T: Comparable>() -> (T?)`). Before Phase 30 the `T?` payload was
        // excluded (`fallible_payload_covered` admitted no type var) — now it routes.
        // Body is a structural `Val(x)` (a type-var payload `Some(user_x)`).
        let src = "\
fn opt_id<T: Comparable>(x: T) => (T?) {
    return Val(x)
}
";
        assert!(covers(src, "opt_id"));
    }

    #[test]
    fn rejects_optional_return_uncovered_payload() {
        // A `T?` return whose payload is an UNcovered type (a trait object is not a
        // fallible payload) stays excluded — the type-var admission is narrow.
        let src = "\
trait Shape {
    fn area(self) => Float
}
fn maybe_shape(s: Shape) => (Shape?) {
    return Val(s)
}
";
        assert!(!covers(src, "maybe_shape"));
    }

    #[test]
    fn covers_trait_object_param() {
        // c109 Phase 30: a TRAIT-OBJECT param (`s: Shape` → `&Box<dyn user_Shape>`). The
        // param type is admitted (`is_covered_trait_object_ty`); a body with no method
        // call is structurally in-subset (the dynamic-dispatch shape needs sema's
        // `recv_type`, proven by the TIR feature integration targets + parity). An
        // empty body covers.
        let src = "\
trait Shape {
    fn area(self) => Float
    fn name(self) => String
}
fn takes_shape(s: Shape) {
}
";
        assert!(covers(src, "takes_shape"));
    }

    #[test]
    fn covers_trait_object_list_param() {
        // c109 Phase 30: a `[Shape]` trait-object list is a covered collection
        // (`collection_elem_covered` admits a trait-object element). A fn taking one,
        // with no body construct beyond the param, routes.
        let src = "\
trait Shape {
    fn area(self) => Float
}
fn takes_shapes(xs: [Shape]) {
}
";
        assert!(covers(src, "takes_shapes"));
    }

    #[test]
    fn rejects_non_trait_object_named() {
        // `is_covered_trait_object_ty` admits only a name in `cx.trait_names`. A param
        // typed as an unknown name (no such trait/struct) stays excluded — the gate
        // never wrongly treats a plain name as a trait object.
        let src = "\
fn bad(s: Nonexistent) {
}
";
        assert!(!covers(src, "bad"));
    }

    #[test]
    fn covers_recursive_struct_construction() {
        // c109 (recursive struct): constructing a self-referential (boxed) struct is
        // covered — `struct_lit_constructible` admits the boxed edge and lowering wraps the
        // field value `Box::new(…)`. A fn building a nested `Tree { value, child: Val(…) }`
        // routes. (The boxed-field READ is also covered now — see covers_recursive_struct_boxed_field_read.)
        let src = "\
struct Tree {
    value: Int
    child: Tree?
}
fn build() {
    root :: Tree.{ value: 1, child: Val(Tree.{ value: 2, child: None }) }
    print(root.value)
}
";
        assert!(covers(src, "build"));
    }

    #[test]
    fn covers_recursive_struct_boxed_field_read() {
        // c109 (recursive struct read): a boxed (recursive) field READ (`t.child`,
        // Rust type `Box<…>`) is now covered — the read derefs the `Box` (`(*(…))`, a
        // total `boxed` fact lowered from `cx.boxed_edges`), so a recursive struct is a
        // covered VALUE type. A fn reading a boxed child routes through the TIR.
        let src = "\
struct Tree {
    value: Int
    child: Tree?
}
fn first_child(t: Tree) => Int {
    kid: Tree? :: t.child
    if kid == {
        Val(c) -> {
            return c.value
        }
        None -> {
            return 0
        }
    }
    return 0
}
";
        assert!(covers(src, "first_child"));
    }

    #[test]
    fn covers_owning_nonscalar_field_read_clone() {
        // c109: an owning field read of a NON-SCALAR field (`s :: p.name`, `name:
        // String`) — sema rewrites the read to `(p.name).clone()` (a `MethodCall`
        // clone shape). With the single-uppercase-letter struct name `P` now treated
        // as a concrete declared type (not a type var), the whole `main` routes
        // through the TIR. The owning clone emits `((user_p).user_name).clone()`.
        let src = r#"
struct P {
    name: String
}

fn run() {
    p :: P.{ name: "x" }
    s :: p.name
    t :: p.name
    print(s)
    print(t)
}
"#;
        assert!(
            covers_after_sema(src, "run"),
            "owning field-read clone not covered"
        );
    }

    #[test]
    fn covers_indexed_map_assign_through_field() {
        // c109: an indexed map-assign whose base is a FIELD read (`s.scores["a"] = 1`,
        // `scores: [String: Int]`). The `LValue::Index` gate already admits a
        // field-read base + sema-resolved `IndexKind`; the only blocker was the
        // single-uppercase-letter struct name `S` (covered by the type-var-heuristic
        // guard). The whole `main` routes through the TIR; the assign emits
        // `{ let __jet_v = 1i64; jet_map_insert(&mut ((user_s).user_scores), …); }`.
        let src = r#"
struct S {
    scores: [String: Int]
}

fn run() {
    s := S.{ scores: [] }
    s.scores["a"] = 1
    print(s.scores["a"])
}
"#;
        assert!(
            covers_after_sema(src, "run"),
            "map-assign through field not covered"
        );
    }

    #[test]
    fn covers_map_builtin_on_struct_field_receiver() {
        // c109: a map builtin (`.len()`) on a struct-FIELD-read receiver
        // (`s.scores.len()`), where the field was initialized from an empty-map
        // struct-literal field (`scores: [:]` takes its type from the struct field).
        // The builtin gate already admits a field-read receiver + the struct-literal
        // empty-map field is in-subset; the single-uppercase-letter struct name `S` was
        // the only blocker (now a concrete declared type). The whole `main` routes
        // through the TIR.
        let src = r#"
struct S {
    scores: [String: Int]
}

fn run() {
    s := S.{ scores: [] }
    print(s.scores.len())
}
"#;
        assert!(
            covers_after_sema(src, "run"),
            "map builtin on field receiver not covered"
        );
    }

    #[test]
    fn covers_field_read_and_eq_on_inlined_comptime_values() {
        // c109: a FIELD READ off a comptime-const struct value (`$pair_value ::
        // Pair{…}`; then `pair_value.left`) and an `==` against a comptime-const enum value
        // (`$light_value :: Light.Green`; then `light_value == Light.Green`). The const inlines to
        // its pre-rendered Rust value string (`cx.consts[…]`); reading a field off the
        // inlined struct / comparing the inlined enum is byte-identical to the AST path.
        // The Field gate now admits a non-local comptime-const receiver.
        let src = r#"
struct Pair {
    left: Int
    right: String
}

enum Light {
    Red
    Green
}

$pair_value :: Pair.{left: 7, right: "seven"}
$light_value :: Light.Green

fn run() {
    print("{pair_value.left}")
    print("{pair_value.right}")
    print("{light_value == Light.Green}")
}
"#;
        assert!(
            covers_after_sema(src, "run"),
            "field-read/== on inlined comptime values not covered"
        );
    }

    #[test]
    fn covers_wildcard_enum_payload_if_let() {
        // c109 (D-PATW): a user-enum variant if-let condition with a WILDCARD payload
        // slot (`if w == Some(_)`). The `_` binds nothing; the if-let head renders
        // `if let user_Wrapper::user_Some(_) = user_w` (byte-for-byte the AST). Covered
        // when the variant is a single-payload variant of a covered enum.
        let src = "\
enum Wrapper {
    Some(Int)
    Empty
}
fn run() {
    w :: Wrapper.Some(42)
    if w == Some(_) {
        print(\"has value\")
    }
}
";
        assert!(
            covers(src, "run"),
            "wildcard enum-payload if-let not covered"
        );
    }
