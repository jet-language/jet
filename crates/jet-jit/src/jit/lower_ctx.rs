use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, Block, InstBuilder, TrapCode, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};
use jet_codegen::Codegen::TIR::{
    self, TBuiltinOp, TCallArg, TCoreClosureKind, TEnumPayload, TExpr, TExprKind, THandleOp,
    TIfCond, TJitSpawnLambda, TModuleCallForm, TOrFallback, TStmt, TStrPart,
};
use jet_foundation::AST::{BinOp, IncDecOp, Type, UnOp};
use std::collections::HashMap;

use super::runtime_host::HostFns;
use super::safety::{
    collect_select_arms_jit, flatten_string, jit_list_float_type, jit_list_int_type,
    jit_list_iter_elem_type, jit_value_type, record_type_key, user_type_name,
};
use super::types_meta::{clif_ty, init_clif_ty, JitMeta};
use super::JitRuntime;

#[derive(Clone)]
pub(crate) struct LoopTargets {
    label: Option<String>,
    continue_block: Block,
    break_block: Block,
    shield_depth: u32,
}

/// One JIT-lowering pass over a single function's `TStmt`/`TExpr` tree into
/// Cranelift IR, via `FunctionBuilder`. This is the "JIT lower" consumer of
/// R12 (`docs/spec/architecture.md`): the Rust AOT emitter
/// (`jet-codegen/src/Codegen/TIR/emit/*.rs`) is the other. Both must handle
/// every executable TIR variant, either with a real lowering or a named
/// internal "unsupported" arm that falls through transparently — this file
/// never uses a bare `_ =>` over a TIR enum (`TStmt`/`TExprKind`/`TBuiltinOp`/
/// `THandleOp`/`TIfCond`) so a new variant fails this match at compile time in
/// every JIT site, not silently at run time. "Unsupported here" is not a bug:
/// `AotFallbackBackend`/`InterpreterBackend` (`Source/JitBackend.rs`) retry the
/// same program through the AOT path and then the tier-0 interpreter, so an
/// `Err` from this file only costs JIT speed, never correctness (D-JIT1).
pub(crate) struct LowerCtx<'a, 'b> {
    pub(crate) b: &'a mut FunctionBuilder<'b>,
    pub(crate) module: &'a mut JITModule,
    pub(crate) host: &'a HostFns,
    pub(crate) runtime: &'a mut JitRuntime,
    pub(crate) meta: &'a JitMeta<'a>,
    pub(crate) vars: &'a mut HashMap<String, Variable>,
    pub(crate) var_tys: &'a mut HashMap<String, Type>,
    pub(crate) func_ids: &'a HashMap<String, FuncId>,
    pub(crate) spawn_site: &'a mut usize,
    pub(crate) spawn_func_ids: &'a [FuncId],
    pub(crate) spawn_lambdas: &'a [TJitSpawnLambda],
    pub(crate) loop_stack: Vec<LoopTargets>,
    pub(crate) dead: bool,
    pub(crate) next_var: u32,
    /// Owning struct for inherent methods (`Point::dist_sq` → `Point`).
    pub(crate) method_struct: Option<String>,
    /// CLIF return type of the function being lowered (`None` = returns void).
    /// Drives the dummy value `emit_trap_check` returns on the trap-unwind path.
    pub(crate) ret_clif: Option<types::Type>,
    /// Lexical `#Shield` depth in emitted native code. Used to emit exact
    /// cleanup calls before every non-local control-flow edge.
    pub(crate) shield_depth: u32,
}

impl LowerCtx<'_, '_> {
    fn emit_dummy_return(&mut self) {
        match self.ret_clif {
            Some(ty) => {
                let value = if ty == types::F64 {
                    self.b.ins().f64const(0.0)
                } else {
                    self.b.ins().iconst(ty, 0)
                };
                self.b.ins().return_(&[value]);
            }
            None => {
                self.b.ins().return_(&[]);
            }
        }
    }

    fn emit_shield_leaves_to(&mut self, target_depth: u32) -> Option<Value> {
        let leave_ref = self
            .module
            .declare_func_in_func(self.host.conc.shield_leave, self.b.func);
        let mut status = None;
        for _ in target_depth..self.shield_depth {
            let call = self.b.ins().call(leave_ref, &[]);
            status = Some(self.b.inst_results(call)[0]);
        }
        status
    }

    fn emit_pending_interrupt_check(&mut self, status: Value) {
        let zero = self.b.ins().iconst(types::I64, 0);
        let pending = self.b.ins().icmp(IntCC::NotEqual, status, zero);
        let interrupted = self.b.create_block();
        let cont = self.b.create_block();
        self.b.ins().brif(pending, interrupted, &[], cont, &[]);
        self.b.switch_to_block(interrupted);
        self.b.seal_block(interrupted);
        self.emit_dummy_return();
        self.b.switch_to_block(cont);
        self.b.seal_block(cont);
    }

    /// Finish a scheduler wait host call. Host shims return only a typed status;
    /// the value lives in thread-local storage so no Rust unwind or aggregate ABI
    /// crosses the Cranelift frame.
    fn finish_wait_call(&mut self, status: Value) -> Value {
        let zero = self.b.ins().iconst(types::I64, 0);
        let interrupted = self.b.ins().icmp(IntCC::NotEqual, status, zero);
        let unwind = self.b.create_block();
        let ready = self.b.create_block();
        self.b.ins().brif(interrupted, unwind, &[], ready, &[]);
        self.b.switch_to_block(unwind);
        self.b.seal_block(unwind);
        self.emit_shield_leaves_to(0);
        self.emit_dummy_return();
        self.b.switch_to_block(ready);
        self.b.seal_block(ready);
        let value_ref = self
            .module
            .declare_func_in_func(self.host.conc.wait_value, self.b.func);
        let call = self.b.ins().call(value_ref, &[]);
        self.b.inst_results(call)[0]
    }

    fn result_new(&mut self, ok: bool, inner: &TExpr) -> Result<Value, String> {
        let tag = self.b.ins().iconst(types::I8, i64::from(ok));
        let (host_id, payload) = if matches!(&inner.ty, Type::Named(n) if n == "Unit" || n == "Void") {
            (self.host.result_new_i64, self.b.ins().iconst(types::I64, 0))
        } else {
            let value = self.lower_expr(inner)?;
            let host = match clif_ty(&inner.ty) {
                Some(ty) if ty == types::F64 => self.host.result_new_f64,
                Some(ty) if ty == types::I8 => self.host.result_new_i8,
                Some(ty) if ty == types::I32 => self.host.result_new_i32,
                Some(ty) if ty == types::I64 => self.host.result_new_i64,
                _ => return Err(format!("jit Result payload unsupported: {:?}", inner.ty)),
            };
            (host, value)
        };
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        let call = self.b.ins().call(host_ref, &[tag, payload]);
        Ok(self.b.inst_results(call)[0])
    }

    fn result_payload(&mut self, handle: Value, ty: &Type) -> Result<Value, String> {
        if matches!(ty, Type::Named(n) if n == "Unit" || n == "Void") {
            return Ok(self.b.ins().iconst(types::I8, 0));
        }
        let host_id = match clif_ty(ty) {
            Some(clif) if clif == types::F64 => self.host.result_get_f64,
            Some(clif) if clif == types::I8 => self.host.result_get_i8,
            Some(clif) if clif == types::I32 => self.host.result_get_i32,
            Some(clif) if clif == types::I64 => self.host.result_get_i64,
            _ => return Err(format!("jit Result payload unsupported: {ty:?}")),
        };
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        let call = self.b.ins().call(host_ref, &[handle]);
        Ok(self.b.inst_results(call)[0])
    }

