#[cfg(test)]
mod tests {
    use super::super::fixed_sigs::core_fixed_sig;
    use super::super::module_items::core_module_items;
    use crate::AST::Type;

    #[test]
    fn raylib_skeleton_signatures_are_registered() {
        let window = core_fixed_sig("core.raylib", "window_open")
            .expect("raylib window_open signature")
            .1
            .expect("window_open return type");
        assert_eq!(window, Type::Named("RaylibWindow".to_string()));

        let color = core_fixed_sig("core.raylib", "color")
            .expect("raylib color signature")
            .1
            .expect("color return type");
        assert_eq!(color, Type::Named("RaylibColor".to_string()));

        let items = core_module_items("core.raylib");
        assert!(items.contains(&"window_ready".to_string()));
        assert!(items.contains(&"draw_text".to_string()));
        assert!(items.contains(&"draw_rectangle".to_string()));
        assert!(items.contains(&"key_down".to_string()));
        assert!(items.contains(&"close_window".to_string()));
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
        for row in crate::Syntax::CORE_CALLS {
            if let Some((params, _)) = core_fixed_sig(row.module, row.member) {
                assert_eq!(
                    params.len(),
                    row.arity(),
                    "typed sema signature drifted for {}.{}",
                    row.module,
                    row.member
                );
            }
        }
    }
}
