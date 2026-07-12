use super::*;
impl<'a> Checker<'a> {
        pub(crate) fn push_scope(&mut self) {
            self.scopes.push(HashMap::new());
            self.lambda_mut_borrow_stack.push(HashSet::new());
            self.ct_scopes.push(HashMap::new());
        }
    
        pub(crate) fn pop_scope(&mut self) {
            self.lint_unjoined_tasks_in_current_scope();
            self.check_single_use_consumed_in_current_scope();
            self.drop_scope_no_obligation_checks();
        }

        pub(crate) fn drop_scope_no_obligation_checks(&mut self) {
            // D-ALLOC2: arena `view`s declared in the scope being popped leave their
            // region here — drop their bookkeeping so a same-named binding in an
            // outer scope isn't mistaken for the view. The arena binding itself
            // lives in `self.scopes`, so it pops alongside.
            let depth = self.scopes.len();
            self.arena_views.retain(|_, v| v.scope_len < depth);
            // D-DYNARRAY1: `View<T>` bindings leave scope the same way arena views do.
            self.list_views.retain(|_, v| v.scope_len < depth);
            // D-MEM1 S5: string-`view` bindings (`.trim()`/`.after()`/`.before()`)
            // leave scope the same way.
            self.string_views.retain(|_, v| v.scope_len < depth);
            self.scopes.pop();
            self.lambda_mut_borrow_stack.pop();
            self.ct_scopes.pop();
        }
    
        pub(crate) fn lambda_mut_borrow_active(&self, name: &str) -> bool {
            self.lambda_mut_borrow_stack
                .iter()
                .any(|s| s.contains(name))
        }
    
        pub(crate) fn current_ct_globals(&self) -> HashMap<String, crate::Comptime::CtValue> {
            let mut globals = self.ct_globals.clone();
            for scope in &self.ct_scopes {
                for (name, value) in scope {
                    globals.insert(name.clone(), value.clone());
                }
            }
            globals
        }
    
        pub(crate) fn lookup(&self, name: &str) -> Option<&LocalInfo> {
            self.scopes.iter().rev().find_map(|s| s.get(name))
        }
    
        /// A binding is borrowed (a `view`) when it is a `Read` parameter of a
        /// non-scalar type — in v1 those lower to `&T`, so the value can't be moved
        /// out of it. Used to decide where a consuming use must clone (B1).
        pub(crate) fn is_borrowed_binding(&self, name: &str) -> bool {
            self.lookup(name)
                .map(|info| {
                    matches!(info.param_conv, Some(AccessConvention::Read)) && !info.ty.is_scalar()
                })
                .unwrap_or(false)
        }
    
        pub(crate) fn declare(&mut self, name: &str, name_span: Span, info: LocalInfo) {
            if self.lookup(name).is_some() || self.consts.contains_key(name) {
                self.diags.push(already_defined(name, name_span));
            }
            // D-CONFUSE1=A (L0503): homoglyph confusable pairs in one scope.
            // `l`/`I`/`1` and `O`/`0` are virtually identical in most fonts.
            if name.len() == 1 {
                let confusable_group: Option<&[char]> = if "l1I".contains(name) {
                    Some(&['l', '1', 'I'])
                } else if "O0".contains(name) {
                    Some(&['O', '0'])
                } else {
                    None
                };
                if let Some(group) = confusable_group {
                    for scope in self.scopes.iter() {
                        for existing in scope.keys() {
                            if existing.len() == 1
                                && existing != name
                                && group.contains(&existing.chars().next().unwrap_or(' '))
                            {
                                self.diags.push(Diagnostic::lint(
                                    "L0503",
                                    format!("`{name}` and `{existing}` are visually confusable in this scope"),
                                    "single-character names like `l`, `1`, `I`, `O`, and `0` look identical in many fonts — readers may misread the code".to_string(),
                                    format!("rename `{name}` (or `{existing}`) to a longer, unambiguous name"),
                                    Some(name_span),
                                ));
                                break;
                            }
                        }
                    }
                }
            }
            self.moved.remove(name);
            self.scopes
                .last_mut()
                .unwrap()
                .insert(name.to_string(), info);
        }
    
        pub(crate) fn declare_loop_var(&mut self, name: String, name_span: Span, ty: &Type) {
            if self.lookup(&name).is_some() || self.consts.contains_key(&name) {
                self.diags.push(already_defined(&name, name_span));
            } else {
                self.scopes.last_mut().unwrap().insert(
                    name,
                    LocalInfo {
                        def_span: name_span,
                        ty: ty.clone(),
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        sendable: true,
                        task_lint_span: None,
                        single_use_span: None,
                        task_has_view_capture: false,
                    },
                );
            }
        }
    
}
