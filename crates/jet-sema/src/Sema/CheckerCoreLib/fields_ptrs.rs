use crate::AST::{Expr, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Syntax;
use super::alloc_ptrs::{e3101, ptr_type};
use super::serde_diags::unknown_core_item;
impl<'a> Checker<'a> {
        pub(crate) fn infer_core_field(
            &mut self,
            module: &str,
            name: &str,
            alias_span: Span,
            span: Span,
        ) -> Option<Type> {
            match (module, name) {
                ("core.math", "pi" | "e" | "tau" | "infinity" | "nan") => Some(Type::Float),
                // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): `mem.Arena`, `mem.Bump`,
                // `mem.Pool`, `mem.Fixed` — accessed as a field on the `core.mem` alias,
                // then `.new()` is called on the sentinel type to construct the allocator.
                ("core.mem", "Arena") => Some(Type::Named(Syntax::MEM_ARENA.to_string())),
                ("core.mem", "Bump") => Some(Type::Named(Syntax::MEM_BUMP.to_string())),
                ("core.mem", "Pool") => Some(Type::Named(Syntax::MEM_POOL.to_string())),
                ("core.mem", "Fixed") => Some(Type::Named(Syntax::MEM_FIXED.to_string())),
                // D-OPTGC1: `gc.Gc` sentinel — `.new<T>(value)` constructs a traced handle.
                ("core.gc", "Gc") => Some(Type::Named(Syntax::GC_TYPE.to_string())),
                // D-SOLVER-LIB1=A: `solve.Solver.new(seed)` constructs explicit solver state.
                ("core.solve", "Solver") => Some(Type::Named(Syntax::SOLVER_TYPE.to_string())),
                // D-GAME1/2/3 + D-WD10: static sentinels for `game.Scene.new`,
                // `game.Replay.record`, `game.Backend.headless`, and `game.Budgets.new`.
                ("core.game", "Scene") => Some(Type::Named("GameSceneType".to_string())),
                ("core.game", "Replay") => Some(Type::Named("GameReplayType".to_string())),
                ("core.game", "Backend") => Some(Type::Named("GameBackendType".to_string())),
                ("core.game", "Budgets") => Some(Type::Named("GameBudgetsType".to_string())),
                // D-FIDELITY-API1=A: `core.perf.Perf` static API sentinel.
                ("core.perf", "Perf") => Some(Type::Named("Perf".to_string())),
                _ => {
                    self.diags.push(unknown_core_item(module, name, span));
                    let _ = alias_span;
                    None
                }
            }
        }
    
        /// S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)`. Gated by `use core.mem`
        /// (E3102) and an enclosing `#Unsafe` block (E3101). Returns `Ptr<T>`.
        pub(crate) fn infer_ptr_from_addr(
            &mut self,
            alias: &str,
            alias_span: Span,
            elem: &Type,
            addr: &mut Expr,
            span: Span,
        ) -> Option<Type> {
            // E3102: the discovery gate — the alias must be a `core.mem` import.
            let is_mem = self
                .core_imports
                .get(alias)
                .map(|m| m == Syntax::CORE_MEM_MODULE)
                .unwrap_or(false);
            if !is_mem {
                self.diags.push(self.e3102(alias, alias_span));
                self.infer(addr);
                return None;
            }
            // E3101: pointer construction is a low-level operation; it needs the
            // audit gate.
            if !self.in_unsafe {
                self.diags.push(e3101(Syntax::MEM_FROM_ADDR, span));
            }
            // The address is a plain Int.
            if let Some(t) = self.infer(addr) {
                if t != Type::Int {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` needs an Int address, not {}",
                            Syntax::MEM_FROM_ADDR,
                            t.show()
                        ),
                        "a pointer is built from a numeric machine address".to_string(),
                        "pass an Int, e.g. from `mem.address_of(x)`".to_string(),
                        Some(addr.span()),
                    ));
                }
            }
            Some(ptr_type(elem.clone()))
        }
    
        /// E3102: a `core.mem` item was named without `use core.mem`.
        pub(crate) fn e3102(&self, alias: &str, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E3102",
                format!("`{}` is part of the low-level tier", Syntax::TYPE_PTR),
                format!(
                    "naming `{}`, `{}`, or an allocator needs the discovery gate",
                    Syntax::TYPE_PTR,
                    Syntax::MEM_VOLATILE_READ
                ),
                format!(
                    "add `use {};` and call through `{}.…`",
                    Syntax::CORE_MEM_MODULE,
                    alias
                ),
                Some(span),
            )
        }
    
}