    fn lower_try(&mut self, inner: &TExpr, convert: &TIR::TTryConvert) -> Result<Value, String> {
        if !matches!(convert, TIR::TTryConvert::None) {
            return Err("jit typed Result conversion unsupported".to_string());
        }
        let handle = self.lower_expr(inner)?;
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[handle]);
        let is_ok = self.b.inst_results(status_call)[0];
        let ok_block = self.b.create_block();
        let err_block = self.b.create_block();
        self.b.ins().brif(is_ok, ok_block, &[], err_block, &[]);
        self.b.switch_to_block(err_block);
        self.b.seal_block(err_block);
        self.emit_shield_leaves_to(0);
        self.b.ins().return_(&[handle]);
        self.b.switch_to_block(ok_block);
        self.b.seal_block(ok_block);
        let ok_ty = inner
            .ty
            .unwrap_result()
            .map(|(ok, _)| ok)
            .ok_or("jit try operand is not Result")?;
        self.result_payload(handle, ok_ty)
    }

    fn loop_targets(&self, label: Option<&str>, kind: &str) -> Result<LoopTargets, String> {
        match label {
            Some(name) => self
                .loop_stack
                .iter()
                .rev()
                .find(|targets| targets.label.as_deref() == Some(name))
                .cloned()
                .ok_or_else(|| format!("jit {kind} to unknown loop label `{name}`")),
            None => self
                .loop_stack
                .last()
                .cloned()
                .ok_or_else(|| format!("jit {kind} outside loop")),
        }
    }

    pub(crate) fn fresh_var(&mut self, ty: cranelift_codegen::ir::Type) -> Variable {
        let var = Variable::from_u32(self.next_var);
        self.next_var += 1;
        self.b.declare_var(var, ty);
        var
    }

    /// Lower a `TStmt` sequence in the current block. `self.dead` (set by
    /// `Return`/`Break`/`Continue`) short-circuits `lower_stmt` so statements
    /// after an unconditional jump are skipped rather than emitted into an
    /// already-terminated Cranelift block (Cranelift rejects instructions
    /// after a block terminator).
    pub(crate) fn lower_stmts(&mut self, stmts: &[TStmt]) -> Result<(), String> {
        for stmt in stmts {
            self.lower_stmt(stmt)?;
        }
        Ok(())
    }

    /// After any call that may set the runtime's trapped flag — a fallible host
    /// shim (checked arith, list get/set/slice, channel receive/panic) or a call
    /// to another jet function (a callee's trap must propagate transitively) —
    /// read the flag and, if set, branch to this function's epilogue returning a
    /// dummy value. The whole unwind is Cranelift control flow: no Rust panic is
    /// ever unwound through a JIT frame (cranelift-jit emits no unwind tables —
    /// doing so would be UB, forbidden by I1). `resident_invoke` observes the
    /// flag after `main` returns and reports the trap as `E0953`.
    fn emit_trap_check(&mut self) -> Result<(), String> {
        let is_ref = self
            .module
            .declare_func_in_func(self.host.is_trapped, self.b.func);
        let call = self.b.ins().call(is_ref, &[]);
        let flag = self.b.inst_results(call)[0];
        let zero = self.b.ins().iconst(types::I64, 0);
        let trapped = self.b.ins().icmp(IntCC::NotEqual, flag, zero);
        let epilogue = self.b.create_block();
        let cont = self.b.create_block();
        self.b.ins().brif(trapped, epilogue, &[], cont, &[]);

        self.b.switch_to_block(epilogue);
        self.b.seal_block(epilogue);
        self.emit_shield_leaves_to(0);
        self.emit_dummy_return();

        self.b.switch_to_block(cont);
        self.b.seal_block(cont);
        Ok(())
    }

    /// `lower_stmts` for a fresh block scope (loop body, if-branch): resets
    /// `dead` because a `break`/`return` in a PRIOR sibling branch must not
    /// suppress this branch's own statements.
    fn lower_stmts_scoped(&mut self, stmts: &[TStmt]) -> Result<(), String> {
        self.dead = false;
        self.lower_stmts(stmts)
    }

    /// Exhaustive match on every `TStmt` variant (`TIR/mod.rs`) — the JIT half
    /// of the R12 two-consumer contract; `TIR/emit/statements.rs::emit_tir_stmt`
    /// is the AOT half. Control-flow variants (`If`/`Loop`/`While`/`CountedLoop`/
    /// `Range`/`EnumMatch`/`MixedSwitch`) build real Cranelift blocks with
    /// explicit `seal_block` calls — sealing before all predecessors are wired
    /// is a real bug class here (Cranelift block-param SSA), so each loop shape
    /// seals header/body/exit in a fixed order matched to its jump topology.
    /// Named-unsupported variants (destructures other than tuple, `Unsafe`,
    /// `Reactive`, `Layout`, …) return `Err` rather than a wildcard so a new
    /// `TStmt` variant fails to compile here, not silently drops at runtime.
    fn lower_stmt(&mut self, stmt: &TStmt) -> Result<(), String> {
        if self.dead {
            return Ok(());
        }
        match stmt {
            TStmt::Let { name, init, .. } => {
                let val = self.lower_expr(init)?;
                let ty = init_clif_ty(init)?;
                let var = self.fresh_var(ty);
                self.b.def_var(var, val);
                self.vars.insert(TIR::local_place(name), var);
                self.var_tys.insert(TIR::local_place(name), init.ty.clone());
            }
            // D-TUPLE-DESTRUCT1: `(tx, rx) := tasks.channel<T>()`. The coverage gate
            // (`resident_safe_stmt`) admitted only this exact shape: a 2-element
            // `TupleDestructure` whose init is the `tasks.channel` producer, canonical
            // field order `(sender, receiver)`. Reproduce the old single-handle
            // `let ch := tasks.channel(); s := ch.sender();` host calls — `channel_new`
            // for the receiver handle, then `channel_sender` on it for the sender
            // handle — both fired here instead of at a later `.sender()` call.
            TStmt::TupleDestructure { init, binds, .. } => {
                if matches!(
                    &init.kind,
                    TExprKind::CoreCall { module, method, args }
                        if module == "core.tasks" && method == "channel" && args.is_empty()
                ) {
                    let ch_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.channel_new, self.b.func);
                    let ch_call = self.b.ins().call(ch_ref, &[]);
                    let ch_val = self.b.inst_results(ch_call)[0];
                    let tx_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.channel_sender, self.b.func);
                    let tx_call = self.b.ins().call(tx_ref, &[ch_val]);
                    let tx_val = self.b.inst_results(tx_call)[0];
                    // `binds[i].0` is already the mangled Rust name (`mangle(elem.name)`,
                    // set at TIR lowering — unlike plain `Let.name`, which is the raw Jet
                    // name and needs `local_place`'s own `mangle` call). Use it directly.
                    let tx_var = self.fresh_var(types::I64);
                    self.b.def_var(tx_var, tx_val);
                    self.vars.insert(binds[0].0.clone(), tx_var);
                    self.var_tys.insert(binds[0].0.clone(), Type::Int);
                    let ch_var = self.fresh_var(types::I64);
                    self.b.def_var(ch_var, ch_val);
                    self.vars.insert(binds[1].0.clone(), ch_var);
                    self.var_tys.insert(binds[1].0.clone(), Type::Int);
                } else {
                    self.lower_tuple_destructure(init, binds)?;
                }
            }
            TStmt::Assign {
                place, op, value, ..
            } => {
                let var = self
                    .vars
                    .get(place)
                    .copied()
                    .ok_or_else(|| format!("jit assign to unknown place `{place}`"))?;
                let val = if let Some(op) = op {
                    let current = self.b.use_var(var);
                    let rhs = self.lower_expr(value)?;
                    self.apply_binop_to_var(current, *op, rhs, &value.ty)?
                } else {
                    self.lower_expr(value)?
                };
                self.b.def_var(var, val);
            }
            TStmt::Return(Some(expr)) => {
                let val = self.lower_expr(expr)?;
                self.emit_shield_leaves_to(0);
                self.b.ins().return_(&[val]);
                self.dead = true;
            }
            TStmt::Return(None) => {
                self.emit_shield_leaves_to(0);
                self.b.ins().return_(&[]);
                self.dead = true;
            }
            TStmt::ExprStmt(expr) => {
                self.lower_expr(expr)?;
            }
            TStmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                let cond_val = match cond {
                    TIfCond::Plain(e) => self.lower_expr(e)?,
                    // IfLet/IsNone/Matches lower to a pre-computed `matches!`/`is_none`
                    // string in the AOT emitter (TIR/emit/statements.rs); the JIT has no
                    // pattern-test lowering, so each is named (not `_`) to keep this match
                    // exhaustive over `TIfCond`.
                    TIfCond::IfLet { .. } => {
                        return Err("jit if-let condition unsupported".to_string());
                    }
                    TIfCond::IsNone { .. } => {
                        return Err("jit is-none condition unsupported".to_string());
                    }
                    TIfCond::Matches { .. } => {
                        return Err("jit pattern-match condition unsupported".to_string());
                    }
                };
                let then_block = self.b.create_block();
                let else_block = self.b.create_block();
                let merge_block = self.b.create_block();
                self.b
                    .ins()
                    .brif(cond_val, then_block, &[], else_block, &[]);

                self.b.switch_to_block(then_block);
                self.b.seal_block(then_block);
                self.lower_stmts_scoped(then_body)?;
                let then_reaches_merge = !self.dead;
                if then_reaches_merge {
                    self.b.ins().jump(merge_block, &[]);
                }

                self.b.switch_to_block(else_block);
                self.b.seal_block(else_block);
                // Reachability is branch-local. In particular, an absent else
                // is a live fallthrough even when the then branch returned.
                self.dead = false;
                if let Some(body) = else_body {
                    self.lower_stmts(body)?;
                }
                let else_reaches_merge = !self.dead;
                if else_reaches_merge {
                    self.b.ins().jump(merge_block, &[]);
                }

                if then_reaches_merge || else_reaches_merge {
                    self.b.switch_to_block(merge_block);
                    self.b.seal_block(merge_block);
                    self.dead = false;
                } else {
                    // Both branches terminated. Leave the unreferenced merge
                    // block detached and keep later statements unreachable.
                    self.dead = true;
                }
            }
            TStmt::Loop { label, body } => {
                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                self.b.seal_block(header);
                self.b.ins().jump(body_block, &[]);

                self.loop_stack.push(LoopTargets {
                    label: label.clone(),
                    continue_block: header,
                    break_block: exit,
                    shield_depth: self.shield_depth,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(header, &[]);
                }

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::While {
                label, cond, body, ..
            } => {
                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let cond_val = self.lower_expr(cond)?;
                self.b.ins().brif(cond_val, body_block, &[], exit, &[]);

                self.loop_stack.push(LoopTargets {
                    label: label.clone(),
                    continue_block: header,
                    break_block: exit,
                    shield_depth: self.shield_depth,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(header, &[]);
                    self.b.seal_block(header);
                }

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::CountedLoop {
                label,
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.lower_stmt(init)?;
                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let cond_val = self.lower_expr(cond)?;
                self.b.ins().brif(cond_val, body_block, &[], exit, &[]);

                self.loop_stack.push(LoopTargets {
                    label: label.clone(),
                    continue_block: header,
                    break_block: exit,
                    shield_depth: self.shield_depth,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                if !self.dead {
                    self.lower_stmt(step)?;
                }
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(header, &[]);
                    self.b.seal_block(header);
                }

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::Range {
                label,
                var,
                start,
                end,
                step,
                body,
                ..
            } => {
                let start_val = self.lower_expr(start)?;
                let end_val = self.lower_expr(end)?;
                let loop_var = self.fresh_var(types::I64);
                self.b.def_var(loop_var, start_val);
                self.vars.insert(TIR::local_place(var), loop_var);
                self.var_tys.insert(TIR::local_place(var), Type::Int);

                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let step_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let cur = self.b.use_var(loop_var);
                let past_end = self.b.ins().icmp(IntCC::SignedGreaterThan, cur, end_val);
                self.b.ins().brif(past_end, exit, &[], body_block, &[]);

                self.loop_stack.push(LoopTargets {
                    label: label.clone(),
                    continue_block: step_block,
                    break_block: exit,
                    shield_depth: self.shield_depth,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(step_block, &[]);
                }

                self.b.switch_to_block(step_block);
                self.b.seal_block(step_block);
                let cur = self.b.use_var(loop_var);
                let stride = if let Some(step_expr) = step {
                    self.lower_expr(step_expr)?
                } else {
                    self.b.ins().iconst(types::I64, 1)
                };
                let next = self.b.ins().iadd(cur, stride);
                self.b.def_var(loop_var, next);
                self.b.ins().jump(header, &[]);
                self.b.seal_block(header);

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::Break(label) => {
                let targets = self.loop_targets(label.as_deref(), "break")?;
                if let Some(status) = self.emit_shield_leaves_to(targets.shield_depth) {
                    let zero = self.b.ins().iconst(types::I64, 0);
                    let pending = self.b.ins().icmp(IntCC::NotEqual, status, zero);
                    let interrupted = self.b.create_block();
                    self.b.ins().brif(
                        pending,
                        interrupted,
                        &[],
                        targets.break_block,
                        &[],
                    );
                    self.b.switch_to_block(interrupted);
                    self.b.seal_block(interrupted);
                    self.emit_dummy_return();
                } else {
                    self.b.ins().jump(targets.break_block, &[]);
                }
                self.dead = true;
            }
            TStmt::Continue(label) => {
                let targets = self.loop_targets(label.as_deref(), "continue")?;
                if let Some(status) = self.emit_shield_leaves_to(targets.shield_depth) {
                    let zero = self.b.ins().iconst(types::I64, 0);
                    let pending = self.b.ins().icmp(IntCC::NotEqual, status, zero);
                    let interrupted = self.b.create_block();
                    self.b.ins().brif(
                        pending,
                        interrupted,
                        &[],
                        targets.continue_block,
                        &[],
                    );
                    self.b.switch_to_block(interrupted);
                    self.b.seal_block(interrupted);
                    self.emit_dummy_return();
                } else {
                    self.b.ins().jump(targets.continue_block, &[]);
                }
                self.dead = true;
            }
            TStmt::IndexAssign {
                base,
                index,
                is_map,
                value,
            } => {
                if *is_map {
                    return Err("jit map assign unsupported".to_string());
                }
                let list = self.lower_expr(base)?;
                let idx = self.lower_expr(index)?;
                let val = self.lower_expr(value)?;
                let line = self.b.ins().iconst(types::I32, 1);
                let host_id = match &value.ty {
                    Type::Float => self.host.coll.list_set_f64,
                    _ => self.host.coll.list_set,
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                self.b.ins().call(host_ref, &[list, idx, val, line]);
                self.emit_trap_check()?;
            }
            TStmt::ForIn {
                label,
                var,
                collection_str,
                body,
                ..
            } => {
                let coll_place = collection_str.trim().to_string();
                let coll_ty = self
                    .var_tys
                    .get(&coll_place)
                    .ok_or_else(|| format!("jit for-in unknown collection `{coll_place}`"))?;
                let elem_ty = jit_list_iter_elem_type(coll_ty).ok_or_else(|| {
                    format!("jit for-in collection type unsupported: {coll_ty:?}")
                })?;
                let coll_var = self
                    .vars
                    .get(&coll_place)
                    .copied()
                    .ok_or_else(|| format!("jit for-in unknown collection `{coll_place}`"))?;
                let header = self.b.create_block();
                let body_block = self.b.create_block();
                let step_block = self.b.create_block();
                let exit = self.b.create_block();
                let idx_var = self.fresh_var(types::I64);
                let zero = self.b.ins().iconst(types::I64, 0);
                self.b.def_var(idx_var, zero);
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let idx = self.b.use_var(idx_var);
                let coll = self.b.use_var(coll_var);
                let len_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_len, self.b.func);
                let len_call = self.b.ins().call(len_ref, &[coll]);
                let len = self.b.inst_results(len_call)[0];
                let done = self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
                self.b.ins().brif(done, exit, &[], body_block, &[]);

                self.loop_stack.push(LoopTargets {
                    label: label.clone(),
                    continue_block: step_block,
                    break_block: exit,
                    shield_depth: self.shield_depth,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                let line = self.b.ins().iconst(types::I32, 1);
                let get_ref = self.module.declare_func_in_func(
                    match elem_ty {
                        Type::Float => self.host.coll.list_get_f64,
                        _ => self.host.coll.list_get,
                    },
                    self.b.func,
                );
                let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
                let elem = self.b.inst_results(get_call)[0];
                self.emit_trap_check()?;
                let loop_var =
                    self.fresh_var(clif_ty(&elem_ty).ok_or_else(|| {
                        format!("jit for-in element type unsupported: {elem_ty:?}")
                    })?);
                self.b.def_var(loop_var, elem);
                self.vars.insert(TIR::local_place(var), loop_var);
                self.var_tys.insert(TIR::local_place(var), elem_ty);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(step_block, &[]);
                }

                self.b.switch_to_block(step_block);
                self.b.seal_block(step_block);
                let idx = self.b.use_var(idx_var);
                let one = self.b.ins().iconst(types::I64, 1);
                let next = self.b.ins().iadd(idx, one);
                self.b.def_var(idx_var, next);
                self.b.ins().jump(header, &[]);
                self.b.seal_block(header);

                self.b.switch_to_block(exit);
                self.b.seal_block(exit);
                self.dead = false;
            }
            TStmt::EnumMatch {
                scrutinee,
                arms,
                else_body,
                fallthrough,
            } => {
                let subj = self.scrutinee_value(scrutinee)?;
                let merge = self.b.create_block();
                let mut tail = self.b.create_block();
                self.b.ins().jump(tail, &[]);
                // Sema already proved this match exhaustive (E0307): every path is
                // either an arm, the else, or (when `fallthrough`) the trap below —
                // never a silent fall-through past the whole construct. So the
                // merge block is reachable only via a path that didn't itself
                // terminate (return/break/…); track that like `TStmt::If` does,
                // rather than assuming the construct is always live afterward.
                let mut any_reaches_merge = false;
                for arm in arms {
                    self.b.switch_to_block(tail);
                    self.b.seal_block(tail);
                    let disc = self
                        .meta
                        .enum_variant_disc(&arm.pattern)
                        .ok_or_else(|| format!("jit enum pattern `{}`", arm.pattern))?;
                    let then_block = self.b.create_block();
                    let next = self.b.create_block();
                    let disc_const = self.b.ins().iconst(types::I64, disc);
                    let mask = self.b.ins().iconst(types::I64, 0xff);
                    let actual_disc = self.b.ins().band(subj, mask);
                    let eq = self.bool_from_icmp(IntCC::Equal, actual_disc, disc_const);
                    self.b.ins().brif(eq, then_block, &[], next, &[]);
                    self.b.switch_to_block(then_block);
                    self.b.seal_block(then_block);
                    if let Some(open) = arm.pattern.find('(') {
                        if let Some(close) = arm.pattern[open + 1..].find(')') {
                            let binding = arm.pattern[open + 1..open + 1 + close].trim();
                            if !binding.is_empty() && !binding.chars().any(|ch| matches!(ch, '|' | ',' | '{')) {
                                let payload = self.b.ins().sshr_imm(subj, 8);
                                let var = self.fresh_var(types::I64);
                                self.b.def_var(var, payload);
                                self.vars.insert(binding.to_string(), var);
                                self.var_tys.insert(binding.to_string(), Type::Int);
                            }
                        }
                    }
                    self.lower_stmts_scoped(&arm.body)?;
                    if !self.dead {
                        self.b.ins().jump(merge, &[]);
                        any_reaches_merge = true;
                    }
                    tail = next;
                }
                self.b.switch_to_block(tail);
                self.b.seal_block(tail);
                if let Some(body) = else_body {
                    self.lower_stmts_scoped(body)?;
                    if !self.dead {
                        self.b.ins().jump(merge, &[]);
                        any_reaches_merge = true;
                    }
                } else if *fallthrough {
                    self.b.ins().trap(TrapCode::UnreachableCodeReached);
                } else if !self.dead {
                    self.b.ins().jump(merge, &[]);
                    any_reaches_merge = true;
                }
                if any_reaches_merge {
                    self.b.switch_to_block(merge);
                    self.b.seal_block(merge);
                    self.dead = false;
                } else {
                    // Every arm (and the else/trap fallback) terminated. Leave the
                    // unreferenced merge block detached and keep later statements
                    // unreachable — same convention as `TStmt::If`.
                    self.dead = true;
                }
            }
            TStmt::MixedSwitch {
                arms, else_body, ..
            } => {
                let merge = self.b.create_block();
                let mut tail = self.b.create_block();
                self.b.ins().jump(tail, &[]);
                // Unlike `EnumMatch`, a missing `else_body` here is a genuine live
                // fall-through (no exhaustiveness proof backs this shape) — see the
                // `TStmt::If` absent-else handling this mirrors.
                let mut any_reaches_merge = false;
                for (cond, body) in arms {
                    self.b.switch_to_block(tail);
                    self.b.seal_block(tail);
                    let cond_val = self.lower_expr(cond)?;
                    let then_block = self.b.create_block();
                    let next = self.b.create_block();
                    self.b.ins().brif(cond_val, then_block, &[], next, &[]);
                    self.b.switch_to_block(then_block);
                    self.b.seal_block(then_block);
                    self.lower_stmts_scoped(body)?;
                    if !self.dead {
                        self.b.ins().jump(merge, &[]);
                        any_reaches_merge = true;
                    }
                    tail = next;
                }
                self.b.switch_to_block(tail);
                self.b.seal_block(tail);
                self.dead = false;
                if let Some(body) = else_body {
                    self.lower_stmts_scoped(body)?;
                }
                if !self.dead {
                    self.b.ins().jump(merge, &[]);
                    any_reaches_merge = true;
                }
                if any_reaches_merge {
                    self.b.switch_to_block(merge);
                    self.b.seal_block(merge);
                    self.dead = false;
                } else {
                    self.dead = true;
                }
            }
            TStmt::Region(body) => {
                self.lower_stmts_scoped(body)?;
            }
            TStmt::DebugOnly(body) => {
                self.lower_stmts_scoped(body)?;
            }
            TStmt::StructDestructure { .. } => {
                return Err("jit struct destructure unsupported".to_string());
            }
            TStmt::ListDestructure { .. } => {
                return Err("jit list destructure unsupported".to_string());
            }
            TStmt::IndexHookAssign { .. } => {
                return Err("jit index-hook assign unsupported".to_string());
            }
            TStmt::MathSwizzleAssign { .. } => {
                return Err("jit math swizzle assign unsupported".to_string());
            }
            TStmt::RangeSwitch { .. } => return Err("jit range switch unsupported".to_string()),
            TStmt::Inline(stmts) if stmts.is_empty() => {}
            TStmt::Inline(_) => return Err("jit inline comptime branch unsupported".to_string()),
            TStmt::Unsafe(_) => return Err("jit unsafe region unsupported".to_string()),
            TStmt::Reactive { .. } => return Err("jit reactive statement unsupported".to_string()),
            TStmt::Layout { .. } => return Err("jit layout block unsupported".to_string()),
            TStmt::ContextBlock { .. } => {
                return Err("jit context block unsupported".to_string());
            }
            TStmt::Live { .. } => return Err("jit live block unsupported".to_string()),
            TStmt::Shield { body } => {
                let enter_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.shield_enter, self.b.func);
                self.b.ins().call(enter_ref, &[]);
                self.shield_depth += 1;
                self.lower_stmts_scoped(body)?;
                if !self.dead {
                    let status = self
                        .emit_shield_leaves_to(self.shield_depth - 1)
                        .expect("one shield leave");
                    self.emit_pending_interrupt_check(status);
                }
                self.shield_depth -= 1;
            }
            TStmt::ScopeMember { .. } => return Err("jit scope member unsupported".to_string()),
            TStmt::Transact { .. } => return Err("jit transact block unsupported".to_string()),
            TStmt::LineMarker(_) => return Err("jit line marker unsupported".to_string()),
        }
        Ok(())
    }

    /// Compound-assign lowering for `TStmt::Assign { op: Some(op), .. }`. Keyed
    /// on `(BinOp, Type)` rather than a `TIR` enum, so the wildcard fallback
    /// here is a genuine combinatorial gap (unsupported operator/type pairs,
    /// e.g. no compound bitwise-on-float), not a hidden `TIR` variant — it is
    /// intentionally out of the exhaustive-TIR-match contract this file holds
    /// elsewhere.
    fn apply_binop_to_var(
        &mut self,
        current: Value,
        op: BinOp,
        rhs: Value,
        rhs_ty: &Type,
    ) -> Result<Value, String> {
        Ok(match (op, rhs_ty) {
            (BinOp::Add, Type::Int) => self.b.ins().iadd(current, rhs),
            (BinOp::Sub, Type::Int) => self.b.ins().isub(current, rhs),
            (BinOp::Mul, Type::Int) => self.b.ins().imul(current, rhs),
            (BinOp::Div, Type::Int) => self.b.ins().sdiv(current, rhs),
            (BinOp::Rem, Type::Int) => self.b.ins().srem(current, rhs),
            (BinOp::BitAnd, Type::Int) => self.b.ins().band(current, rhs),
            (BinOp::BitOr, Type::Int) => self.b.ins().bor(current, rhs),
            (BinOp::BitXor, Type::Int) => self.b.ins().bxor(current, rhs),
            (BinOp::Shl, Type::Int) => self.b.ins().ishl(current, rhs),
            (BinOp::Shr, Type::Int) => self.b.ins().sshr(current, rhs),
            (BinOp::Add, Type::Float) => self.b.ins().fadd(current, rhs),
            (BinOp::Sub, Type::Float) => self.b.ins().fsub(current, rhs),
            (BinOp::Mul, Type::Float) => self.b.ins().fmul(current, rhs),
            (BinOp::Div, Type::Float) => self.b.ins().fdiv(current, rhs),
            _ => return Err("jit compound assign unsupported".to_string()),
        })
    }

    /// `TCallArg` wrapper flags (`mut_borrow`/`clone`/`arc_clone`/`fn_coerce`/
    /// `widen_to_vec`) are all AOT-only borrow/coercion machinery the JIT's
    /// value model (bare Cranelift `Value`, no borrow tracking) cannot express;
    /// bail rather than silently drop the wrapper's semantics.
    fn lower_call_arg(&mut self, arg: &TCallArg) -> Result<Value, String> {
        if arg.mut_borrow
            || arg.clone
            || arg.arc_clone
            || arg.fn_coerce.is_some()
            || arg.widen_to_vec
        {
            return Err("jit call arg wrapper unsupported".to_string());
        }
        if arg.borrow && !jit_value_type(&arg.value.ty) {
            return Err("jit call arg borrow unsupported".to_string());
        }
        self.lower_expr(&arg.value)
    }

    /// `TStrPart` (`TIR/mod.rs`) is exhaustive here inline (`Lit`/`Interp`), not
    /// factored into its own top-level fn: an all-literal string is folded to
    /// one heap allocation (`flatten_string`); an interpolated one streams each
    /// part through a `str_push_*` host call keyed on the interpolated expr's
    /// `Type`, matching the AOT emitter's `format!`-based concatenation
    /// byte-for-byte (R12 parity — same runtime string, not just same shape).
    fn lower_string_lit(&mut self, parts: &[TStrPart]) -> Result<Value, String> {
        if let Some(text) = flatten_string(parts) {
            let id = self.runtime.heap.alloc_string(text);
            return Ok(self.b.ins().iconst(types::I64, id));
        }
        let begin_ref = self
            .module
            .declare_func_in_func(self.host.str_begin, self.b.func);
        let begin_call = self.b.ins().call(begin_ref, &[]);
        let buf_id = self.b.inst_results(begin_call)[0];
        for part in parts {
            match part {
                TStrPart::Lit(s) => {
                    let lit_id = self.runtime.heap.alloc_string(s.clone());
                    let lit_const = self.b.ins().iconst(types::I64, lit_id);
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.str_push_lit, self.b.func);
                    self.b.ins().call(host_ref, &[buf_id, lit_const]);
                }
                TStrPart::Interp(e, _) => {
                    let val = self.lower_expr(e)?;
                    let host_id = match &e.ty {
                        Type::Int => self.host.str_push_i64,
                        Type::Float => self.host.str_push_f64,
                        Type::Bool => self.host.str_push_bool,
                        Type::Char => self.host.str_push_char,
                        Type::String => self.host.str_push_str,
                        other => {
                            return Err(format!("jit string interp type unsupported: {other:?}"));
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    match &e.ty {
                        Type::Float => self.b.ins().call(host_ref, &[buf_id, val]),
                        _ => self.b.ins().call(host_ref, &[buf_id, val]),
                    };
                }
            }
        }
        Ok(buf_id)
    }

    /// Place strings arrive from two lowering paths that disagree on prefix
    /// convention: a plain `Let`/`Assign` name (raw Jet name, `local_place`
    /// mangles it here) vs. a `TIR`-lowering-time name already carrying the
    /// `user_` Rust-mangle prefix (destructure binds — see the
    /// `TStmt::TupleDestructure` comment in `lower_stmt`). Both must resolve
    /// to the same `self.vars` key or a valid local silently misses its slot.
    fn normalize_place(&self, place: &str) -> Result<String, String> {
        let place = place.trim();
        if let Some(inner) = place.strip_prefix("(*").and_then(|s| s.strip_suffix(')')) {
            return self.normalize_place(inner);
        }
        if self.vars.contains_key(place) {
            return Ok(place.to_string());
        }
        if let Some(name) = place.strip_prefix("user_") {
            return Ok(TIR::local_place(name));
        }
        Ok(place.to_string())
    }

    fn load_place(&mut self, place: &str) -> Result<Value, String> {
        let key = self.normalize_place(place)?;
        let var = self
            .vars
            .get(&key)
            .copied()
            .ok_or_else(|| format!("jit unknown place `{place}`"))?;
        Ok(self.b.use_var(var))
    }

    fn scrutinee_value(&mut self, s: &str) -> Result<Value, String> {
        let trimmed = s.trim();
        if let Some(stripped) = trimmed.strip_suffix(".clone()") {
            let inner = stripped
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(')');
            return self.load_place(inner);
        }
        self.load_place(trimmed)
    }

    /// `TExprKind::MethodCall`'s `func_ids` lookup key: JIT compiles user
    /// methods into plain functions named `Type::method`, so both this and
    /// `static_method_key` must reproduce the AOT emitter's exact naming
    /// convention or `func_ids.get(&key)` misses (an internal lookup failure,
    /// not a user-facing error — surfaces here as "missing method").
    fn method_key(recv_ty: &Type, method_rust: &str) -> Option<String> {
        let type_name = user_type_name(recv_ty)?;
        let method = method_rust.strip_prefix("user_").unwrap_or(method_rust);
        Some(format!("{type_name}::{method}"))
    }

    fn static_method_key(type_prefix: &str, method_rust: &str) -> Option<String> {
        let type_name = type_prefix.strip_prefix("user_")?;
        let method = method_rust.strip_prefix("user_").unwrap_or(method_rust);
        Some(format!("{type_name}::{method}"))
    }

    /// Shared backing for `TExprKind::StructLit`/`TupleLit`: both lower to the
    /// same boxed-handle host-struct representation (`struct_new`/
    /// `struct_set_*`), keyed by positional index — a struct's declared field
    /// order for `StructLit`, tuple position for `TupleLit`. Field values are
    /// lowered in source order, not name order, so evaluation order matches
    /// the AOT emitter (side effects in field-init exprs must run in the same
    /// sequence under both tiers, R12).
    fn lower_record_fields<'f>(
        &mut self,
        fields: impl Iterator<Item = &'f TExpr>,
    ) -> Result<Value, String> {
        let values: Vec<&TExpr> = fields.collect();
        let n = self.b.ins().iconst(types::I64, values.len() as i64);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[n]);
        let handle = self.b.inst_results(new_call)[0];
        for (i, value) in values.iter().enumerate() {
            let raw = self.lower_expr(value)?;
            let host_id = match &value.ty {
                Type::Int => self.host.struct_set_i64,
                Type::Float => self.host.struct_set_f64,
                Type::Bool => self.host.struct_set_bool,
                Type::Char => self.host.struct_set_char,
                Type::String => self.host.struct_set_str,
                other if clif_ty(other) == Some(types::I64) => self.host.struct_set_i64,
                _ => return Err(format!("jit record field unsupported: {:?}", value.ty)),
            };
            let idx = self.b.ins().iconst(types::I64, i as i64);
            let set_ref = self.module.declare_func_in_func(host_id, self.b.func);
            self.b.ins().call(set_ref, &[handle, idx, raw]);
        }
        Ok(handle)
    }

    fn lower_struct_lit(&mut self, fields: &[(String, TExpr, bool)]) -> Result<Value, String> {
        self.lower_record_fields(fields.iter().map(|(_, value, _)| value))
    }

    fn lower_tuple_lit(&mut self, fields: &[(String, TExpr)]) -> Result<Value, String> {
        self.lower_record_fields(fields.iter().map(|(_, value)| value))
    }

    fn lower_record_field(
        &mut self,
        handle: Value,
        type_name: &str,
        field_rust: &str,
        fallback_ty: &Type,
    ) -> Result<Value, String> {
        let idx = self
            .meta
            .struct_field_index(type_name, field_rust)
            .ok_or_else(|| format!("jit field `{field_rust}` on `{type_name}`"))?
            as i64;
        let idx_val = self.b.ins().iconst(types::I64, idx);
        let field_ty = self
            .meta
            .struct_field_type(type_name, field_rust)
            .unwrap_or_else(|| fallback_ty.clone());
        let host_id = match &field_ty {
            Type::Int => self.host.struct_get_i64,
            Type::Float => self.host.struct_get_f64,
            Type::Bool => self.host.struct_get_bool,
            Type::Char => self.host.struct_get_char,
            Type::String => self.host.struct_get_str,
            other if clif_ty(other) == Some(types::I64) => self.host.struct_get_i64,
            other => return Err(format!("jit field type unsupported: {other:?}")),
        };
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        let call = self.b.ins().call(host_ref, &[handle, idx_val]);
        Ok(self.b.inst_results(call)[0])
    }

    fn lower_tuple_destructure(
        &mut self,
        init: &TExpr,
        binds: &[(String, String)],
    ) -> Result<(), String> {
        let handle = self.lower_expr(init)?;
        let type_name = record_type_key(&init.ty).ok_or("jit tuple destructure type")?;
        for (local, field_rust) in binds {
            let fallback_ty = match &init.ty {
                Type::Tuple(fields) => fields
                    .iter()
                    .find(|(name, _)| TIR::local_place(name) == *field_rust)
                    .map(|(_, ty)| ty.as_ref().clone())
                    .unwrap_or(Type::Int),
                _ => Type::Int,
            };
            let value = self.lower_record_field(handle, &type_name, field_rust, &fallback_ty)?;
            let clif = clif_ty(&fallback_ty)
                .ok_or_else(|| format!("jit tuple destructure unsupported: {fallback_ty:?}"))?;
            let var = self.fresh_var(clif);
            self.b.def_var(var, value);
            self.vars.insert(local.clone(), var);
            self.var_tys.insert(local.clone(), fallback_ty);
        }
        Ok(())
    }

    fn lower_list_lit(&mut self, list_ty: &Type, elems: &[TExpr]) -> Result<Value, String> {
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[]);
        let handle = self.b.inst_results(new_call)[0];
        for e in elems {
            let v = self.lower_expr(e)?;
            let push_id = match list_ty {
                ty if jit_list_float_type(ty) => self.host.coll.list_push_f64,
                _ => self.host.coll.list_push,
            };
            let push_ref = self.module.declare_func_in_func(push_id, self.b.func);
            self.b.ins().call(push_ref, &[handle, v]);
        }
        Ok(handle)
    }

    fn lower_i64_value_list(&mut self, vals: &[Value]) -> Result<Value, String> {
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[]);
        let handle = self.b.inst_results(new_call)[0];
        for v in vals {
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[handle, *v]);
        }
        Ok(handle)
    }

    /// Exhaustive match on every `TExprKind` variant (`TIR/mod.rs`) — the JIT
    /// half of the R12 two-consumer contract; `TIR/emit/expressions.rs::
    /// emit_tir_expr` is the AOT half. Each variant here is either lowered for
    /// real or returns a named `Err("jit … unsupported")` (never a bare `_`),
    /// so adding a `TExprKind` variant without updating this match is a
    /// compile error in this crate, not a silent runtime gap. An `Err` is not
    /// a user-facing failure: `AotFallbackBackend`/`InterpreterBackend`
    /// (`Source/JitBackend.rs`) retry through AOT compilation and then the
    /// tier-0 interpreter, so unsupported-here only costs `jet dev` JIT speed.
    pub(crate) fn lower_expr(&mut self, expr: &TExpr) -> Result<Value, String> {
        match &expr.kind {
            TExprKind::IntLit(v, _) => Ok(self.b.ins().iconst(types::I64, *v)),
            TExprKind::FloatLit(v) => Ok(self.b.ins().f64const(*v)),
            TExprKind::BoolLit(v) => Ok(self.b.ins().iconst(types::I8, if *v { 1 } else { 0 })),
            TExprKind::CharLit(v) => Ok(self.b.ins().iconst(types::I32, *v as i64)),
            TExprKind::StrLit(parts) => self.lower_string_lit(parts),
            TExprKind::Local(place) => {
                let key = self.normalize_place(place)?;
                let var = self
                    .vars
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit unknown local `{place}`"))?;
                Ok(self.b.use_var(var))
            }
            TExprKind::Unary { op, operand } => {
                let inner = self.lower_expr(operand)?;
                Ok(match op {
                    UnOp::Neg => match &operand.ty {
                        Type::Int => self.b.ins().ineg(inner),
                        Type::Float => self.b.ins().fneg(inner),
                        other => {
                            return Err(format!("jit unary neg unsupported type: {other:?}"));
                        }
                    },
                    UnOp::Not => {
                        let zero = self.b.ins().iconst(types::I8, 0);
                        let one = self.b.ins().iconst(types::I8, 1);
                        let cmp = self.b.ins().icmp(IntCC::Equal, inner, zero);
                        self.b.ins().select(cmp, one, zero)
                    }
                })
            }
            TExprKind::IncDec {
                op,
                place,
                postfix,
                ty,
            } => self.lower_incdec(*op, place, *postfix, ty),
            TExprKind::Binary {
                op,
                overflow,
                line,
                lhs,
                rhs,
            } => {
                if matches!(op, BinOp::And | BinOp::Or) {
                    return self.lower_short_circuit(*op, lhs, rhs);
                }
                self.lower_binary(*op, *overflow, *line, lhs, rhs)
            }
            TExprKind::CompareChain { operands, ops } => self.lower_compare_chain(operands, ops),
            TExprKind::Call { name, args } => {
                let func_id = self
                    .func_ids
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("jit call to unknown function `{name}`"))?;
                let arg_vals: Result<Vec<_>, _> =
                    args.iter().map(|a| self.lower_call_arg(a)).collect();
                let arg_vals = arg_vals?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &arg_vals);
                let result = clif_ty(&expr.ty).map(|_| self.b.inst_results(call)[0]);
                self.emit_trap_check()?;
                Ok(result.unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
            }
            TExprKind::Print(inner) => {
                self.emit_print(inner)?;
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TExprKind::IfExpr {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
            } => {
                let cond_val = self.lower_expr(cond)?;
                let ret_ty = clif_ty(&expr.ty).ok_or("jit if-expr result type unsupported")?;
                let then_block = self.b.create_block();
                let else_block = self.b.create_block();
                let merge_block = self.b.create_block();
                self.b.append_block_param(merge_block, ret_ty);
                self.b
                    .ins()
                    .brif(cond_val, then_block, &[], else_block, &[]);

                self.b.switch_to_block(then_block);
                self.b.seal_block(then_block);
                self.lower_stmts(then_body)?;
                let then_val = self.lower_expr(then_value)?;
                self.b.ins().jump(merge_block, &[then_val]);

                self.b.switch_to_block(else_block);
                self.b.seal_block(else_block);
                self.lower_stmts(else_body)?;
                let else_val = self.lower_expr(else_value)?;
                self.b.ins().jump(merge_block, &[else_val]);

                self.b.switch_to_block(merge_block);
                self.b.seal_block(merge_block);
                let phi = self.b.block_params(merge_block)[0];
                Ok(phi)
            }
            TExprKind::Clone(inner) => self.lower_clone(inner),
            TExprKind::CoreCall {
                module,
                method,
                args,
            } => {
                if module == "core.tasks" && method == "channel" && args.is_empty() {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.channel_new, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.time" && method == "sleep" && args.len() == 1 {
                    let millis = self.lower_expr(&args[0])?;
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.sleep, self.b.func);
                    let call = self.b.ins().call(host_ref, &[millis]);
                    let _ = self.finish_wait_call(self.b.inst_results(call)[0]);
                    return Ok(self.b.ins().iconst(types::I8, 0));
                }
                if module == "core.perf" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "fidelity" if args.is_empty() => (self.host.perf_fidelity, Vec::new()),
                        "default_fidelity" if args.is_empty() => {
                            (self.host.perf_default_fidelity, Vec::new())
                        }
                        "override_fidelity" if args.len() == 1 => {
                            (self.host.perf_override_fidelity, vec![self.lower_expr(&args[0])?])
                        }
                        "reset_fidelity" if args.is_empty() => {
                            (self.host.perf_reset_fidelity, Vec::new())
                        }
                        _ => return Err(format!("jit core.perf call unsupported: {method}")),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(clif_ty(&expr.ty)
                        .map(|_| self.b.inst_results(call)[0])
                        .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)));
                }
                Err("jit core call unsupported".to_string())
            }
            TExprKind::CoreClosureCall { kind } => match kind {
                TCoreClosureKind::Spawn { .. } => self.lower_spawn(),
                TCoreClosureKind::Serve { .. } => {
                    Err("jit http serve closure unsupported".to_string())
                }
                TCoreClosureKind::Guard { .. } => {
                    Err("jit scope guard closure unsupported".to_string())
                }
                TCoreClosureKind::OnCommit { .. } => {
                    Err("jit transact on_commit closure unsupported".to_string())
                }
                TCoreClosureKind::OnRollback { .. } => {
                    Err("jit transact on_rollback closure unsupported".to_string())
                }
                TCoreClosureKind::ReactiveDerived { .. } => {
                    Err("jit reactive derived closure unsupported".to_string())
                }
                TCoreClosureKind::ReactiveEffect { .. } => {
                    Err("jit reactive effect closure unsupported".to_string())
                }
                TCoreClosureKind::UiReactiveRender { .. } => {
                    Err("jit UI reactive render closure unsupported".to_string())
                }
            },
            TExprKind::HandleMethod { recv, op, args } => {
                self.lower_handle_method(recv, op, args, &expr.ty)
            }
            TExprKind::TaskGroupAll { tasks } => {
                let list = self.lower_expr(tasks)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_all, self.b.func);
                let call = self.b.ins().call(host_ref, &[list]);
                Ok(self.finish_wait_call(self.b.inst_results(call)[0]))
            }
            TExprKind::TaskGroupRace { tasks } => {
                let list = self.lower_expr(tasks)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_race, self.b.func);
                let call = self.b.ins().call(host_ref, &[list]);
                Ok(self.finish_wait_call(self.b.inst_results(call)[0]))
            }
            TExprKind::TaskGroupAny { tasks } => {
                let list = self.lower_expr(tasks)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_any, self.b.func);
                let call = self.b.ins().call(host_ref, &[list]);
                Ok(self.finish_wait_call(self.b.inst_results(call)[0]))
            }
            TExprKind::SelectStart => Ok(self.b.ins().iconst(types::I64, 0)),
            TExprKind::SelectRecv { builder, channel } => {
                let _ = self.lower_expr(builder)?;
                self.lower_expr(channel)
            }
            TExprKind::SelectAfter {
                builder,
                millis,
                value: _,
            } => {
                let _ = self.lower_expr(builder)?;
                self.lower_expr(millis)
            }
            TExprKind::SelectRead { builder, .. } => self.lower_expr(builder),
            TExprKind::SelectWait { builder } => {
                let (recvs, afters) = collect_select_arms_jit(builder);
                let mut recv_vals = Vec::new();
                for ch in recvs {
                    recv_vals.push(self.lower_expr(ch)?);
                }
                let mut after_vals = Vec::new();
                for (ms, value) in afters {
                    if value.is_some() {
                        return Err("jit select timer value unsupported".to_string());
                    }
                    after_vals.push(self.lower_expr(ms)?);
                }
                let recv_list = self.lower_i64_value_list(&recv_vals)?;
                let after_list = self.lower_i64_value_list(&after_vals)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.select_wait, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_list, after_list]);
                Ok(self.finish_wait_call(self.b.inst_results(call)[0]))
            }
            TExprKind::OrFallback {
                value,
                fallback,
                is_option,
            } => {
                if *is_option {
                    let status = self.lower_list_get_opt_status(value)?;
                    let ok_block = self.b.create_block();
                    let fail_block = self.b.create_block();
                    let merge = self.b.create_block();
                    self.b.append_block_param(merge, types::I64);
                    let zero = self.b.ins().iconst(types::I64, 0);
                    let gt = self.b.ins().icmp(IntCC::SignedGreaterThan, status, zero);
                    self.b.ins().brif(gt, ok_block, &[], fail_block, &[]);
                    self.b.switch_to_block(ok_block);
                    self.b.seal_block(ok_block);
                    let one = self.b.ins().iconst(types::I64, 1);
                    let val = self.b.ins().isub(status, one);
                    self.b.ins().jump(merge, &[val]);
                    self.b.switch_to_block(fail_block);
                    self.b.seal_block(fail_block);
                    let fb = match fallback {
                        TOrFallback::Value(e) => self.lower_expr(e)?,
                        TOrFallback::Return(_)
                        | TOrFallback::Panic(_)
                        | TOrFallback::Break
                        | TOrFallback::Continue => {
                            return Err("jit option fallback unsupported".to_string());
                        }
                    };
                    self.b.ins().jump(merge, &[fb]);
                    self.b.switch_to_block(merge);
                    self.b.seal_block(merge);
                    return Ok(self.b.block_params(merge)[0]);
                }
                let status = self.lower_result_receive_status(value)?;
                let ok_block = self.b.create_block();
                let fail_block = self.b.create_block();
                let merge = self.b.create_block();
                self.b.append_block_param(merge, types::I64);
                let zero = self.b.ins().iconst(types::I64, 0);
                let gt = self.b.ins().icmp(IntCC::SignedGreaterThan, status, zero);
                self.b.ins().brif(gt, ok_block, &[], fail_block, &[]);
                self.b.switch_to_block(ok_block);
                self.b.seal_block(ok_block);
                let one = self.b.ins().iconst(types::I64, 1);
                let val = self.b.ins().isub(status, one);
                self.b.ins().jump(merge, &[val]);
                self.b.switch_to_block(fail_block);
                self.b.seal_block(fail_block);
                match fallback {
                    TOrFallback::Panic(_) => {
                        let line = self.b.ins().iconst(types::I32, 1);
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.conc.panic_channel_closed, self.b.func);
                        let call = self.b.ins().call(host_ref, &[line]);
                        let panic_val = self.b.inst_results(call)[0];
                        self.emit_trap_check()?;
                        self.b.ins().jump(merge, &[panic_val]);
                    }
                    TOrFallback::Value(_)
                    | TOrFallback::Return(_)
                    | TOrFallback::Break
                    | TOrFallback::Continue => {
                        return Err("jit or-fallback unsupported".to_string());
                    }
                }
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                Ok(self.b.block_params(merge)[0])
            }
            TExprKind::ListLit(elems) => self.lower_list_lit(&expr.ty, elems),
            TExprKind::Index {
                base,
                index,
                is_map,
                line,
            } => {
                if *is_map {
                    return Err("jit map index unsupported".to_string());
                }
                let list = self.lower_expr(base)?;
                let idx = self.lower_expr(index)?;
                let line_const = self.b.ins().iconst(types::I32, *line as i64);
                let host_id = match &expr.ty {
                    Type::Float => self.host.coll.list_get_f64,
                    _other => self.host.coll.list_get,
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host_ref, &[list, idx, line_const]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
            }
            TExprKind::Slice {
                base,
                start,
                end,
                line,
            } => {
                let list = self.lower_expr(base)?;
                let s = self.lower_expr(start)?;
                let e = self.lower_expr(end)?;
                let line_const = self.b.ins().iconst(types::I32, *line as i64);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_slice, self.b.func);
                let call = self.b.ins().call(host_ref, &[list, s, e, line_const]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
            }
            TExprKind::BuiltinMethod { recv, op, args } => {
                self.lower_builtin_method(recv, op, args, &expr.ty)
            }
            TExprKind::StructLit { fields, .. } => self.lower_struct_lit(fields),
            TExprKind::TupleLit { fields, .. } => self.lower_tuple_lit(fields),
            TExprKind::Field {
                recv, field_rust, ..
            } => {
                if record_type_key(&recv.ty).is_some_and(|name| name.contains("__")) {
                    if let Some(method_rust) = field_rust.strip_suffix("()") {
                        let key = Self::method_key(&recv.ty, method_rust)
                            .ok_or_else(|| format!("jit computed field on {:?}", recv.ty))?;
                        let func_id = self
                            .func_ids
                            .get(&key)
                            .copied()
                            .ok_or_else(|| format!("jit missing computed field `{key}`"))?;
                        let receiver = self.lower_expr(recv)?;
                        let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                        let call = self.b.ins().call(func_ref, &[receiver]);
                        let result = self.b.inst_results(call)[0];
                        self.emit_trap_check()?;
                        return Ok(result);
                    }
                }
                let handle = self.lower_expr(recv)?;
                let type_name = record_type_key(&recv.ty)
                    .or_else(|| self.method_struct.clone())
                    .ok_or("jit field recv type")?;
                self.lower_record_field(handle, &type_name, field_rust, &expr.ty)
            }
            TExprKind::MethodCall {
                recv,
                method_rust,
                args,
            } => {
                let key = Self::method_key(&recv.ty, method_rust)
                    .ok_or_else(|| format!("jit method on {:?}", recv.ty))?;
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing method `{key}`"))?;
                let mut arg_vals = vec![self.lower_expr(recv)?];
                for a in args {
                    arg_vals.push(self.lower_call_arg(a)?);
                }
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &arg_vals);
                let result = clif_ty(&expr.ty).map(|_| self.b.inst_results(call)[0]);
                self.emit_trap_check()?;
                Ok(result.unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
            }
            TExprKind::StaticCall {
                type_prefix,
                method_rust,
                args,
            } => {
                let key = Self::static_method_key(type_prefix, method_rust)
                    .ok_or_else(|| format!("jit static `{type_prefix}::{method_rust}`"))?;
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing static `{key}`"))?;
                let arg_vals: Result<Vec<_>, _> =
                    args.iter().map(|a| self.lower_call_arg(a)).collect();
                let arg_vals = arg_vals?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &arg_vals);
                let result = clif_ty(&expr.ty).map(|_| self.b.inst_results(call)[0]);
                self.emit_trap_check()?;
                Ok(result.unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
            }
            TExprKind::EnumLit { prefix, payload } => match payload {
                TEnumPayload::Unit => {
                    let disc = self
                        .meta
                        .enum_variant_disc(prefix)
                        .ok_or_else(|| format!("jit enum lit `{prefix}`"))?;
                    Ok(self.b.ins().iconst(types::I64, disc))
                }
                TEnumPayload::Positional(values) if values.len() == 1 && matches!(values[0].value.ty, Type::Int) => {
                    let disc = self.meta.enum_variant_disc(prefix)
                        .ok_or_else(|| format!("jit enum lit `{prefix}`"))?;
                    let payload = self.lower_expr(&values[0].value)?;
                    let shifted = self.b.ins().ishl_imm(payload, 8);
                    let disc = self.b.ins().iconst(types::I64, disc);
                    Ok(self.b.ins().bor(shifted, disc))
                }
                TEnumPayload::Positional(_) => Err("jit enum positional payload unsupported".to_string()),
                TEnumPayload::Named(_) => Err("jit enum named payload unsupported".to_string()),
            },
            TExprKind::Present(inner) => {
                let v = self.lower_expr(inner)?;
                // Encode optional Some as value+1 (0 = None elsewhere).
                let one = self.b.ins().iconst(types::I64, 1);
                Ok(self.b.ins().iadd(v, one))
            }
            TExprKind::Absent => Ok(self.b.ins().iconst(types::I64, 0)),
            TExprKind::ConstInline(code) => self.meta.int_constant(code)
                .or_else(|| self.meta.has_generic_instances().then(|| code.strip_suffix("i64").and_then(|value| value.parse().ok())).flatten())
                .map(|value| self.b.ins().iconst(types::I64, value))
                .ok_or_else(|| "jit const inline unsupported".to_string()),
            TExprKind::RangeCheckedCtor { .. } => {
                Err("jit range-checked ctor unsupported".to_string())
            }
            TExprKind::MathBuiltin { .. } => Err("jit math builtin unsupported".to_string()),
            // D-BIGINT1: `BigInt(...)` ctor + `+`/`-`/`*` binop, lowered from
            // `TIR/lower/expressions.rs`. `BigInt` values are opaque i64 handles
            // into `rt.heap` (`Numeric.rs`'s host shims), the same pattern as
            // `String`/list handles. `Decimal` (`type_name != "BigInt"`) stays
            // unsupported — out of this card's slice (jit_gaps.txt keeps
            // `text/decimal`).
            TExprKind::PreciseBuiltin {
                type_name,
                func,
                args,
            } if type_name == "BigInt" => {
                let host_fn = match func.as_str() {
                    "from_int" => self.host.num.bigint_from_int,
                    "from_str" => self.host.num.bigint_from_str,
                    "add" => self.host.num.bigint_add,
                    "sub" => self.host.num.bigint_sub,
                    "mul" => self.host.num.bigint_mul,
                    _ => return Err(format!("jit precise numeric builtin unsupported: {func}")),
                };
                let arg_vals: Result<Vec<_>, _> = args.iter().map(|a| self.lower_expr(a)).collect();
                let arg_vals = arg_vals?;
                let host_ref = self.module.declare_func_in_func(host_fn, self.b.func);
                let call = self.b.ins().call(host_ref, &arg_vals);
                let result = self.b.inst_results(call)[0];
                if func == "from_str" {
                    self.emit_trap_check()?;
                }
                Ok(result)
            }
            TExprKind::PreciseBuiltin { .. } => {
                Err("jit precise numeric builtin unsupported".to_string())
            }
            TExprKind::Drop(_) => Err("jit drop expression unsupported".to_string()),
            TExprKind::AmbientInput { .. } => Err("jit ambient input unsupported".to_string()),
            TExprKind::RequireStop(_) => Err("jit require/panic stop unsupported".to_string()),
            TExprKind::LayoutCompare { .. } => Err("jit layout compare unsupported".to_string()),
            TExprKind::LayoutLit { .. } => Err("jit layout literal unsupported".to_string()),
            TExprKind::PtrFromAddr { .. } => Err("jit pointer from addr unsupported".to_string()),
            TExprKind::Deref(_) => Err("jit pointer deref unsupported".to_string()),
            TExprKind::RawOf(_) => Err("jit raw pointer address-of unsupported".to_string()),
            TExprKind::AllocNew { .. } => Err("jit allocator constructor unsupported".to_string()),
            TExprKind::JsonLit { .. } => Err("jit JSON literal unsupported".to_string()),
            TExprKind::DbValueLit { .. } => Err("jit DbValue literal unsupported".to_string()),
            TExprKind::ListSpread { .. } => Err("jit list spread unsupported".to_string()),
            TExprKind::ColumnarListLit { .. } => {
                Err("jit columnar list literal unsupported".to_string())
            }
            TExprKind::ColumnarGather { .. } => Err("jit columnar gather unsupported".to_string()),
            TExprKind::ColumnarColumnRead { .. } => {
                Err("jit columnar column read unsupported".to_string())
            }
            TExprKind::MapLit(_) => Err("jit map literal unsupported".to_string()),
            TExprKind::IndexHook { .. } => Err("jit index hook unsupported".to_string()),
            TExprKind::MathLaneIndex { .. } => Err("jit math lane index unsupported".to_string()),
            TExprKind::MathSwizzleRead { .. } => {
                Err("jit math swizzle read unsupported".to_string())
            }
            TExprKind::MaterializeView(_) => {
                Err("jit string-view materialize unsupported".to_string())
            }
            TExprKind::FnFieldCall { .. } => Err("jit fn-field call unsupported".to_string()),
            TExprKind::Todo { .. } => Err("jit todo expression unsupported".to_string()),
            TExprKind::DistinctRaw(_) => Err("jit distinct raw unsupported".to_string()),
            TExprKind::Ok(inner) => self.result_new(true, inner),
            TExprKind::Err(inner) => self.result_new(false, inner),
            TExprKind::Try { inner, convert, .. } => self.lower_try(inner, convert),
            TExprKind::OptField { .. } => Err("jit optional field chain unsupported".to_string()),
            TExprKind::Lambda(_) => Err("jit lambda unsupported".to_string()),
            TExprKind::PatternMatches { .. } => {
                Err("jit pattern-matches expression unsupported".to_string())
            }
            TExprKind::FanOut { .. } => Err("jit fan-out expression unsupported".to_string()),
            TExprKind::OptionLift2 { .. } => Err("jit Option.lift2 unsupported".to_string()),
            TExprKind::ClosureMethod { .. } => Err("jit closure method unsupported".to_string()),
            TExprKind::NumericMethod { .. } => Err("jit numeric method unsupported".to_string()),
            TExprKind::OverflowOpt { .. } => {
                Err("jit overflow opt-out expression unsupported".to_string())
            }
            TExprKind::FnValue { .. } => Err("jit fn value unsupported".to_string()),
            TExprKind::ModuleCall { form, args } => match form {
                TModuleCallForm::InlineMangled { mangled } => {
                    let func_id = self.func_ids.get(mangled).copied()
                        .ok_or_else(|| "jit module call unsupported".to_string())?;
                    let arg_vals: Result<Vec<_>, _> = args.iter().map(|arg| self.lower_call_arg(arg)).collect();
                    let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                    let call = self.b.ins().call(func_ref, &arg_vals?);
                    let result = clif_ty(&expr.ty).map(|_| self.b.inst_results(call)[0]);
                    self.emit_trap_check()?;
                    Ok(result.unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
                }
                TModuleCallForm::Qualified { .. } => Err("jit file-module call unsupported".to_string()),
            },
            TExprKind::ExternCall { .. } => Err("jit extern call unsupported".to_string()),
        }
    }

    fn lower_list_get_opt_status(&mut self, value: &TExpr) -> Result<Value, String> {
        if let TExprKind::BuiltinMethod {
            recv,
            op: TBuiltinOp::GetList,
            args,
        } = &value.kind
        {
            let list = self.lower_expr(recv)?;
            let idx = self.lower_expr(&args[0])?;
            let host_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_get_opt, self.b.func);
            let call = self.b.ins().call(host_ref, &[list, idx]);
            return Ok(self.b.inst_results(call)[0]);
        }
        Err("jit list get_opt status unsupported".to_string())
    }

    /// Exhaustive match on `TBuiltinOp` (`TIR/mod.rs`) for `TExprKind::
    /// BuiltinMethod`. The JIT only covers the small hot-path subset (string
    /// len/trim/case, list push/sort/len/get/join) worth a native host-call
    /// fast path; every other `TBuiltinOp` variant is named individually (not
    /// `_`) below so a new one added to `TIR/mod.rs` fails to compile here —
    /// the AOT emitter (`TIR/emit/expressions.rs`) already handles the full
    /// set, and JIT-unsupported falls through to it per R12.
    fn lower_builtin_method(
        &mut self,
        recv: &TExpr,
        op: &TBuiltinOp,
        args: &[TExpr],
        _ret_ty: &Type,
    ) -> Result<Value, String> {
        let recv_val = self.lower_expr(recv)?;
        match op {
            TBuiltinOp::LenString => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_len, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Trim => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_trim, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::ToUpper => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_to_upper, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::ToLower => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_to_lower, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Replace => {
                let from = self.lower_expr(&args[0])?;
                let to = self.lower_expr(&args[1])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_replace, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, from, to]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Push => {
                let v = self.lower_expr(&args[0])?;
                let host_id = match &args[0].ty {
                    Type::Float => self.host.coll.list_push_f64,
                    _ => self.host.coll.list_push,
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::Sort => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_sort, self.b.func);
                self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::LenList => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_len, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::GetList => {
                let idx = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_get_opt, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, idx]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::JoinSep => {
                let sep = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_join_str, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, sep]);
                Ok(self.b.inst_results(call)[0])
            }
            // Remaining TBuiltinOp variants have no JIT lowering: each is named
            // explicitly (no catch-all) so a future TIR/mod.rs variant fails this
            // match at compile time in every JIT-lowering site (R12 exhaustive-or-
            // named-unsupported). The AOT emitter (TIR/emit/expressions.rs) covers
            // the full set; the tier-0 interpreter covers it too since it re-runs the
            // AST directly. JIT falls through to that fallback ladder for all of these.
            TBuiltinOp::IsEmpty => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Pop => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::InsertMap => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::AddNewMap => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::InsertList => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::RemoveMap => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::RemoveList { .. } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::GetMap => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::First => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Last => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Contains => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::IndexOf => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Reverse => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Sum => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Product => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Min => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Max => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Flatten => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Intersperse => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Unzip { .. } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Clear => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Chars => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Bytes => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Split => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Lines => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ToIntString => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::StartsWith => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::EndsWith => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Repeat => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Slice { .. } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::After => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Before => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::TrimView => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::AfterView => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BeforeView => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Keys => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Values => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ContainsKey => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ToString => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::MatchGroup => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Take => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Skip => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::StepBy => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Dedup => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Chunks => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Windows => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Enumerate { .. } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Zip { .. } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::OptionZip { .. } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SetFrom => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SetInsert => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SetRemove => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SetToList => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SetUnion => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SortedSetFrom => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SortedSetInsert => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SortedSetRemove => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SortedSetToList => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::SortedSetUnion => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::PriorityQueueFrom => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::PriorityQueuePeek => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::PriorityQueueToSortedList => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::LruPut => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::LruAddNew => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::LruGet => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::LruCapacity => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::LruKeys => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BitSetAdd => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BitSetRemove => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BitSetCount => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BitSetToList => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BitSetNew => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ByteBufferNew => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ByteBufferFrom => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ByteBufferWrite { .. } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ByteBufferToBytes => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BagAdd => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BagRemove => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BagHas => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BagCount => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::BagLen => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::DequePushFront => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::DequePushBack => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::DequePopFront => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::DequePopBack => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::DequePeekFront => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::DequePeekBack => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::TryCollect => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ViewNew { .. } => Err("jit builtin method unsupported".to_string()),
        }
    }

    fn lower_clone(&mut self, inner: &TExpr) -> Result<Value, String> {
        if jit_list_int_type(&inner.ty) || jit_list_float_type(&inner.ty) {
            let val = self.lower_expr(inner)?;
            let host_ref = self.module.declare_func_in_func(self.host.coll.list_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        if matches!(&inner.ty, Type::Apply { name, .. } if name == "Sender") {
            let val = self.lower_expr(inner)?;
            let host_ref = self
                .module
                .declare_func_in_func(self.host.conc.sender_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        Err("jit clone unsupported".to_string())
    }

    fn lower_spawn(&mut self) -> Result<Value, String> {
        let site = *self.spawn_site;
        *self.spawn_site += 1;
        let lam = self
            .spawn_lambdas
            .get(site)
            .ok_or_else(|| format!("jit spawn site {site} missing lambda"))?;
        let spawn_fn = self
            .spawn_func_ids
            .get(site)
            .copied()
            .ok_or_else(|| format!("jit spawn site {site} missing"))?;
        let mut cap_vals = Vec::new();
        for cap in &lam.captures {
            let mut val = self.lower_expr(&TExpr {
                ty: cap.ty.clone(),
                kind: TExprKind::Local(TIR::local_place(&cap.name)),
            })?;
            if cap.clone_at_spawn {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.sender_clone, self.b.func);
                let call = self.b.ins().call(host_ref, &[val]);
                val = self.b.inst_results(call)[0];
            }
            cap_vals.push(val);
        }
        let spawn_ref = self.module.declare_func_in_func(spawn_fn, self.b.func);
        let spawn_ptr = self.b.ins().func_addr(types::I64, spawn_ref);
        let (host_id, call_args) = match cap_vals.len() {
            0 => (self.host.conc.spawn0, vec![spawn_ptr]),
            1 => (self.host.conc.spawn1, vec![spawn_ptr, cap_vals[0]]),
            2 => (
                self.host.conc.spawn2,
                vec![spawn_ptr, cap_vals[0], cap_vals[1]],
            ),
            3 => (
                self.host.conc.spawn3,
                vec![spawn_ptr, cap_vals[0], cap_vals[1], cap_vals[2]],
            ),
            4 => (
                self.host.conc.spawn4,
                vec![
                    spawn_ptr,
                    cap_vals[0],
                    cap_vals[1],
                    cap_vals[2],
                    cap_vals[3],
                ],
            ),
            n => return Err(format!("jit spawn capture count {n} > 4")),
        };
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        let call = self.b.ins().call(host_ref, &call_args);
        Ok(self.b.inst_results(call)[0])
    }

    /// Exhaustive match on `THandleOp` (`TIR/mod.rs`) for `TExprKind::
    /// HandleMethod`. `THandleOp` covers every runtime-handle method in the
    /// stdlib (I/O, RNG, DB, HTTP, math types, …); the JIT lowers only the
    /// concurrency primitives worth a native fast path (task join/cancel,
    /// channel receive/send). Every other variant is named individually below
    /// (not `_`) so a new stdlib handle method fails to compile here instead
    /// of silently JIT-succeeding with the wrong behavior; AOT emit + the
    /// tier-0 interpreter fallback ladder cover the rest (R12).
    fn lower_handle_method(
        &mut self,
        recv: &TExpr,
        op: &THandleOp,
        args: &[TExpr],
        ret_ty: &Type,
    ) -> Result<Value, String> {
        let recv_val = self.lower_expr(recv)?;
        match op {
            THandleOp::TaskJoin => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_join, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                let result = self.finish_wait_call(self.b.inst_results(call)[0]);
                if clif_ty(ret_ty).is_some() {
                    Ok(result)
                } else {
                    Ok(self.b.ins().iconst(types::I8, 0))
                }
            }
            THandleOp::TaskCancel => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_cancel, self.b.func);
                self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::ChannelReceive => {
                let line = self.b.ins().iconst(types::I32, 1);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.channel_receive, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, line]);
                let result = self.finish_wait_call(self.b.inst_results(call)[0]);
                self.emit_trap_check()?;
                Ok(result)
            }
            THandleOp::SenderSend => {
                let val = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.sender_send, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, val]);
                let _ = self.finish_wait_call(self.b.inst_results(call)[0]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            // Remaining THandleOp variants have no JIT lowering: named explicitly
            // (no catch-all) so a future TIR/mod.rs variant fails this match at
            // compile time (R12). AOT emit + tier-0 interpreter fallback cover these.
            THandleOp::FileReaderReadLine => Err("jit handle method unsupported".to_string()),
            THandleOp::FileWriterWriteLine => Err("jit handle method unsupported".to_string()),
            THandleOp::FileWriterFlush => Err("jit handle method unsupported".to_string()),
            THandleOp::JSONReaderNext => {
                Err("jit JSON streaming falls back to the AOT executable TIR path".to_string())
            }
            THandleOp::JSONWriterWrite
            | THandleOp::JSONWriterFlush
            | THandleOp::JSONWriterFinish => {
                Err("jit JSON streaming falls back to the AOT executable TIR path".to_string())
            }
            THandleOp::JSONLReaderNext
            | THandleOp::JSONLWriterWrite
            | THandleOp::JSONLWriterFlush
            | THandleOp::JSONLWriterFinish => {
                Err("jit JSONL streaming falls back to the AOT executable TIR path".to_string())
            }
            THandleOp::CSVReaderNext
            | THandleOp::CSVWriterWrite
            | THandleOp::CSVWriterFlush
            | THandleOp::CSVWriterFinish => {
                Err("jit CSV streaming falls back to the AOT executable TIR path".to_string())
            }
            THandleOp::XMLReaderNext
            | THandleOp::XMLWriterWrite
            | THandleOp::XMLWriterFlush
            | THandleOp::XMLWriterFinish => {
                Err("jit XML streaming falls back to the AOT executable TIR path".to_string())
            }
            THandleOp::CBORReaderNext | THandleOp::CBORWriterWrite | THandleOp::CBORWriterFlush | THandleOp::CBORWriterFinish => Err("jit CBOR streaming falls back to the AOT executable TIR path".to_string()),
            THandleOp::StdinReadLine => Err("jit handle method unsupported".to_string()),
            THandleOp::StdoutWrite => Err("jit handle method unsupported".to_string()),
            THandleOp::StdoutWriteLine => Err("jit handle method unsupported".to_string()),
            THandleOp::StdoutWriteBytes => Err("jit handle method unsupported".to_string()),
            THandleOp::StdoutFlush => Err("jit handle method unsupported".to_string()),
            THandleOp::StdoutIsTty => Err("jit handle method unsupported".to_string()),
            THandleOp::StderrWrite => Err("jit handle method unsupported".to_string()),
            THandleOp::StderrWriteLine => Err("jit handle method unsupported".to_string()),
            THandleOp::StderrWriteBytes => Err("jit handle method unsupported".to_string()),
            THandleOp::StderrFlush => Err("jit handle method unsupported".to_string()),
            THandleOp::StderrIsTty => Err("jit handle method unsupported".to_string()),
            THandleOp::StopwatchElapsedMillis => Err("jit handle method unsupported".to_string()),
            THandleOp::ClockNow => Err("jit handle method unsupported".to_string()),
            THandleOp::ClockTick => Err("jit handle method unsupported".to_string()),
            THandleOp::ClockAdvance => Err("jit handle method unsupported".to_string()),
            THandleOp::ClockWait => Err("jit handle method unsupported".to_string()),
            THandleOp::RngInt => Err("jit handle method unsupported".to_string()),
            THandleOp::RngFloat => Err("jit handle method unsupported".to_string()),
            THandleOp::RngFloatRange => Err("jit handle method unsupported".to_string()),
            THandleOp::RngBool => Err("jit handle method unsupported".to_string()),
            THandleOp::RngBoolP => Err("jit handle method unsupported".to_string()),
            THandleOp::RngNormal => Err("jit handle method unsupported".to_string()),
            THandleOp::RngExponential => Err("jit handle method unsupported".to_string()),
            THandleOp::RngBytes => Err("jit handle method unsupported".to_string()),
            THandleOp::RngSplit => Err("jit handle method unsupported".to_string()),
            THandleOp::RngPick => Err("jit handle method unsupported".to_string()),
            THandleOp::RngWeightedPick => Err("jit handle method unsupported".to_string()),
            THandleOp::RngSample => Err("jit handle method unsupported".to_string()),
            THandleOp::RngShuffle => Err("jit handle method unsupported".to_string()),
            THandleOp::SolverNew => {
                let host = self.module.declare_func_in_func(self.host.solver.new, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::SolverRequire => {
                let ok = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(self.host.solver.require, self.b.func);
                self.b.ins().call(host, &[recv_val, ok]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::SolverFailureCount => {
                let host = self.module.declare_func_in_func(self.host.solver.failure_count, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::SolverStatus => {
                let host = self.module.declare_func_in_func(self.host.solver.status, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::GameSceneNew => Err("jit handle method unsupported".to_string()),
            THandleOp::GameReplayRecord => Err("jit handle method unsupported".to_string()),
            THandleOp::GameBackendHeadless => Err("jit handle method unsupported".to_string()),
            THandleOp::GameSceneOnFrame => Err("jit handle method unsupported".to_string()),
            THandleOp::GameSceneComponent => Err("jit handle method unsupported".to_string()),
            THandleOp::GameSceneQuery => Err("jit handle method unsupported".to_string()),
            THandleOp::GameAssetsImage => Err("jit handle method unsupported".to_string()),
            THandleOp::GameAssetsSound => Err("jit handle method unsupported".to_string()),
            THandleOp::GameInputBind => Err("jit handle method unsupported".to_string()),
            THandleOp::GameInputPressed => Err("jit handle method unsupported".to_string()),
            THandleOp::DurationMillis => Err("jit handle method unsupported".to_string()),
            THandleOp::DurationSeconds => Err("jit handle method unsupported".to_string()),
            // D-BIGINT1: `BigInt` instance methods (`.add`/`.sub`/`.mul`/`.neg`/
            // `.to_string`) — reuses the same `rt.heap` handle host shims as the
            // `PreciseBuiltin` ctor/binop path above. `Decimal` stays unsupported
            // (out of this card's slice).
            THandleOp::PreciseMethod { type_name, method } if type_name == "BigInt" => {
                let (host_fn, extra_args) = match method.as_str() {
                    "add" => (self.host.num.bigint_add, 1),
                    "sub" => (self.host.num.bigint_sub, 1),
                    "mul" => (self.host.num.bigint_mul, 1),
                    "neg" => (self.host.num.bigint_neg, 0),
                    "to_string" => (self.host.num.bigint_to_string, 0),
                    _ => return Err(format!("jit handle method unsupported: BigInt::{method}")),
                };
                let mut arg_vals = vec![recv_val];
                for a in args.iter().take(extra_args) {
                    arg_vals.push(self.lower_expr(a)?);
                }
                let host_ref = self.module.declare_func_in_func(host_fn, self.b.func);
                let call = self.b.ins().call(host_ref, &arg_vals);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PreciseMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpListenerAccept => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpListenerLocalAddr => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamRead => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamWrite => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamPeerAddr => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamLocalAddr => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamClose => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamReadBytes
            | THandleOp::TcpStreamReadText
            | THandleOp::TcpStreamWriteBytes
            | THandleOp::TcpStreamWriteAllBytes
            | THandleOp::TcpStreamWriteText
            | THandleOp::TcpStreamShutdown
            | THandleOp::TcpStreamReady => Err("jit handle method unsupported".to_string()),
            THandleOp::AllocAlloc => Err("jit handle method unsupported".to_string()),
            THandleOp::AllocReset => Err("jit handle method unsupported".to_string()),
            THandleOp::AllocFree => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpReqField(..) => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpReqHeader => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpReqParam => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpRespField(..) => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpRespHeader => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecFlag => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecFlagShort => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOption => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionShort => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionDefault => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionEnv => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionInt => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionFloat => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionChoice => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecRepeat => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecRequiredOption => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecPositional => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecSubcommand => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecVersion => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecCompletion => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecHelp => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecParse => Err("jit handle method unsupported".to_string()),
            THandleOp::ParsedArgsFlag => Err("jit handle method unsupported".to_string()),
            THandleOp::ParsedArgsOption => Err("jit handle method unsupported".to_string()),
            THandleOp::ParsedArgsOptionInt => Err("jit handle method unsupported".to_string()),
            THandleOp::ParsedArgsOptionFloat => Err("jit handle method unsupported".to_string()),
            THandleOp::ParsedArgsOptions => Err("jit handle method unsupported".to_string()),
            THandleOp::ParsedArgsSubcommand => Err("jit handle method unsupported".to_string()),
            THandleOp::ParsedArgsPositional => Err("jit handle method unsupported".to_string()),
            THandleOp::ProcessSpecMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::ProcessChildMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::ProcessStdinWrite => Err("jit handle method unsupported".to_string()),
            THandleOp::ReflectValueTypeName => Err("jit handle method unsupported".to_string()),
            THandleOp::ReflectValueDisplay => Err("jit handle method unsupported".to_string()),
            THandleOp::ReflectValueFields => Err("jit handle method unsupported".to_string()),
            THandleOp::ReflectFieldName => Err("jit handle method unsupported".to_string()),
            THandleOp::ReflectFieldValue => Err("jit handle method unsupported".to_string()),
            THandleOp::TaskDetach => Err("jit handle method unsupported".to_string()),
            THandleOp::TaskPause => Err("jit handle method unsupported".to_string()),
            THandleOp::TaskResume => Err("jit handle method unsupported".to_string()),
            THandleOp::TaskTrace => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpRouterRegister { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::MathMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::ReactiveGet => Err("jit handle method unsupported".to_string()),
            THandleOp::ReactiveSet => Err("jit handle method unsupported".to_string()),
            THandleOp::EventMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::WatchMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::MeasurementMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::LayoutMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::LoadableMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::ExpiringMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::RottingMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::SketchMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::CivilTimeMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::UrlMimeMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::EmailMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::RegexMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpClientMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpServerMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::DataTreeField => Err("jit handle method unsupported".to_string()),
            THandleOp::DataTreeAt => Err("jit handle method unsupported".to_string()),
            THandleOp::DataTreeInt => Err("jit handle method unsupported".to_string()),
            THandleOp::DataTreeText => Err("jit handle method unsupported".to_string()),
            THandleOp::DataTreeBool => Err("jit handle method unsupported".to_string()),
            THandleOp::DataTreeFloat => Err("jit handle method unsupported".to_string()),
            THandleOp::JsonField => Err("jit handle method unsupported".to_string()),
            THandleOp::JsonAt => Err("jit handle method unsupported".to_string()),
            THandleOp::JsonInt => Err("jit handle method unsupported".to_string()),
            THandleOp::JsonText => Err("jit handle method unsupported".to_string()),
            THandleOp::JsonBool => Err("jit handle method unsupported".to_string()),
            THandleOp::JsonFloat => Err("jit handle method unsupported".to_string()),
            THandleOp::PathFrom => Err("jit handle method unsupported".to_string()),
            THandleOp::PathJoin => Err("jit handle method unsupported".to_string()),
            THandleOp::PathParent => Err("jit handle method unsupported".to_string()),
            THandleOp::PathExtension => Err("jit handle method unsupported".to_string()),
            THandleOp::PathStem => Err("jit handle method unsupported".to_string()),
            THandleOp::PathToString => Err("jit handle method unsupported".to_string()),
            THandleOp::PathWriteAtomic => Err("jit handle method unsupported".to_string()),
            THandleOp::PathWalk => Err("jit handle method unsupported".to_string()),
            THandleOp::UiBackendMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::DevServerMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::DbQuery => Err("jit handle method unsupported".to_string()),
            THandleOp::DbQueryOne => Err("jit handle method unsupported".to_string()),
            THandleOp::DbExecute => Err("jit handle method unsupported".to_string()),
            THandleOp::DbBegin => Err("jit handle method unsupported".to_string()),
            THandleOp::DbCommit => Err("jit handle method unsupported".to_string()),
            THandleOp::DbRollback => Err("jit handle method unsupported".to_string()),
            THandleOp::DbClose => Err("jit handle method unsupported".to_string()),
            THandleOp::DbValueInt => Err("jit handle method unsupported".to_string()),
            THandleOp::DbValueFloat => Err("jit handle method unsupported".to_string()),
            THandleOp::DbValueText => Err("jit handle method unsupported".to_string()),
            THandleOp::DbValueBool => Err("jit handle method unsupported".to_string()),
            THandleOp::DbValueIsNull => Err("jit handle method unsupported".to_string()),
            THandleOp::PluginCall => Err("jit handle method unsupported".to_string()),
            THandleOp::PluginCallInt => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderOver => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderReadU8 => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderReadU16Le => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderReadU16Be => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderReadU32Le => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderReadU32Be => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderReadU64Le => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderReadU64Be => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderTake => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderRemaining => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderAtEnd => Err("jit handle method unsupported".to_string()),
            THandleOp::CursorOver => Err("jit handle method unsupported".to_string()),
            THandleOp::CursorTakeUntil => Err("jit handle method unsupported".to_string()),
            THandleOp::CursorSkipWs => Err("jit handle method unsupported".to_string()),
            THandleOp::CursorTakePattern { .. }
            | THandleOp::ReaderTakePattern { .. }
            | THandleOp::DataTreeDecode(_)
            | THandleOp::SerdeEncode => {
                Err("jit handle method unsupported".to_string())
            }
        }
    }

    fn lower_result_receive_status(&mut self, value: &TExpr) -> Result<Value, String> {
        if let TExprKind::HandleMethod {
            recv,
            op: THandleOp::ChannelReceive,
            args,
        } = &value.kind
        {
            if !args.is_empty() {
                return Err("jit receive status arity".to_string());
            }
            let ch = self.lower_expr(recv)?;
            let host_ref = self
                .module
                .declare_func_in_func(self.host.conc.channel_receive_status, self.b.func);
            let call = self.b.ins().call(host_ref, &[ch]);
            return Ok(self.finish_wait_call(self.b.inst_results(call)[0]));
        }
        Err("jit result status unsupported".to_string())
    }

    fn lower_short_circuit(
        &mut self,
        op: BinOp,
        lhs: &TExpr,
        rhs: &TExpr,
    ) -> Result<Value, String> {
        let lhs_val = self.lower_expr(lhs)?;
        let rhs_block = self.b.create_block();
        let merge_block = self.b.create_block();
        self.b.append_block_param(merge_block, types::I8);

        let short_val = if matches!(op, BinOp::And) {
            self.b.ins().iconst(types::I8, 0)
        } else {
            self.b.ins().iconst(types::I8, 1)
        };

        let zero = self.b.ins().iconst(types::I8, 0);
        let take_short = if matches!(op, BinOp::And) {
            self.b.ins().icmp(IntCC::Equal, lhs_val, zero)
        } else {
            self.b.ins().icmp(IntCC::NotEqual, lhs_val, zero)
        };
        self.b
            .ins()
            .brif(take_short, merge_block, &[short_val], rhs_block, &[]);

        self.b.switch_to_block(rhs_block);
        self.b.seal_block(rhs_block);
        let rhs_val = self.lower_expr(rhs)?;
        self.b.ins().jump(merge_block, &[rhs_val]);

        self.b.switch_to_block(merge_block);
        self.b.seal_block(merge_block);
        Ok(self.b.block_params(merge_block)[0])
    }

    /// `TExprKind::CompareChain` (`a < b <= c`, …): ops/types are ancillary
    /// `BinOp`/`Type` combinations, not a `TIR` enum, so the `_` fallback here
    /// is a genuine unsupported-combination gap (e.g. no chained String
    /// comparison), not a hidden `TIR` variant.
    fn lower_compare_chain(&mut self, operands: &[TExpr], ops: &[BinOp]) -> Result<Value, String> {
        if operands.len() != ops.len() + 1 {
            return Err("jit compare chain arity mismatch".to_string());
        }
        let vals: Result<Vec<_>, _> = operands.iter().map(|e| self.lower_expr(e)).collect();
        let vals = vals?;
        let mut acc = self.b.ins().iconst(types::I8, 1);
        for (i, op) in ops.iter().enumerate() {
            let lhs_ty = self.expr_arith_type(&operands[i]);
            let rhs_ty = self.expr_arith_type(&operands[i + 1]);
            if lhs_ty != rhs_ty {
                return Err("jit compare chain mixed operand types".to_string());
            }
            let cmp = match (&lhs_ty, op) {
                (Type::Int, BinOp::Lt) => {
                    self.bool_from_icmp(IntCC::SignedLessThan, vals[i], vals[i + 1])
                }
                (Type::Int, BinOp::Gt) => {
                    self.bool_from_icmp(IntCC::SignedGreaterThan, vals[i], vals[i + 1])
                }
                (Type::Int, BinOp::Le) => {
                    self.bool_from_icmp(IntCC::SignedLessThanOrEqual, vals[i], vals[i + 1])
                }
                (Type::Int, BinOp::Ge) => {
                    self.bool_from_icmp(IntCC::SignedGreaterThanOrEqual, vals[i], vals[i + 1])
                }
                (Type::Float, BinOp::Lt) => {
                    self.bool_from_fcmp(FloatCC::LessThan, vals[i], vals[i + 1])
                }
                (Type::Float, BinOp::Gt) => {
                    self.bool_from_fcmp(FloatCC::GreaterThan, vals[i], vals[i + 1])
                }
                (Type::Float, BinOp::Le) => {
                    self.bool_from_fcmp(FloatCC::LessThanOrEqual, vals[i], vals[i + 1])
                }
                (Type::Float, BinOp::Ge) => {
                    self.bool_from_fcmp(FloatCC::GreaterThanOrEqual, vals[i], vals[i + 1])
                }
                _ => return Err("jit compare chain operator unsupported".to_string()),
            };
            acc = self.b.ins().band(acc, cmp);
        }
        Ok(acc)
    }

    fn lower_incdec(
        &mut self,
        op: IncDecOp,
        place: &str,
        postfix: bool,
        ty: &Type,
    ) -> Result<Value, String> {
        if !matches!(ty, Type::Int) {
            return Err("jit increment/decrement unsupported type".to_string());
        }
        let key = self.normalize_place(place)?;
        let var = self
            .vars
            .get(&key)
            .copied()
            .ok_or_else(|| format!("jit increment/decrement unknown local `{place}`"))?;
        let old = self.b.use_var(var);
        let delta = match op {
            IncDecOp::Inc => self.b.ins().iconst(types::I64, 1),
            IncDecOp::Dec => self.b.ins().iconst(types::I64, -1),
        };
        let next = self.b.ins().iadd(old, delta);
        self.b.def_var(var, next);
        Ok(if postfix { old } else { next })
    }

    fn expr_field_type(&self, expr: &TExpr) -> Option<Type> {
        let TExprKind::Field {
            recv, field_rust, ..
        } = &expr.kind
        else {
            return None;
        };
        let type_name = record_type_key(&recv.ty).or_else(|| self.method_struct.clone())?;
        self.meta.struct_field_type(&type_name, field_rust)
    }

    fn expr_arith_type(&self, expr: &TExpr) -> Type {
        if let Some(t) = self.expr_field_type(expr) {
            return t;
        }
        if let TExprKind::Binary { lhs, rhs, .. } = &expr.kind {
            let lt = self.expr_arith_type(lhs);
            let rt = self.expr_arith_type(rhs);
            if lt == Type::Float || rt == Type::Float {
                return Type::Float;
            }
            if lt == Type::Int && rt == Type::Int {
                return Type::Int;
            }
        }
        expr.ty.clone()
    }

    /// `TExprKind::Binary` (`op != And/Or`, those short-circuit separately in
    /// `lower_short_circuit`). Keyed on `(Type, BinOp)` — ancillary types, not
    /// a `TIR` enum — so the trailing `_` is a real unsupported-combination
    /// gap (e.g. no bitwise-on-float), not a hidden `TIR` variant.
    fn lower_binary(
        &mut self,
        op: BinOp,
        overflow: bool,
        line: u32,
        lhs: &TExpr,
        rhs: &TExpr,
    ) -> Result<Value, String> {
        let l = self.lower_expr(lhs)?;
        let r = self.lower_expr(rhs)?;
        if overflow {
            let host_id = match op {
                BinOp::Add => self.host.add_i64,
                BinOp::Sub => self.host.sub_i64,
                BinOp::Mul => self.host.mul_i64,
                BinOp::Div => self.host.div_i64,
                _ => return Err("jit overflow op unsupported".to_string()),
            };
            let line_const = self.b.ins().iconst(types::I32, line as i64);
            let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
            let call = self.b.ins().call(host_ref, &[l, r, line_const]);
            let result = self.b.inst_results(call)[0];
            self.emit_trap_check()?;
            return Ok(result);
        }
        let lhs_ty = self.expr_arith_type(lhs);
        let _rhs_ty = self.expr_arith_type(rhs);
        if matches!(op, BinOp::Eq | BinOp::Ne) && matches!(lhs_ty, Type::Tuple(_)) {
            return self.lower_tuple_eq(op, &lhs_ty, l, r);
        }
        Ok(match (&lhs_ty, op) {
            (Type::Int, BinOp::Add) => self.b.ins().iadd(l, r),
            (Type::Int, BinOp::Sub) => self.b.ins().isub(l, r),
            (Type::Int, BinOp::Mul) => self.b.ins().imul(l, r),
            (Type::Int, BinOp::Div) => self.b.ins().sdiv(l, r),
            (Type::Int, BinOp::Rem) => self.b.ins().srem(l, r),
            (Type::Int, BinOp::BitAnd) => self.b.ins().band(l, r),
            (Type::Int, BinOp::BitOr) => self.b.ins().bor(l, r),
            (Type::Int, BinOp::BitXor) => self.b.ins().bxor(l, r),
            (Type::Int, BinOp::Shl) => self.b.ins().ishl(l, r),
            (Type::Int, BinOp::Shr) => self.b.ins().sshr(l, r),
            (Type::Float, BinOp::Add) => self.b.ins().fadd(l, r),
            (Type::Float, BinOp::Sub) => self.b.ins().fsub(l, r),
            (Type::Float, BinOp::Mul) => self.b.ins().fmul(l, r),
            (Type::Float, BinOp::Div) => self.b.ins().fdiv(l, r),
            (Type::Int, BinOp::Eq) => self.bool_from_icmp(IntCC::Equal, l, r),
            (Type::Int, BinOp::Ne) => self.bool_from_icmp(IntCC::NotEqual, l, r),
            (Type::Int, BinOp::Lt) => self.bool_from_icmp(IntCC::SignedLessThan, l, r),
            (Type::Int, BinOp::Gt) => self.bool_from_icmp(IntCC::SignedGreaterThan, l, r),
            (Type::Int, BinOp::Le) => self.bool_from_icmp(IntCC::SignedLessThanOrEqual, l, r),
            (Type::Int, BinOp::Ge) => self.bool_from_icmp(IntCC::SignedGreaterThanOrEqual, l, r),
            (Type::Float, BinOp::Eq) => self.bool_from_fcmp(FloatCC::Equal, l, r),
            (Type::Float, BinOp::Ne) => self.bool_from_fcmp(FloatCC::NotEqual, l, r),
            (Type::Float, BinOp::Lt) => self.bool_from_fcmp(FloatCC::LessThan, l, r),
            (Type::Float, BinOp::Gt) => self.bool_from_fcmp(FloatCC::GreaterThan, l, r),
            (Type::Float, BinOp::Le) => self.bool_from_fcmp(FloatCC::LessThanOrEqual, l, r),
            (Type::Float, BinOp::Ge) => self.bool_from_fcmp(FloatCC::GreaterThanOrEqual, l, r),
            (Type::Bool, BinOp::Eq) => self.bool_from_icmp(IntCC::Equal, l, r),
            (Type::Bool, BinOp::Ne) => self.bool_from_icmp(IntCC::NotEqual, l, r),
            (Type::Named(_) | Type::Apply { .. }, BinOp::Eq) => self.bool_from_icmp(IntCC::Equal, l, r),
            (Type::Named(_) | Type::Apply { .. }, BinOp::Ne) => self.bool_from_icmp(IntCC::NotEqual, l, r),
            (Type::String, BinOp::Eq) => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_eq, self.b.func);
                let call = self.b.ins().call(host_ref, &[l, r]);
                self.b.inst_results(call)[0]
            }
            (Type::String, BinOp::Ne) => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_eq, self.b.func);
                let call = self.b.ins().call(host_ref, &[l, r]);
                let eq = self.b.inst_results(call)[0];
                let one = self.b.ins().iconst(types::I8, 1);
                self.b.ins().isub(one, eq)
            }
            _ => return Err("jit binary op unsupported".to_string()),
        })
    }

    fn lower_tuple_eq(
        &mut self,
        op: BinOp,
        ty: &Type,
        lhs: Value,
        rhs: Value,
    ) -> Result<Value, String> {
        let Type::Tuple(fields) = ty else {
            return Err("jit tuple equality needs tuple type".to_string());
        };
        let type_name = record_type_key(ty).ok_or("jit tuple equality type")?;
        let mut acc = self.b.ins().iconst(types::I8, 1);
        for (name, field_ty) in fields {
            let field_rust = TIR::local_place(name);
            let left = self.lower_record_field(lhs, &type_name, &field_rust, field_ty)?;
            let right = self.lower_record_field(rhs, &type_name, &field_rust, field_ty)?;
            let eq = match field_ty.as_ref() {
                Type::Int => self.bool_from_icmp(IntCC::Equal, left, right),
                Type::Float => self.bool_from_fcmp(FloatCC::Equal, left, right),
                other => return Err(format!("jit tuple equality unsupported: {other:?}")),
            };
            acc = self.b.ins().band(acc, eq);
        }
        if matches!(op, BinOp::Ne) {
            let one = self.b.ins().iconst(types::I8, 1);
            Ok(self.b.ins().isub(one, acc))
        } else {
            Ok(acc)
        }
    }

    fn bool_from_icmp(&mut self, cc: IntCC, l: Value, r: Value) -> Value {
        let cmp = self.b.ins().icmp(cc, l, r);
        let one = self.b.ins().iconst(types::I8, 1);
        let zero = self.b.ins().iconst(types::I8, 0);
        self.b.ins().select(cmp, one, zero)
    }

    fn bool_from_fcmp(&mut self, cc: FloatCC, l: Value, r: Value) -> Value {
        let cmp = self.b.ins().fcmp(cc, l, r);
        let one = self.b.ins().iconst(types::I8, 1);
        let zero = self.b.ins().iconst(types::I8, 0);
        self.b.ins().select(cmp, one, zero)
    }

    /// `TExprKind::Print` payload lowering: literal-kind exprs (`IntLit`/
    /// `FloatLit`/…) print without a materialized `Value` round-trip; the
    /// fallback arm lowers the expr once and dispatches on its result `Type`
    /// — a `Type`, not a `TIR` variant, so its own `_ => Err(..)` is a real
    /// unsupported-print-type gap, not a hidden `TExprKind` case.
    fn emit_print(&mut self, inner: &TExpr) -> Result<(), String> {
        let (host_id, arg) = match &inner.kind {
            TExprKind::IntLit(v, _) => (self.host.print_i64, self.b.ins().iconst(types::I64, *v)),
            TExprKind::FloatLit(v) => (self.host.print_f64, self.b.ins().f64const(*v)),
            TExprKind::BoolLit(v) => (
                self.host.print_bool,
                self.b.ins().iconst(types::I8, if *v { 1 } else { 0 }),
            ),
            TExprKind::CharLit(v) => (
                self.host.print_char,
                self.b.ins().iconst(types::I32, *v as i64),
            ),
            TExprKind::StrLit(parts) => {
                let id = self.lower_string_lit(parts)?;
                (self.host.print_str, id)
            }
            _ => {
                let val = self.lower_expr(inner)?;
                let host_id = match &inner.ty {
                    Type::Int => self.host.print_i64,
                    Type::String => self.host.print_str,
                    Type::Float => self.host.print_f64,
                    Type::Bool => self.host.print_bool,
                    Type::Char => self.host.print_char,
                    _ => return Err("jit print type unsupported".to_string()),
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                self.b.ins().call(host_ref, &[val]);
                return Ok(());
            }
        };
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        self.b.ins().call(host_ref, &[arg]);
        Ok(())
    }
}
