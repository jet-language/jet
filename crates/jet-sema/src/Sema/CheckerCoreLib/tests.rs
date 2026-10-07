#[cfg(test)]
mod tests {
    use super::super::fixed_sigs::core_fixed_sig;
    use super::super::module_items::core_module_items;
    use crate::AST::Type;

    #[test]
    fn dyn_cutover_has_closed_payloads_and_members() {
        use super::super::{core_json_pattern_types, dyn_member, dyn_method_return};
        let dyn_ty = Type::Named("Dyn".to_string());
        assert_eq!(core_json_pattern_types("Bytes"), Some(vec![Type::List(Box::new(
            Type::IntN { signed: false, bits: 8 },
        ))]));
        assert_eq!(core_json_pattern_types("DateTime"),
            Some(vec![Type::Named("ZonedDateTime".to_string())]));
        for member in ["get", "has", "keys", "remove", "len", "decode", "to_text",
            "field", "at", "int", "float", "text", "bool", "equal_unordered", "encode"] {
            assert!(dyn_member(member), "{member}");
        }
        assert!(!dyn_member("user"));
        assert_eq!(dyn_method_return("get", 1), Some(Type::Option(Box::new(dyn_ty.clone()))));
        assert_eq!(dyn_method_return("remove", 1), Some(Type::Option(Box::new(dyn_ty))));
        assert_eq!(dyn_method_return("has", 1), Some(Type::Bool));
        assert_eq!(dyn_method_return("keys", 0), Some(Type::List(Box::new(Type::String))));
        assert_eq!(dyn_method_return("len", 0), Some(Type::Int));
        assert_eq!(dyn_method_return("get", 0), None);
    }

    #[test]
    fn dyn_automatic_wrap_is_recursive_and_not_encode_in_general() {
        use super::super::dyn_wrappable;
        for ty in [Type::Int, Type::Float, Type::String, Type::Bool,
            Type::Named("Dyn".to_string()), Type::Named("DateTime".to_string()),
            Type::Named("ZonedDateTime".to_string()),
            Type::List(Box::new(Type::IntN { signed: false, bits: 8 }))] {
            assert!(dyn_wrappable(&ty), "{}", ty.show());
            assert!(dyn_wrappable(&Type::List(Box::new(ty.clone()))));
            assert!(dyn_wrappable(&Type::Map {
                key: Box::new(Type::String), key_span: None, value: Box::new(ty),
            }));
        }
        for ty in [Type::Named("Record".to_string()), Type::Named("Decimal".to_string()),
            Type::Char, Type::IntN { signed: true, bits: 32 }, Type::Option(Box::new(Type::Int))] {
            assert!(!dyn_wrappable(&ty), "{}", ty.show());
            assert!(!dyn_wrappable(&Type::List(Box::new(ty))));
        }
        assert!(!dyn_wrappable(&Type::Map {
            key: Box::new(Type::Int), key_span: None, value: Box::new(Type::String),
        }));
    }

    #[test]
    fn raylib_skeleton_signatures_are_registered() {
        let window = core_fixed_sig("core.game.raylib", "window_open")
            .expect("raylib window_open signature")
            .1
            .expect("window_open return type");
        assert_eq!(window, Type::Named("RaylibWindow".to_string()));

        let color = core_fixed_sig("core.game.raylib", "color")
            .expect("raylib color signature")
            .1
            .expect("color return type");
        assert_eq!(color, Type::Named("RaylibColor".to_string()));

        let items = core_module_items("core.game.raylib");
        assert!(items.contains(&"window_ready".to_string()));
        assert!(items.contains(&"draw_text".to_string()));
        assert!(items.contains(&"draw_rectangle".to_string()));
        assert!(items.contains(&"key_down".to_string()));
        assert!(items.contains(&"close_window".to_string()));
    }

    #[test]
    fn base64_url_entries_have_typed_signatures() {
        let encode = core_fixed_sig("core.encoding.base64", "encode_url")
            .expect("encode_url signature");
        assert_eq!(encode.0.len(), 1);
        assert_eq!(encode.1, Some(Type::String));

        let decode = core_fixed_sig("core.encoding.base64", "decode_url")
            .expect("decode_url signature");
        assert_eq!(decode.0.len(), 1);
        assert!(matches!(decode.1, Some(Type::Result { .. })));
    }

    #[test]
    fn core_mem_gate_table_matches_checker_surface() {
        let expected = [
            ("Ptr", crate::Syntax::CoreMemGate::Import),
            ("from_addr", crate::Syntax::CoreMemGate::Audit),
            ("volatile_read", crate::Syntax::CoreMemGate::Audit),
            ("volatile_write", crate::Syntax::CoreMemGate::Audit),
            ("address_of", crate::Syntax::CoreMemGate::Import),
            ("pin", crate::Syntax::CoreMemGate::Import),
            ("Pin", crate::Syntax::CoreMemGate::Import),
            ("Arena", crate::Syntax::CoreMemGate::Import),
            ("Bump", crate::Syntax::CoreMemGate::Import),
            ("Pool", crate::Syntax::CoreMemGate::Import),
            ("Fixed", crate::Syntax::CoreMemGate::Import),
            ("AllocError", crate::Syntax::CoreMemGate::Import),
            ("Atomic", crate::Syntax::CoreMemGate::Import),
        ];

        assert_eq!(
            crate::Syntax::CORE_MEM_GATE_TIERS,
            expected.as_slice(),
            "every checker-facing core.mem item must have its expected tier"
        );
        let expected_items = expected
            .iter()
            .map(|(item, _)| (*item).to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            core_module_items(crate::Syntax::CORE_MEM_MODULE),
            expected_items,
            "every checker-facing core.mem item must be exported"
        );
        for (item, gate) in expected {
            assert_eq!(
                crate::Syntax::core_mem_gate(item),
                Some(gate),
                "{item} must keep its expected gate"
            );
            assert_eq!(
                crate::Syntax::core_mem_requires_audit(item),
                gate == crate::Syntax::CoreMemGate::Audit,
                "{item} checker behavior must match its expected tier"
            );
        }
        assert!(
            crate::Syntax::core_mem_requires_audit("unknown_core_mem_item"),
            "unknown core.mem items must fail closed"
        );
    }

    #[test]
    fn plain_core_rows_keep_typed_signature_arity() {
        let drifted: Vec<String> = crate::Syntax::CORE_CALLS
            .iter()
            .filter_map(|row| {
                let (params, _) = core_fixed_sig(row.module, row.member)?;
                (params.len() != row.arity()).then(|| {
                    format!(
                        "{}.{} typed={} registry={}",
                        row.module,
                        row.member,
                        params.len(),
                        row.arity()
                    )
                })
            })
            .collect();
        assert!(
            drifted.is_empty(),
            "typed sema signature drifted for: {}",
            drifted.join(", ")
        );
    }
}
