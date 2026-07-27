use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, Block, Endianness, InstBuilder, MemFlags, StackSlot, StackSlotData, StackSlotKind,
    TrapCode, Value,
};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};
use jet_codegen::Codegen::TIR::{
    self, TBuiltinOp, TCallArg, TClosureOp, TCoreClosureKind, TEnumPayload, TExpr, TExprKind,
    TForInMethod, THandleOp, THostArg, THostCall, TIfCond, TJitSpawnLambda, TLambda, TLambdaBody,
    TLocal,
    ListSpreadPart,
    TFnValueKind, TMethodRef, TModuleCallForm, TNumericOp, TOrFallback, TPattern,
    TPatternPosition, TPlace,
    TStaticOwner, TStmt, TStrPart, TTypedTextForm, TTypedTextInterpKind,
};
use jet_foundation::AST::{BinOp, IncDecOp, PatSlot, Pattern, StrFormat, Type, UnOp};
use std::collections::HashMap;

use super::runtime_host::{
    HostFns, INTN_MODE_CHECKED, INTN_MODE_SATURATING, INTN_MODE_TRAP, INTN_MODE_WRAPPING,
    INTN_OP_ADD, INTN_OP_BIT_AND, INTN_OP_BIT_OR, INTN_OP_BIT_XOR, INTN_OP_DIV, INTN_OP_MUL,
    INTN_OP_REM, INTN_OP_SHL, INTN_OP_SHR, INTN_OP_SUB,
};
use super::safety::{
    collect_select_arms_jit, flatten_string, jit_closure_elem_type, jit_list_float_type,
    jit_list_iter_elem_type, jit_list_native_type, jit_list_of_int_list_type, jit_list_record_type,
    jit_list_string_type, jit_map_string_type, jit_result_list_elem, jit_struct_type, jit_tuple_type,
    jit_value_type, record_type_key, user_type_name,
};
use super::types_meta::{
    clif_ty, core_struct_field_type, fn_value_signature, init_clif_ty, JitMeta,
};
use super::JitRuntime;

#[derive(Clone)]
pub(crate) struct LoopTargets {
    label: Option<String>,
    continue_block: Block,
    break_block: Block,
    break_value_ty: Option<Type>,
    shield_depth: u32,
    shared_transaction_depth: u32,
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
    pub(crate) raw_slots: HashMap<String, StackSlot>,
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
    /// Nested `#Context(deadline: …)` depth — pop host TLS on early returns.
    pub(crate) deadline_depth: u32,
    /// Current structured subject for `MixedSwitch` field tests and bindings.
    pub(crate) switch_subject: Option<(Value, Type)>,
    /// Sender handle for a native generator body.
    pub(crate) yield_sender: Option<Value>,
    pub(crate) in_shared_transaction: bool,
    pub(crate) shared_transaction_depth: u32,
    pub(crate) unsafe_depth: usize,
    /// `scope.guard` cleanups — compiled zero-arg funcs, run LIFO on exit.
    pub(crate) scope_guards: Vec<FuncId>,
    /// Open `#Transact` frames: snapshot restores + commit/rollback hook funcs.
    pub(crate) txn_stack: Vec<TxnFrame>,
}

#[derive(Clone)]
pub(crate) enum TxnSnap {
    /// Restore via `Type::restore(current, snap)`.
    Rollback(String),
    /// Write-scalar param (`&Int`): `snap` is the loaded value; restore stores
    /// through the still-current pointer in the local.
    ScalarMut,
    /// Plain handle/value rewrite into the local.
    Plain,
}

pub(crate) struct TxnFrame {
    /// `(place, snap_value, kind)`.
    pub snapshots: Vec<(String, Value, TxnSnap)>,
    pub on_commit: Vec<FuncId>,
    pub on_rollback: Vec<FuncId>,
}

impl LowerCtx<'_, '_> {
    fn receiver_is(ty: &Type, expected: &str) -> bool {
        matches!(ty, Type::Named(name) if name == expected)
            || matches!(ty, Type::Apply { name, .. } if name == expected)
    }

    fn uses_result_option_abi(expr: &TExpr) -> bool {
        if !matches!(&expr.ty, Type::Option(_)) {
            return false;
        }
        match &expr.kind {
            TExprKind::BuiltinMethod { op, recv, .. } => match op {
                TBuiltinOp::GetMap => matches!(&recv.ty, Type::Map { .. }),
                TBuiltinOp::First | TBuiltinOp::Last => {
                    Self::receiver_is(&recv.ty, "SortedSet")
                }
                TBuiltinOp::Min { float: false } | TBuiltinOp::Max { float: false } => {
                    matches!(&recv.ty, Type::List(_) | Type::FixedList { .. })
                }
                TBuiltinOp::Pop | TBuiltinOp::PriorityQueuePeek => {
                    Self::receiver_is(&recv.ty, "PriorityQueue")
                }
                TBuiltinOp::LruPut | TBuiltinOp::LruGet => {
                    Self::receiver_is(&recv.ty, "Lru")
                }
                _ => false,
            },
            TExprKind::HostCall(host) => matches!(
                host.as_ref(),
                THostCall::Method { recv, method, .. }
                    if method == "remove"
                        && Self::receiver_is(&recv.ty, "Pool")
            ),
            _ => false,
        }
    }

    fn raw_place_local(expr: &TExpr) -> Option<&TLocal> {
        match &expr.kind {
            TExprKind::Local(local) => Some(local),
            TExprKind::Borrow { place, .. } => Self::raw_place_local(place),
            TExprKind::DistinctCtor { arg, .. } => Self::raw_place_local(arg),
            _ => None,
        }
    }

    fn enum_discriminant(&mut self, subject: Value, heap: bool) -> Value {
        if heap {
            let zero = self.b.ins().iconst(types::I64, 0);
            let get = self
                .module
                .declare_func_in_func(self.host.struct_get_i64, self.b.func);
            let call = self.b.ins().call(get, &[subject, zero]);
            self.b.inst_results(call)[0]
        } else {
            let mask = self.b.ins().iconst(types::I64, 0xff);
            self.b.ins().band(subject, mask)
        }
    }

    fn lower_pattern_condition(
        &mut self,
        subject: Value,
        pattern: &Pattern,
        enum_name: Option<&str>,
        heap: bool,
    ) -> Result<Value, String> {
        match pattern {
            Pattern::Range { lo, hi, .. } => {
                let lo = self.b.ins().iconst(types::I64, *lo);
                let hi = self.b.ins().iconst(types::I64, *hi);
                let ge = self.bool_from_icmp(IntCC::SignedGreaterThanOrEqual, subject, lo);
                let le = self.bool_from_icmp(IntCC::SignedLessThanOrEqual, subject, hi);
                Ok(self.b.ins().band(ge, le))
            }
            Pattern::Or(alternatives, _) => {
                let mut result = self.b.ins().iconst(types::I8, 0);
                for alternative in alternatives {
                    let condition =
                        self.lower_pattern_condition(subject, alternative, enum_name, heap)?;
                    result = self.b.ins().bor(result, condition);
                }
                Ok(result)
            }
            Pattern::Variant {
                variant, bindings, ..
            } => {
                let enum_name = enum_name.ok_or("jit enum pattern missing type")?;
                let indices = self.meta.enum_variant_indices(enum_name, variant);
                if indices.is_empty() {
                    return Err(format!("jit enum `{enum_name}::{variant}`"));
                }
                let actual = self.enum_discriminant(subject, heap);
                let mut matches_variant = self.b.ins().iconst(types::I8, 0);
                for index in indices {
                    let expected = self.b.ins().iconst(types::I64, index);
                    let equal = self.bool_from_icmp(IntCC::Equal, actual, expected);
                    matches_variant = self.b.ins().bor(matches_variant, equal);
                }
                let Some(PatSlot::Range { lo, hi }) = bindings.first() else {
                    return Ok(matches_variant);
                };
                let payload_ty = self
                    .meta
                    .enum_variant_payload_types(enum_name, variant)
                    .and_then(|types| types.first())
                    .cloned()
                    .unwrap_or(Type::Int);
                let payload = if heap {
                    self.unpack_enum_heap_payload(subject, &payload_ty)?
                } else {
                    self.unpack_enum_scalar(subject, &payload_ty)?
                };
                let lo = self.b.ins().iconst(types::I64, *lo);
                let hi = self.b.ins().iconst(types::I64, *hi);
                let ge = self.bool_from_icmp(IntCC::SignedGreaterThanOrEqual, payload, lo);
                let le = self.bool_from_icmp(IntCC::SignedLessThanOrEqual, payload, hi);
                let in_range = self.b.ins().band(ge, le);
                Ok(self.b.ins().band(matches_variant, in_range))
            }
            _ => Err("jit enum arm is not a supported pattern".to_string()),
        }
    }

    fn pattern_binding(pattern: &Pattern) -> Option<(&str, &str)> {
        match pattern {
            Pattern::Variant {
                variant, bindings, ..
            } => bindings
                .first()
                .and_then(PatSlot::as_bind)
                .map(|binding| (variant.as_str(), binding)),
            Pattern::Or(alternatives, _) => alternatives.first().and_then(Self::pattern_binding),
            _ => None,
        }
    }

    fn emit_dummy_return(&mut self) {
        if let Some(sender) = self.yield_sender {
            let close = self
                .module
                .declare_func_in_func(self.host.conc.sender_close, self.b.func);
            self.b.ins().call(close, &[sender]);
        }
        self.emit_deadline_pops_to(0);
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

    fn emit_deadline_pops_to(&mut self, target_depth: u32) {
        let pop = self
            .module
            .declare_func_in_func(self.host.conc.deadline_pop, self.b.func);
        for _ in target_depth..self.deadline_depth {
            self.b.ins().call(pop, &[]);
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

    fn emit_shared_transaction_aborts_to(&mut self, target_depth: u32) {
        let abort = self
            .module
            .declare_func_in_func(self.host.memory.shared_txn_abort, self.b.func);
        for _ in target_depth..self.shared_transaction_depth {
            self.b.ins().call(abort, &[]);
        }
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
        self.emit_shared_transaction_aborts_to(0);
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

    fn result_from_packed_i64(&mut self, status: Value) -> Value {
        let zero = self.b.ins().iconst(types::I64, 0);
        let present = self.b.ins().icmp(IntCC::NotEqual, status, zero);
        let one = self.b.ins().iconst(types::I64, 1);
        let payload = self.b.ins().isub(status, one);
        let payload = self.b.ins().select(present, payload, zero);
        let ok_one = self.b.ins().iconst(types::I8, 1);
        let ok_zero = self.b.ins().iconst(types::I8, 0);
        let ok = self.b.ins().select(present, ok_one, ok_zero);
        let host = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let call = self.b.ins().call(host, &[ok, payload]);
        self.b.inst_results(call)[0]
    }

    fn result_payload(&mut self, handle: Value, ty: &Type) -> Result<Value, String> {
        if matches!(ty, Type::Named(n) if n == "Unit" || n == "Void")
            || matches!(ty, Type::Tuple(items) if items.is_empty())
        {
            return Ok(self.b.ins().iconst(types::I8, 0));
        }
        let erased = match ty {
            Type::Named(name) => self.meta.distinct_base(name).unwrap_or(ty),
            _ => ty,
        };
        let host_id = match clif_ty(erased) {
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

    fn scalar_bitcast_memflags() -> MemFlags {
        MemFlags::new().with_endianness(Endianness::Little)
    }

    fn pack_enum_scalar(
        &mut self,
        disc: i64,
        payload: Value,
        payload_ty: &Type,
    ) -> Result<Value, String> {
        let disc_v = self.b.ins().iconst(types::I64, disc);
        match self.meta.clif_ty(payload_ty) {
            Some(types::I64) => {
                let shifted = self.b.ins().ishl_imm(payload, 8);
                Ok(self.b.ins().bor(shifted, disc_v))
            }
            // F64 payloads cannot share one i64 with the disc byte: `shl 8`
            // drops the sign/exponent byte and corrupts the float. Heap-box
            // instead: record [disc:i64, payload:f64], return the handle.
            Some(types::F64) => {
                let n = self.b.ins().iconst(types::I64, 2);
                let new_ref = self
                    .module
                    .declare_func_in_func(self.host.struct_new, self.b.func);
                let call = self.b.ins().call(new_ref, &[n]);
                let handle = self.b.inst_results(call)[0];
                let zero = self.b.ins().iconst(types::I64, 0);
                let one = self.b.ins().iconst(types::I64, 1);
                let set_i = self
                    .module
                    .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                self.b.ins().call(set_i, &[handle, zero, disc_v]);
                let set_f = self
                    .module
                    .declare_func_in_func(self.host.struct_set_f64, self.b.func);
                self.b.ins().call(set_f, &[handle, one, payload]);
                Ok(handle)
            }
            Some(types::I8) => {
                let widened = self.b.ins().uextend(types::I64, payload);
                let shifted = self.b.ins().ishl_imm(widened, 8);
                Ok(self.b.ins().bor(shifted, disc_v))
            }
            Some(types::I32) => {
                let widened = self.b.ins().sextend(types::I64, payload);
                let shifted = self.b.ins().ishl_imm(widened, 8);
                Ok(self.b.ins().bor(shifted, disc_v))
            }
            other => Err(format!(
                "jit enum payload type unsupported: {payload_ty:?} ({other:?})"
            )),
        }
    }

    fn unpack_enum_scalar(&mut self, packed: Value, payload_ty: &Type) -> Result<Value, String> {
        match self.meta.clif_ty(payload_ty) {
            Some(types::F64) => {
                let one = self.b.ins().iconst(types::I64, 1);
                let get_f = self
                    .module
                    .declare_func_in_func(self.host.struct_get_f64, self.b.func);
                let call = self.b.ins().call(get_f, &[packed, one]);
                Ok(self.b.inst_results(call)[0])
            }
            Some(types::I64) => {
                let raw = self.b.ins().sshr_imm(packed, 8);
                Ok(raw)
            }
            Some(types::I8) => {
                let raw = self.b.ins().sshr_imm(packed, 8);
                Ok(self.b.ins().ireduce(types::I8, raw))
            }
            Some(types::I32) => {
                let raw = self.b.ins().sshr_imm(packed, 8);
                Ok(self.b.ins().ireduce(types::I32, raw))
            }
            other => Err(format!(
                "jit enum payload type unsupported: {payload_ty:?} ({other:?})"
            )),
        }
    }

    /// True when this enum match uses heap-boxed payloads (F64 or DataTree ABI).
    fn enum_match_uses_f64_heap(&self, arms: &[TIR::TMatchArm]) -> bool {
        arms.iter().any(|arm| {
            let Some(variant) = arm.pattern.variant() else {
                return false;
            };
            let Some(enum_name) = arm.pattern.enum_type.as_deref() else {
                return false;
            };
            if matches!(enum_name, "DataTree" | "Json" | "Toml" | "Yaml" | "Csv") {
                return true;
            }
            self.meta
                .enum_variant_payload_types(enum_name, variant)
                .and_then(|tys| tys.first())
                .is_some_and(|ty| matches!(ty, Type::Float | Type::Float32))
        })
    }

    fn is_datatree_value_ty(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Named(n) if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
        )
    }

    /// Constant string from `(…) => "lit"` / block with sole string return.
    fn constant_string_lambda(expr: &TExpr) -> Option<String> {
        let TExprKind::Lambda(lam) = &expr.kind else {
            return None;
        };
        let body_expr = match &lam.executable {
            TLambdaBody::Expr(body) => body.as_ref(),
            TLambdaBody::Block(stmts) => match stmts.as_slice() {
                [TStmt::Return(Some(e))] | [TStmt::ExprStmt(e)] => e,
                _ => return None,
            },
        };
        match &body_expr.kind {
            TExprKind::StrLit(parts) if parts.len() == 1 => match &parts[0] {
                TStrPart::Lit(s) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// TIR `handle_method_return_ty` omits DataTree accessors (Unit fallback).
    /// Recover the real `Result` ok payload for `??` / `?`.
    fn datatree_handle_ok_ty(value: &TExpr) -> Option<Type> {
        let TExprKind::HandleMethod { op, .. } = &value.kind else {
            return None;
        };
        match op {
            THandleOp::DataTreeField
            | THandleOp::JsonField
            | THandleOp::DataTreeAt
            | THandleOp::JsonAt => Some(Type::Named("DataTree".into())),
            THandleOp::DataTreeInt | THandleOp::JsonInt => Some(Type::Int),
            THandleOp::DataTreeText | THandleOp::JsonText => Some(Type::String),
            THandleOp::DataTreeBool | THandleOp::JsonBool => Some(Type::Bool),
            THandleOp::DataTreeFloat | THandleOp::JsonFloat => Some(Type::Float),
            // ParsedArgs handle lives in result bits (i64).
            THandleOp::ArgsSpecParse => Some(Type::Int),
            _ => None,
        }
    }

    /// Recover CORE call / sketch return types when TIR left `Unit`.
    fn recover_core_return_ty(expr: &TExpr) -> Option<Type> {
        match &expr.kind {
            TExprKind::CoreCall {
                module, method, args, ..
            } if module == "core.text" => match method.as_str() {
                "byte_count" | "scalar_count" | "display_width" if args.len() == 1 => {
                    Some(Type::Int)
                }
                // Policy overload returns Result<Int, TextError>.
                "display_width" if args.len() == 2 => None,
                "caseless_eq" | "is_alphabetic" | "is_numeric" | "starts_any" => {
                    Some(Type::Bool)
                }
                "lower" | "upper" | "nfc" | "nfkc" | "nfd" | "nfkd" | "pad_start"
                | "center" | "trim" => Some(Type::String),
                "graphemes" | "words" | "sentences" | "char_indices" => {
                    Some(Type::List(Box::new(Type::String)))
                }
                _ => None,
            },
            TExprKind::HandleMethod {
                op: THandleOp::SketchMethod { method, .. },
                args,
                ..
            } => match method.as_str() {
                "count" => Some(Type::Int),
                "quantile" => Some(Type::Float),
                "sample" if args.is_empty() => Some(Type::List(Box::new(Type::String))),
                _ => None,
            },
            TExprKind::HandleMethod { op, args, .. } => match op {
                THandleOp::ParsedArgsFlag => Some(Type::Bool),
                THandleOp::ParsedArgsOption
                | THandleOp::ParsedArgsPositional
                | THandleOp::ParsedArgsSubcommand => {
                    Some(Type::Option(Box::new(Type::String)))
                }
                THandleOp::ArgsSpecHelp | THandleOp::ArgsSpecCompletion => Some(Type::String),
                THandleOp::ParsedArgsOptionInt => Some(Type::Option(Box::new(Type::Int))),
                THandleOp::ParsedArgsOptionFloat => Some(Type::Option(Box::new(Type::Float))),
                THandleOp::ParsedArgsOptions => Some(Type::List(Box::new(Type::String))),
                THandleOp::ArgsSpecFlag
                | THandleOp::ArgsSpecFlagShort
                | THandleOp::ArgsSpecOption
                | THandleOp::ArgsSpecOptionDefault
                | THandleOp::ArgsSpecOptionInt
                | THandleOp::ArgsSpecOptionChoice
                | THandleOp::ArgsSpecRepeat
                | THandleOp::ArgsSpecPositional
                | THandleOp::ArgsSpecSubcommand
                | THandleOp::ArgsSpecVersion => Some(Type::Int), // ArgsSpec handle
                THandleOp::ArgsSpecParse => Some(Type::Int), // Result handle
                // D-DET-CAPAPI: TIR may leave these as Unit; recover Int for print/interp.
                THandleOp::ClockNow
                | THandleOp::ClockTick
                | THandleOp::ClockAdvance
                | THandleOp::ClockWait => Some(Type::Int),
                THandleOp::DurationIn { .. } => Some(Type::Option(Box::new(Type::Int))),
                _ => {
                    let _ = args;
                    None
                }
            },
            TExprKind::BuiltinMethod {
                op: TBuiltinOp::LenList | TBuiltinOp::LenString,
                ..
            } => Some(Type::Int),
            _ => None,
        }
    }

    fn result_ok_ty_recover(value: &TExpr) -> Option<Type> {
        if let Some(ty) = Self::datatree_handle_ok_ty(value) {
            return Some(ty);
        }
        if let TExprKind::CoreCall {
            module, method, args, ..
        } = &value.kind
        {
            if module == "core.text" && method == "display_width" && args.len() == 2 {
                return Some(Type::Int);
            }
        }
        None
    }

    fn datatree_variant_disc(variant: &str) -> Option<i64> {
        match variant {
            "Null" => Some(0),
            "Bool" => Some(1),
            "Int" => Some(2),
            "Float" => Some(3),
            "Text" => Some(4),
            "Array" => Some(5),
            "Object" => Some(6),
            _ => None,
        }
    }

    fn unpack_enum_heap_payload(
        &mut self,
        packed: Value,
        payload_ty: &Type,
    ) -> Result<Value, String> {
        let one = self.b.ins().iconst(types::I64, 1);
        match self.meta.clif_ty(payload_ty) {
            Some(types::F64) => {
                let get_f = self
                    .module
                    .declare_func_in_func(self.host.struct_get_f64, self.b.func);
                let call = self.b.ins().call(get_f, &[packed, one]);
                Ok(self.b.inst_results(call)[0])
            }
            Some(types::I64) => {
                let get_i = self
                    .module
                    .declare_func_in_func(self.host.struct_get_i64, self.b.func);
                let call = self.b.ins().call(get_i, &[packed, one]);
                Ok(self.b.inst_results(call)[0])
            }
            Some(types::I8) => {
                let get_i = self
                    .module
                    .declare_func_in_func(self.host.struct_get_i64, self.b.func);
                let call = self.b.ins().call(get_i, &[packed, one]);
                let raw = self.b.inst_results(call)[0];
                Ok(self.b.ins().ireduce(types::I8, raw))
            }
            Some(types::I32) => {
                let get_i = self
                    .module
                    .declare_func_in_func(self.host.struct_get_i64, self.b.func);
                let call = self.b.ins().call(get_i, &[packed, one]);
                let raw = self.b.inst_results(call)[0];
                Ok(self.b.ins().ireduce(types::I32, raw))
            }
            other => Err(format!(
                "jit enum heap payload type unsupported: {payload_ty:?} ({other:?})"
            )),
        }
    }

    fn pack_datatree_enum(
        &mut self,
        disc: i64,
        payload: Option<(Value, &Type)>,
    ) -> Result<Value, String> {
        let disc_v = self.b.ins().iconst(types::I64, disc);
        let payload_bits = match payload {
            None => self.b.ins().iconst(types::I64, 0),
            Some((v, ty)) => match self.meta.clif_ty(ty) {
                Some(types::F64) => {
                    // Store as bits so `datatree_pack` can record_set_float.
                    self.b.ins().bitcast(
                        types::I64,
                        Self::scalar_bitcast_memflags(),
                        v,
                    )
                }
                Some(types::I8) => self.b.ins().uextend(types::I64, v),
                Some(types::I32) => self.b.ins().sextend(types::I64, v),
                Some(types::I64) => v,
                other => {
                    return Err(format!(
                        "jit DataTree payload type unsupported: {ty:?} ({other:?})"
                    ))
                }
            },
        };
        let host_ref = self
            .module
            .declare_func_in_func(self.host.encoding.datatree_pack, self.b.func);
        let call = self.b.ins().call(host_ref, &[disc_v, payload_bits]);
        Ok(self.b.inst_results(call)[0])
    }

    /// Primitive / container / user `Encode` → DataTree handle (D-SERDE2).
    fn lower_serde_encode_value(&mut self, val: Value, ty: &Type) -> Result<Value, String> {
        let ty = self.erase_distinct_ty(ty);
        if Self::is_datatree_value_ty(&ty) {
            return Ok(val);
        }
        match &ty {
            Type::Int | Type::IntN { .. } => self.pack_datatree_enum(2, Some((val, &Type::Int))),
            Type::Bool => {
                let wide = self.b.ins().uextend(types::I64, val);
                self.pack_datatree_enum(1, Some((wide, &Type::Int)))
            }
            Type::Float | Type::Float32 => {
                self.pack_datatree_enum(3, Some((val, &Type::Float)))
            }
            Type::String => self.pack_datatree_enum(4, Some((val, &Type::String))),
            Type::Char => {
                return Err("jit SerdeEncode Char unsupported".to_string());
            }
            Type::Option(inner) => {
                let zero = self.b.ins().iconst(types::I64, 0);
                let is_none = self.b.ins().icmp(IntCC::Equal, val, zero);
                let none_block = self.b.create_block();
                let some_block = self.b.create_block();
                let merge = self.b.create_block();
                self.b.append_block_param(merge, types::I64);
                self.b
                    .ins()
                    .brif(is_none, none_block, &[], some_block, &[]);

                self.b.switch_to_block(none_block);
                self.b.seal_block(none_block);
                let null_tree = self.pack_datatree_enum(0, None)?;
                self.b.ins().jump(merge, &[null_tree]);

                self.b.switch_to_block(some_block);
                self.b.seal_block(some_block);
                let payload = self.unpack_option_payload(val, inner)?;
                let encoded = self.lower_serde_encode_value(payload, inner)?;
                self.b.ins().jump(merge, &[encoded]);

                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                Ok(self.b.block_params(merge)[0])
            }
            Type::List(elem) | Type::FixedList { elem, .. } => {
                self.lower_serde_encode_list(val, elem)
            }
            Type::Union(members)
                if !members.is_empty()
                    && members
                        .iter()
                        .all(|member| self.meta.clif_ty(member) != Some(types::F64)) =>
            {
                let value_var = self.fresh_var(types::I64);
                self.b.def_var(value_var, val);
                let merge = self.b.create_block();
                self.b.append_block_param(merge, types::I64);
                let mask = self.b.ins().iconst(types::I64, 0xff);
                let disc = self.b.ins().band(val, mask);
                for (index, member) in members.iter().enumerate() {
                    let arm = self.b.create_block();
                    let next = if index + 1 == members.len() {
                        self.b.ins().jump(arm, &[]);
                        None
                    } else {
                        let next = self.b.create_block();
                        let expected = self.b.ins().iconst(types::I64, index as i64);
                        let matched = self.bool_from_icmp(IntCC::Equal, disc, expected);
                        self.b.ins().brif(matched, arm, &[], next, &[]);
                        Some(next)
                    };
                    self.b.switch_to_block(arm);
                    self.b.seal_block(arm);
                    let packed = self.b.use_var(value_var);
                    let payload = self.unpack_enum_scalar(packed, member)?;
                    let encoded = self.lower_serde_encode_value(payload, member)?;
                    self.b.ins().jump(merge, &[encoded]);
                    if let Some(next) = next {
                        self.b.switch_to_block(next);
                        self.b.seal_block(next);
                    }
                }
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                Ok(self.b.block_params(merge)[0])
            }
            Type::Named(_) | Type::Apply { .. } => {
                let key = self
                    .serde_codec_key(&ty, "encode")
                    .ok_or_else(|| format!("jit SerdeEncode unsupported: {ty:?}"))?;
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing encode `{key}`"))?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &[val]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
            }
            other => Err(format!("jit SerdeEncode unsupported: {other:?}")),
        }
    }

    /// Monomorphized Codable methods are named `Wrap<Int>::encode`; fall back to
    /// `Wrap::encode` when only the base owner was lowered.
    fn serde_codec_key(&self, ty: &Type, method: &str) -> Option<String> {
        let base = user_type_name(ty)?;
        if matches!(ty, Type::Apply { .. }) {
            let concrete = format!("{}::{method}", ty.name());
            if self.func_ids.contains_key(&concrete) {
                return Some(concrete);
            }
        }
        Some(format!("{base}::{method}"))
    }

    fn lower_serde_encode_list(&mut self, list: Value, elem_ty: &Type) -> Result<Value, String> {
        let list_var = self.fresh_var(types::I64);
        self.b.def_var(list_var, list);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, self.b.inst_results(out_call)[0]);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
        let exit = self.b.create_block();
        let idx_var = self.fresh_var(types::I64);
        let zero = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(idx_var, zero);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        let idx = self.b.use_var(idx_var);
        let coll = self.b.use_var(list_var);
        let len_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_len, self.b.func);
        let len_call = self.b.ins().call(len_ref, &[coll]);
        let len = self.b.inst_results(len_call)[0];
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let encoded = self.lower_serde_encode_value(elem, elem_ty)?;
        let out = self.b.use_var(out_var);
        let push_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);
        self.b.ins().call(push_ref, &[out, encoded]);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        let out = self.b.use_var(out_var);
        self.pack_datatree_enum(5, Some((out, &Type::List(Box::new(Type::Named("DataTree".into()))))))
    }

    fn lower_serde_encode(&mut self, recv: &TExpr) -> Result<Value, String> {
        let val = self.lower_expr(recv)?;
        self.lower_serde_encode_value(val, &recv.ty)
    }

    /// `DataTree` → typed value Result (primitives via hosts; user types via `T::decode`).
    fn lower_datatree_decode(&mut self, tree: Value, target: &Type) -> Result<Value, String> {
        let target = self.erase_distinct_ty(target);
        if Self::is_datatree_value_ty(&target) {
            let tag = self.b.ins().iconst(types::I8, 1);
            let host_ref = self
                .module
                .declare_func_in_func(self.host.result_new_i64, self.b.func);
            let call = self.b.ins().call(host_ref, &[tag, tree]);
            return Ok(self.b.inst_results(call)[0]);
        }
        match &target {
            Type::Int | Type::IntN { .. } => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_int, self.b.func);
                let call = self.b.ins().call(host_ref, &[tree]);
                Ok(self.b.inst_results(call)[0])
            }
            Type::String => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_text, self.b.func);
                let call = self.b.ins().call(host_ref, &[tree]);
                Ok(self.b.inst_results(call)[0])
            }
            Type::Bool => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_bool, self.b.func);
                let call = self.b.ins().call(host_ref, &[tree]);
                Ok(self.b.inst_results(call)[0])
            }
            Type::Float | Type::Float32 => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_float, self.b.func);
                let call = self.b.ins().call(host_ref, &[tree]);
                Ok(self.b.inst_results(call)[0])
            }
            Type::Option(inner) => self.lower_datatree_decode_option(tree, inner),
            Type::List(elem) | Type::FixedList { elem, .. } => {
                self.lower_datatree_decode_list(tree, elem)
            }
            Type::Union(members)
                if !members.is_empty()
                    && members.iter().all(|member| {
                        matches!(
                            member,
                            Type::Int
                                | Type::IntN { .. }
                                | Type::String
                                | Type::Bool
                                | Type::Float
                                | Type::Float32
                        )
                    }) =>
            {
                let zero = self.b.ins().iconst(types::I64, 0);
                let get = self
                    .module
                    .declare_func_in_func(self.host.struct_get_i64, self.b.func);
                let call = self.b.ins().call(get, &[tree, zero]);
                let tree_disc = self.b.inst_results(call)[0];
                let merge = self.b.create_block();
                self.b.append_block_param(merge, types::I64);
                for (index, member) in members.iter().enumerate() {
                    let member_disc = match member {
                        Type::Bool => 1,
                        Type::Int | Type::IntN { .. } => 2,
                        Type::Float | Type::Float32 => 3,
                        Type::String => 4,
                        _ => unreachable!(),
                    };
                    let arm = self.b.create_block();
                    let next = self.b.create_block();
                    let expected = self.b.ins().iconst(types::I64, member_disc);
                    let matched = self.bool_from_icmp(IntCC::Equal, tree_disc, expected);
                    self.b.ins().brif(matched, arm, &[], next, &[]);
                    self.b.switch_to_block(arm);
                    self.b.seal_block(arm);
                    let decoded = self.lower_datatree_decode(tree, member)?;
                    let status = self
                        .module
                        .declare_func_in_func(self.host.result_is_ok, self.b.func);
                    let status_call = self.b.ins().call(status, &[decoded]);
                    let ok = self.b.inst_results(status_call)[0];
                    let success = self.b.create_block();
                    let failure = self.b.create_block();
                    self.b.ins().brif(ok, success, &[], failure, &[]);
                    self.b.switch_to_block(failure);
                    self.b.seal_block(failure);
                    self.b.ins().jump(merge, &[decoded]);
                    self.b.switch_to_block(success);
                    self.b.seal_block(success);
                    let payload = self.result_payload(decoded, member)?;
                    let packed = self.pack_enum_scalar(index as i64, payload, member)?;
                    let ok_tag = self.b.ins().iconst(types::I8, 1);
                    let result = self
                        .module
                        .declare_func_in_func(self.host.result_new_i64, self.b.func);
                    let result_call = self.b.ins().call(result, &[ok_tag, packed]);
                    let result_value = self.b.inst_results(result_call)[0];
                    self.b.ins().jump(merge, &[result_value]);
                    self.b.switch_to_block(next);
                    self.b.seal_block(next);
                }
                let message = self
                    .runtime
                    .heap
                    .alloc_string("value does not match any union member".to_string());
                let message = self.b.ins().iconst(types::I64, message);
                let err_tag = self.b.ins().iconst(types::I8, 0);
                let result = self
                    .module
                    .declare_func_in_func(self.host.result_new_i64, self.b.func);
                let result_call = self.b.ins().call(result, &[err_tag, message]);
                let result_value = self.b.inst_results(result_call)[0];
                self.b.ins().jump(merge, &[result_value]);
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                Ok(self.b.block_params(merge)[0])
            }
            Type::Named(_) | Type::Apply { .. } => {
                let key = self
                    .serde_codec_key(&target, "decode")
                    .ok_or_else(|| format!("jit DataTreeDecode unsupported: {target:?}"))?;
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing decode `{key}`"))?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &[tree]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
            }
            other => Err(format!("jit DataTreeDecode unsupported: {other:?}")),
        }
    }

    fn lower_datatree_decode_option(
        &mut self,
        tree: Value,
        inner: &Type,
    ) -> Result<Value, String> {
        let zero = self.b.ins().iconst(types::I64, 0);
        let get_i = self
            .module
            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
        let disc_call = self.b.ins().call(get_i, &[tree, zero]);
        let disc = self.b.inst_results(disc_call)[0];
        let is_null = self.b.ins().icmp(IntCC::Equal, disc, zero);
        let none_block = self.b.create_block();
        let some_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        self.b
            .ins()
            .brif(is_null, none_block, &[], some_block, &[]);

        self.b.switch_to_block(none_block);
        self.b.seal_block(none_block);
        let tag = self.b.ins().iconst(types::I8, 1);
        let none_val = self.b.ins().iconst(types::I64, 0);
        let host_ref = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let ok_none = self.b.ins().call(host_ref, &[tag, none_val]);
        let ok_none = self.b.inst_results(ok_none)[0];
        self.b.ins().jump(merge, &[ok_none]);

        self.b.switch_to_block(some_block);
        self.b.seal_block(some_block);
        let inner_r = self.lower_datatree_decode(tree, inner)?;
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[inner_r]);
        let is_ok = self.b.inst_results(status_call)[0];
        let ok_block = self.b.create_block();
        let err_block = self.b.create_block();
        self.b.ins().brif(is_ok, ok_block, &[], err_block, &[]);

        self.b.switch_to_block(err_block);
        self.b.seal_block(err_block);
        self.b.ins().jump(merge, &[inner_r]);

        self.b.switch_to_block(ok_block);
        self.b.seal_block(ok_block);
        let payload = self.result_payload(inner_r, inner)?;
        let present = self.pack_option_payload(payload, inner)?;
        let tag = self.b.ins().iconst(types::I8, 1);
        let host_ref = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let ok_some = self.b.ins().call(host_ref, &[tag, present]);
        let ok_some = self.b.inst_results(ok_some)[0];
        self.b.ins().jump(merge, &[ok_some]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_datatree_decode_list(
        &mut self,
        tree: Value,
        elem_ty: &Type,
    ) -> Result<Value, String> {
        // Expect Array; reuse `.at` path via host disc check + payload list.
        let zero = self.b.ins().iconst(types::I64, 0);
        let get_i = self
            .module
            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
        let disc_call = self.b.ins().call(get_i, &[tree, zero]);
        let disc = self.b.inst_results(disc_call)[0];
        let want = self.b.ins().iconst(types::I64, 5); // Array
        let is_arr = self.b.ins().icmp(IntCC::Equal, disc, want);
        let bad_block = self.b.create_block();
        let good_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        self.b
            .ins()
            .brif(is_arr, good_block, &[], bad_block, &[]);

        self.b.switch_to_block(bad_block);
        self.b.seal_block(bad_block);
        let sid = self.runtime.heap.alloc_string("expected array".to_string());
        let msg = self.b.ins().iconst(types::I64, sid);
        let tag = self.b.ins().iconst(types::I8, 0);
        let host_ref = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let err = self.b.ins().call(host_ref, &[tag, msg]);
        let err = self.b.inst_results(err)[0];
        self.b.ins().jump(merge, &[err]);

        self.b.switch_to_block(good_block);
        self.b.seal_block(good_block);
        let one = self.b.ins().iconst(types::I64, 1);
        let payload_call = self.b.ins().call(get_i, &[tree, one]);
        let src_list = self.b.inst_results(payload_call)[0];
        let decoded = self.lower_datatree_decode_list_items(src_list, elem_ty)?;
        self.b.ins().jump(merge, &[decoded]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_datatree_decode_list_items(
        &mut self,
        src_list: Value,
        elem_ty: &Type,
    ) -> Result<Value, String> {
        let list_var = self.fresh_var(types::I64);
        self.b.def_var(list_var, src_list);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, self.b.inst_results(out_call)[0]);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
        let exit = self.b.create_block();
        let fail = self.b.create_block();
        self.b.append_block_param(fail, types::I64);
        let idx_var = self.fresh_var(types::I64);
        let zero = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(idx_var, zero);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        let idx = self.b.use_var(idx_var);
        let coll = self.b.use_var(list_var);
        let len_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_len, self.b.func);
        let len_call = self.b.ins().call(len_ref, &[coll]);
        let len = self.b.inst_results(len_call)[0];
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem_tree = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let decoded_r = self.lower_datatree_decode(elem_tree, elem_ty)?;
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[decoded_r]);
        let is_ok = self.b.inst_results(status_call)[0];
        let ok_block = self.b.create_block();
        self.b.ins().brif(is_ok, ok_block, &[], fail, &[decoded_r]);

        self.b.switch_to_block(ok_block);
        self.b.seal_block(ok_block);
        let payload = self.result_payload(decoded_r, elem_ty)?;
        let out = self.b.use_var(out_var);
        let push_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);
        self.b.ins().call(push_ref, &[out, payload]);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        let out = self.b.use_var(out_var);
        let tag = self.b.ins().iconst(types::I8, 1);
        let host_ref = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let ok = self.b.ins().call(host_ref, &[tag, out]);
        let ok = self.b.inst_results(ok)[0];
        self.b.ins().jump(merge, &[ok]);

        self.b.switch_to_block(fail);
        self.b.seal_block(fail);
        let err = self.b.block_params(fail)[0];
        self.b.ins().jump(merge, &[err]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_typed_json_decode(&mut self, text: &TExpr, ok_ty: &Type) -> Result<Value, String> {
        self.lower_typed_tree_decode(text, ok_ty, self.host.encoding.json_parse)
    }

    fn lower_typed_json_decode_traced(
        &mut self,
        text: &TExpr,
        result_ok_ty: &Type,
    ) -> Result<Value, String> {
        // result_ok_ty = DecodeResult<T>
        let inner_ty = match result_ok_ty {
            Type::Apply { name, args } if name == "DecodeResult" => args
                .first()
                .cloned()
                .ok_or_else(|| "jit decode_traced missing DecodeResult arg".to_string())?,
            Type::Named(n) if n == "DecodeResult" => {
                return Err("jit decode_traced DecodeResult needs type arg".into());
            }
            other => {
                return Err(format!(
                    "jit decode_traced expected DecodeResult, got {other:?}"
                ));
            }
        };
        let text_v = self.lower_expr(text)?;
        let host_ref = self
            .module
            .declare_func_in_func(self.host.encoding.json_parse, self.b.func);
        let parse_call = self.b.ins().call(host_ref, &[text_v]);
        let parsed = self.b.inst_results(parse_call)[0];
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[parsed]);
        let is_ok = self.b.inst_results(status_call)[0];
        let ok_block = self.b.create_block();
        let err_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        self.b.ins().brif(is_ok, ok_block, &[], err_block, &[]);

        self.b.switch_to_block(err_block);
        self.b.seal_block(err_block);
        self.b.ins().jump(merge, &[parsed]);

        self.b.switch_to_block(ok_block);
        self.b.seal_block(ok_block);
        let tree = self.result_payload(parsed, &Type::Named("DataTree".into()))?;
        let wrapped = self.lower_datatree_decode_traced(tree, &inner_ty)?;
        self.b.ins().jump(merge, &[wrapped]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    /// Decode DataTree → T, applying registered migrations on failure (D-MIGRATE4).
    fn lower_datatree_decode_migrating(
        &mut self,
        tree: Value,
        target: &Type,
    ) -> Result<Value, String> {
        let decoded = self.lower_datatree_decode(tree, target)?;
        let Type::Named(type_name) = target else {
            return Ok(decoded);
        };
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[decoded]);
        let is_ok = self.b.inst_results(status_call)[0];
        let done = self.b.create_block();
        let try_mig = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        self.b.ins().brif(is_ok, done, &[], try_mig, &[]);

        self.b.switch_to_block(done);
        self.b.seal_block(done);
        self.b.ins().jump(merge, &[decoded]);

        self.b.switch_to_block(try_mig);
        self.b.seal_block(try_mig);
        let name_id = self.runtime.heap.alloc_string(type_name.clone());
        let name_v = self.b.ins().iconst(types::I64, name_id);
        let mig_ref = self
            .module
            .declare_func_in_func(self.host.encoding.datatree_migrate, self.b.func);
        let mig_call = self.b.ins().call(mig_ref, &[name_v, tree]);
        let mig_r = self.b.inst_results(mig_call)[0];
        let mig_ok_call = self.b.ins().call(status_ref, &[mig_r]);
        let mig_ok = self.b.inst_results(mig_ok_call)[0];
        let mig_yes = self.b.create_block();
        let mig_no = self.b.create_block();
        self.b.ins().brif(mig_ok, mig_yes, &[], mig_no, &[]);

        self.b.switch_to_block(mig_no);
        self.b.seal_block(mig_no);
        // Keep the original decode Err.
        self.b.ins().jump(merge, &[decoded]);

        self.b.switch_to_block(mig_yes);
        self.b.seal_block(mig_yes);
        let mig_rec = self.result_payload(mig_r, &Type::Named("MigrationProbe".into()))?;
        // Migration probe record: [tree, from, steps]
        let get_i = self
            .module
            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
        let zero = self.b.ins().iconst(types::I64, 0);
        let new_tree_call = self.b.ins().call(get_i, &[mig_rec, zero]);
        let new_tree = self.b.inst_results(new_tree_call)[0];
        let decoded2 = self.lower_datatree_decode(new_tree, target)?;
        self.b.ins().jump(merge, &[decoded2]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_migration_status_fresh(&mut self) -> Result<Value, String> {
        let n = self.b.ins().iconst(types::I64, 3);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[n]);
        let handle = self.b.inst_results(new_call)[0];
        let set_b = self
            .module
            .declare_func_in_func(self.host.struct_set_bool, self.b.func);
        let set_s = self
            .module
            .declare_func_in_func(self.host.struct_set_str, self.b.func);
        let set_i = self
            .module
            .declare_func_in_func(self.host.struct_set_i64, self.b.func);
        let idx0 = self.b.ins().iconst(types::I64, 0);
        let fals = self.b.ins().iconst(types::I8, 0);
        self.b.ins().call(set_b, &[handle, idx0, fals]);
        let empty = self.runtime.heap.alloc_string(String::new());
        let empty_v = self.b.ins().iconst(types::I64, empty);
        let idx1 = self.b.ins().iconst(types::I64, 1);
        self.b.ins().call(set_s, &[handle, idx1, empty_v]);
        let list_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let list_call = self.b.ins().call(list_ref, &[]);
        let empty_list = self.b.inst_results(list_call)[0];
        let idx2 = self.b.ins().iconst(types::I64, 2);
        self.b.ins().call(set_i, &[handle, idx2, empty_list]);
        Ok(handle)
    }

    fn lower_migration_status_from_probe(&mut self, mig_rec: Value) -> Result<Value, String> {
        let get_i = self
            .module
            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
        let one = self.b.ins().iconst(types::I64, 1);
        let two = self.b.ins().iconst(types::I64, 2);
        let from_call = self.b.ins().call(get_i, &[mig_rec, one]);
        let from = self.b.inst_results(from_call)[0];
        let steps_call = self.b.ins().call(get_i, &[mig_rec, two]);
        let steps = self.b.inst_results(steps_call)[0];
        let n = self.b.ins().iconst(types::I64, 3);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[n]);
        let handle = self.b.inst_results(new_call)[0];
        let set_b = self
            .module
            .declare_func_in_func(self.host.struct_set_bool, self.b.func);
        let set_s = self
            .module
            .declare_func_in_func(self.host.struct_set_str, self.b.func);
        let set_i = self
            .module
            .declare_func_in_func(self.host.struct_set_i64, self.b.func);
        let idx0 = self.b.ins().iconst(types::I64, 0);
        let tru = self.b.ins().iconst(types::I8, 1);
        self.b.ins().call(set_b, &[handle, idx0, tru]);
        let idx1 = self.b.ins().iconst(types::I64, 1);
        self.b.ins().call(set_s, &[handle, idx1, from]);
        let idx2 = self.b.ins().iconst(types::I64, 2);
        self.b.ins().call(set_i, &[handle, idx2, steps]);
        Ok(handle)
    }

    fn lower_decode_result_wrap(
        &mut self,
        value: Value,
        migration: Value,
    ) -> Result<Value, String> {
        let n = self.b.ins().iconst(types::I64, 2);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[n]);
        let handle = self.b.inst_results(new_call)[0];
        let set_i = self
            .module
            .declare_func_in_func(self.host.struct_set_i64, self.b.func);
        let idx0 = self.b.ins().iconst(types::I64, 0);
        let idx1 = self.b.ins().iconst(types::I64, 1);
        self.b.ins().call(set_i, &[handle, idx0, value]);
        self.b.ins().call(set_i, &[handle, idx1, migration]);
        let tag = self.b.ins().iconst(types::I8, 1);
        let host_ref = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let call = self.b.ins().call(host_ref, &[tag, handle]);
        Ok(self.b.inst_results(call)[0])
    }

    fn lower_datatree_decode_traced(
        &mut self,
        tree: Value,
        inner_ty: &Type,
    ) -> Result<Value, String> {
        let decoded = self.lower_datatree_decode(tree, inner_ty)?;
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[decoded]);
        let is_ok = self.b.inst_results(status_call)[0];
        let fresh_b = self.b.create_block();
        let try_mig = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        self.b.ins().brif(is_ok, fresh_b, &[], try_mig, &[]);

        self.b.switch_to_block(fresh_b);
        self.b.seal_block(fresh_b);
        let value = self.result_payload(decoded, inner_ty)?;
        let mig = self.lower_migration_status_fresh()?;
        let wrapped = self.lower_decode_result_wrap(value, mig)?;
        self.b.ins().jump(merge, &[wrapped]);

        self.b.switch_to_block(try_mig);
        self.b.seal_block(try_mig);
        let Type::Named(type_name) = inner_ty else {
            // Non-named: no migration path; propagate decode Err.
            self.b.ins().jump(merge, &[decoded]);
            self.b.switch_to_block(merge);
            self.b.seal_block(merge);
            return Ok(self.b.block_params(merge)[0]);
        };
        let name_id = self.runtime.heap.alloc_string(type_name.clone());
        let name_v = self.b.ins().iconst(types::I64, name_id);
        let mig_ref = self
            .module
            .declare_func_in_func(self.host.encoding.datatree_migrate, self.b.func);
        let mig_call = self.b.ins().call(mig_ref, &[name_v, tree]);
        let mig_r = self.b.inst_results(mig_call)[0];
        let mig_ok_call = self.b.ins().call(status_ref, &[mig_r]);
        let mig_ok = self.b.inst_results(mig_ok_call)[0];
        let mig_yes = self.b.create_block();
        let mig_no = self.b.create_block();
        self.b.ins().brif(mig_ok, mig_yes, &[], mig_no, &[]);

        self.b.switch_to_block(mig_no);
        self.b.seal_block(mig_no);
        self.b.ins().jump(merge, &[decoded]);

        self.b.switch_to_block(mig_yes);
        self.b.seal_block(mig_yes);
        let mig_rec = self.result_payload(mig_r, &Type::Named("MigrationProbe".into()))?;
        let get_i = self
            .module
            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
        let zero = self.b.ins().iconst(types::I64, 0);
        let new_tree_call = self.b.ins().call(get_i, &[mig_rec, zero]);
        let new_tree = self.b.inst_results(new_tree_call)[0];
        let decoded2 = self.lower_datatree_decode(new_tree, inner_ty)?;
        let d2_ok_call = self.b.ins().call(status_ref, &[decoded2]);
        let d2_ok = self.b.inst_results(d2_ok_call)[0];
        let d2_yes = self.b.create_block();
        let d2_no = self.b.create_block();
        self.b.ins().brif(d2_ok, d2_yes, &[], d2_no, &[]);

        self.b.switch_to_block(d2_no);
        self.b.seal_block(d2_no);
        self.b.ins().jump(merge, &[decoded2]);

        self.b.switch_to_block(d2_yes);
        self.b.seal_block(d2_yes);
        let value2 = self.result_payload(decoded2, inner_ty)?;
        let status = self.lower_migration_status_from_probe(mig_rec)?;
        let wrapped2 = self.lower_decode_result_wrap(value2, status)?;
        self.b.ins().jump(merge, &[wrapped2]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_typed_tree_decode(
        &mut self,
        text: &TExpr,
        ok_ty: &Type,
        parse_host: FuncId,
    ) -> Result<Value, String> {
        let text_v = self.lower_expr(text)?;
        let host_ref = self.module.declare_func_in_func(parse_host, self.b.func);
        let parse_call = self.b.ins().call(host_ref, &[text_v]);
        let parsed = self.b.inst_results(parse_call)[0];
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[parsed]);
        let is_ok = self.b.inst_results(status_call)[0];
        let ok_block = self.b.create_block();
        let err_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        self.b.ins().brif(is_ok, ok_block, &[], err_block, &[]);

        self.b.switch_to_block(err_block);
        self.b.seal_block(err_block);
        // Propagate parse Err (string bits); `??` only needs the ok flag.
        self.b.ins().jump(merge, &[parsed]);

        self.b.switch_to_block(ok_block);
        self.b.seal_block(ok_block);
        let tree = self.result_payload(parsed, &Type::Named("DataTree".into()))?;
        let decoded = self.lower_datatree_decode_migrating(tree, ok_ty)?;
        self.b.ins().jump(merge, &[decoded]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_typed_json_to_string(
        &mut self,
        arg: &TExpr,
        pretty: bool,
    ) -> Result<Value, String> {
        let host_id = if pretty {
            self.host.encoding.json_to_string_pretty
        } else {
            self.host.encoding.json_to_string
        };
        self.lower_typed_tree_to_string(arg, host_id)
    }

    fn lower_typed_tree_to_string(
        &mut self,
        arg: &TExpr,
        host_id: FuncId,
    ) -> Result<Value, String> {
        let tree = self.lower_serde_encode(arg)?;
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        let call = self.b.ins().call(host_ref, &[tree]);
        Ok(self.b.inst_results(call)[0])
    }

    fn lower_typed_csv_decode(
        &mut self,
        text: &TExpr,
        elem_ty: &Type,
    ) -> Result<Value, String> {
        let text_v = self.lower_expr(text)?;
        let host_ref = self
            .module
            .declare_func_in_func(self.host.encoding.csv_decode_trees, self.b.func);
        let trees_call = self.b.ins().call(host_ref, &[text_v]);
        let trees_r = self.b.inst_results(trees_call)[0];
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[trees_r]);
        let is_ok = self.b.inst_results(status_call)[0];
        let ok_block = self.b.create_block();
        let err_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        self.b.ins().brif(is_ok, ok_block, &[], err_block, &[]);

        self.b.switch_to_block(err_block);
        self.b.seal_block(err_block);
        self.b.ins().jump(merge, &[trees_r]);

        self.b.switch_to_block(ok_block);
        self.b.seal_block(ok_block);
        let list = self.result_payload(
            trees_r,
            &Type::List(Box::new(Type::Named("DataTree".into()))),
        )?;
        let decoded = self.lower_datatree_decode_list_items(list, elem_ty)?;
        self.b.ins().jump(merge, &[decoded]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    /// Erase `#Numeric` / unit-family distinct wrappers to Int/Float for arith,
    /// print, and string interp — values already live as the base ABI.
    fn erase_distinct_ty(&self, ty: &Type) -> Type {
        let mut ty = ty
            .quantity_parts()
            .map_or_else(|| ty.clone(), |(base, _)| base.clone());
        while let Type::Named(name) = &ty {
            match self.meta.distinct_base(name) {
                Some(base) => ty = base.clone(),
                None => break,
            }
        }
        while let Type::Tagged { inner, .. } = ty {
            ty = *inner;
        }
        ty
    }

    fn lower_try(
        &mut self,
        inner: &TExpr,
        convert: &TIR::TTryConvert,
        file: &str,
        line: usize,
        fn_name: &str,
    ) -> Result<Value, String> {
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
        {
            let file_h = self.runtime.heap.alloc_string(Self::strip_rust_str_lit(file));
            let fn_h = self.runtime.heap.alloc_string(Self::strip_rust_str_lit(fn_name));
            let file_v = self.b.ins().iconst(types::I64, file_h);
            let line_v = self.b.ins().iconst(types::I64, line as i64);
            let fn_v = self.b.ins().iconst(types::I64, fn_h);
            let trace = self
                .module
                .declare_func_in_func(self.host.trace_err, self.b.func);
            self.b.ins().call(trace, &[file_v, line_v, fn_v]);
        }
        let return_handle = match convert {
            TIR::TTryConvert::None => handle,
            TIR::TTryConvert::Typed(conv_fn) => {
                let err_ty = inner
                    .ty
                    .unwrap_result()
                    .map(|(_, err)| err.clone())
                    .ok_or("jit try Typed operand is not Result")?;
                let err_payload = self.result_payload(handle, &err_ty)?;
                let func_id = self
                    .func_ids
                    .get(conv_fn)
                    .copied()
                    .ok_or_else(|| format!("jit typed Result conversion unknown `{conv_fn}`"))?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &[err_payload]);
                let converted = self.b.inst_results(call)[0];
                let tag = self.b.ins().iconst(types::I8, 0);
                let host = self
                    .module
                    .declare_func_in_func(self.host.result_new_i64, self.b.func);
                let pack = self.b.ins().call(host, &[tag, converted]);
                self.b.inst_results(pack)[0]
            }
            TIR::TTryConvert::Fallible | TIR::TTryConvert::WidenUnion { .. } => {
                return Err("jit typed Result conversion unsupported".to_string());
            }
        };
        while !self.txn_stack.is_empty() {
            // `?` early-return: emit restores but keep the stack for sibling paths.
            self.emit_txn_rollbacks_keep()?;
            break;
        }
        self.emit_shared_transaction_aborts_to(0);
        self.emit_shield_leaves_to(0);
        self.emit_scope_guards()?;
        self.b.ins().return_(&[return_handle]);
        self.b.switch_to_block(ok_block);
        self.b.seal_block(ok_block);
        let ok_ty = inner
            .ty
            .unwrap_result()
            .map(|(ok, _)| ok.clone())
            .or_else(|| Self::result_ok_ty_recover(inner))
            .ok_or("jit try operand is not Result")?;
        self.result_payload(handle, &ok_ty)
    }

    fn strip_rust_str_lit(s: &str) -> String {
        let s = s.strip_prefix('"').unwrap_or(s);
        let s = s.strip_suffix('"').unwrap_or(s);
        s.replace("\\\"", "\"").replace("\\\\", "\\")
    }

    pub(crate) fn emit_scope_guards(&mut self) -> Result<(), String> {
        // Keep the stack: early returns and sibling exit paths each need the
        // same LIFO cleanup (D-DEFER1). Cleared when the function finishes.
        let guards: Vec<FuncId> = self.scope_guards.iter().rev().copied().collect();
        for id in guards {
            let func_ref = self.module.declare_func_in_func(id, self.b.func);
            self.b.ins().call(func_ref, &[]);
            self.emit_trap_check()?;
        }
        Ok(())
    }

    fn emit_txn_commit_hooks(&mut self) -> Result<(), String> {
        let Some(frame) = self.txn_stack.last_mut() else {
            return Ok(());
        };
        let hooks: Vec<_> = frame.on_commit.drain(..).rev().collect();
        for id in hooks {
            let func_ref = self.module.declare_func_in_func(id, self.b.func);
            self.b.ins().call(func_ref, &[]);
            self.emit_trap_check()?;
        }
        Ok(())
    }

    /// Emit restore + rollback-hook calls for every open transaction without
    /// mutating the compile-time `txn_stack`. Required because a `return` inside
    /// one `if` branch must not pop frames that sibling / fallthrough paths still
    /// need for commit hooks.
    fn emit_txn_rollbacks_keep(&mut self) -> Result<(), String> {
        // Snapshot the restore plan so emit_trap_check can mutably borrow self.
        let frames: Vec<(Vec<(String, Value, TxnSnap)>, Vec<FuncId>)> = self
            .txn_stack
            .iter()
            .rev()
            .map(|frame| (frame.snapshots.clone(), frame.on_rollback.clone()))
            .collect();
        for (snapshots, hooks) in frames {
            for (place, snap, kind) in snapshots.into_iter().rev() {
                match kind {
                    TxnSnap::Rollback(type_name) => {
                        let restore_id = self
                            .func_ids
                            .get(&format!("{type_name}::restore"))
                            .copied()
                            .ok_or_else(|| {
                                format!("jit transaction restore missing `{type_name}::restore`")
                            })?;
                        let var = self.vars.get(&place).copied().ok_or_else(|| {
                            format!("jit transaction restore unknown `{place}`")
                        })?;
                        let current = self.b.use_var(var);
                        let restore_ref =
                            self.module.declare_func_in_func(restore_id, self.b.func);
                        self.b.ins().call(restore_ref, &[current, snap]);
                    }
                    TxnSnap::ScalarMut => {
                        let var = self.vars.get(&place).copied().ok_or_else(|| {
                            format!("jit transaction restore unknown `{place}`")
                        })?;
                        let ptr = self.b.use_var(var);
                        self.b.ins().store(MemFlags::trusted(), snap, ptr, 0);
                    }
                    TxnSnap::Plain => {
                        if let Some(var) = self.vars.get(&place).copied() {
                            self.b.def_var(var, snap);
                        }
                    }
                }
            }
            for id in hooks.into_iter().rev() {
                let func_ref = self.module.declare_func_in_func(id, self.b.func);
                self.b.ins().call(func_ref, &[]);
                self.emit_trap_check()?;
            }
        }
        Ok(())
    }

    fn emit_txn_rollback(&mut self) -> Result<(), String> {
        self.emit_txn_rollbacks_keep()?;
        self.txn_stack.clear();
        Ok(())
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

    fn emit_loop_fallback(
        &mut self,
        label: Option<&str>,
        kind: &str,
        is_continue: bool,
    ) -> Result<(), String> {
        let targets = self.loop_targets(label, kind)?;
        let destination = if is_continue {
            targets.continue_block
        } else {
            targets.break_block
        };
        if let Some(status) = self.emit_shield_leaves_to(targets.shield_depth) {
            let zero = self.b.ins().iconst(types::I64, 0);
            let pending = self.b.ins().icmp(IntCC::NotEqual, status, zero);
            let interrupted = self.b.create_block();
            self.b
                .ins()
                .brif(pending, interrupted, &[], destination, &[]);
            self.b.switch_to_block(interrupted);
            self.b.seal_block(interrupted);
            self.emit_shared_transaction_aborts_to(0);
            self.emit_dummy_return();
        } else {
            self.emit_shared_transaction_aborts_to(targets.shared_transaction_depth);
            self.b.ins().jump(destination, &[]);
        }
        Ok(())
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
        self.emit_shared_transaction_aborts_to(0);
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
        let push = self
            .module
            .declare_func_in_func(self.host.watcher.event_scope_frame_push, self.b.func);
        self.b.ins().call(push, &[]);
        self.lower_stmts(stmts)?;
        if !self.dead {
            let pop = self
                .module
                .declare_func_in_func(self.host.watcher.event_scope_frame_pop, self.b.func);
            self.b.ins().call(pop, &[]);
        }
        Ok(())
    }

    fn lower_watch_method(
        &mut self,
        recv: &TExpr,
        recv_val: Value,
        method: &str,
        callback_index: Option<usize>,
        args: &[TExpr],
    ) -> Result<Value, String> {
        let is_set = matches!(&recv.ty, Type::Named(n) if n == "WatchSet");
        match method {
            "poll" | "events" if args.is_empty() => {
                let host_id = if is_set {
                    self.host.watcher.watchset_poll
                } else {
                    self.host.watcher.watch_poll
                };
                let host = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            "cancel" if args.is_empty() && !is_set => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.watcher.watch_cancel, self.b.func);
                self.b.ins().call(host, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            "is_active" if args.is_empty() && !is_set => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.watcher.watch_is_active, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            "summary" if args.is_empty() => {
                let host_id = if is_set {
                    self.host.watcher.watchset_summary
                } else {
                    self.host.watcher.watch_summary
                };
                let host = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            "add" if args.len() == 1 && is_set => {
                let other = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.watcher.watchset_add, self.b.func);
                self.b.ins().call(host, &[recv_val, other]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            "on" | "once" if args.len() == 2 && !is_set => {
                let Some(idx) = callback_index else {
                    return Err("jit watch callback missing spawn index".to_string());
                };
                let scope = self.lower_expr(&args[0])?;
                let lam = self
                    .spawn_lambdas
                    .get(idx)
                    .ok_or_else(|| format!("jit watch callback site {idx} missing lambda"))?;
                let spawn_fn = self
                    .spawn_func_ids
                    .get(idx)
                    .copied()
                    .ok_or_else(|| format!("jit watch callback site {idx} missing"))?;
                let mut cap_vals = Vec::new();
                for cap in &lam.captures {
                    let captured = TExpr {
                        ty: cap.ty.clone(),
                        kind: TExprKind::Local(TLocal::user(&cap.name)),
                    };
                    let val = if cap.clone_at_spawn {
                        self.lower_clone(&captured)?
                    } else {
                        self.lower_expr(&captured)?
                    };
                    cap_vals.push(val);
                }
                if cap_vals.len() > 4 {
                    return Err(format!(
                        "jit watch callback capture count {} > 4",
                        cap_vals.len()
                    ));
                }
                let spawn_ref = self.module.declare_func_in_func(spawn_fn, self.b.func);
                let fn_ptr = self.b.ins().func_addr(types::I64, spawn_ref);
                let n_caps = self.b.ins().iconst(types::I64, cap_vals.len() as i64);
                let zero = self.b.ins().iconst(types::I64, 0);
                let mut caps = [zero, zero, zero, zero];
                for (i, v) in cap_vals.into_iter().enumerate() {
                    caps[i] = v;
                }
                let host_id = if method == "once" {
                    self.host.watcher.watch_once
                } else {
                    self.host.watcher.watch_on
                };
                let host = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(
                    host,
                    &[
                        recv_val, scope, fn_ptr, n_caps, caps[0], caps[1], caps[2], caps[3],
                    ],
                );
                Ok(self.b.inst_results(call)[0])
            }
            _ => Err(format!("jit watch method unsupported: {method}")),
        }
    }

    fn lower_inline_block(&mut self, stmts: &[TStmt], ty: &Type) -> Result<Value, String> {
        let saved_vars = self.vars.clone();
        let saved_var_tys = self.var_tys.clone();
        let result = (|| {
            let (tail, prefix) = stmts
                .split_last()
                .ok_or("jit inline loop block has no result")?;
            self.lower_stmts(prefix)?;
            match tail {
                TStmt::ExprStmt(value) => self.lower_expr(value),
                TStmt::Loop { label, body } => self.lower_result_loop(label, body, ty),
                _ => Err("jit inline loop block has unsupported result statement".to_string()),
            }
        })();
        *self.vars = saved_vars;
        *self.var_tys = saved_var_tys;
        result
    }

    fn lower_result_loop(
        &mut self,
        label: &Option<String>,
        body: &[TStmt],
        ty: &Type,
    ) -> Result<Value, String> {
        let result_ty = self
            .meta
            .clif_ty(ty)
            .ok_or_else(|| format!("jit result-loop type unsupported: {ty:?}"))?;
        let header = self.b.create_block();
        let body_block = self.b.create_block();
        let exit = self.b.create_block();
        self.b.append_block_param(exit, result_ty);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        self.b.ins().jump(body_block, &[]);

        self.loop_stack.push(LoopTargets {
            label: label.clone(),
            continue_block: header,
            break_block: exit,
            break_value_ty: Some(ty.clone()),
            shield_depth: self.shield_depth,
            shared_transaction_depth: self.shared_transaction_depth,
        });
        self.b.switch_to_block(body_block);
        self.b.seal_block(body_block);
        self.lower_stmts_scoped(body)?;
        self.loop_stack.pop();
        if !self.dead {
            self.b.ins().jump(header, &[]);
        }
        self.b.seal_block(header);

        self.b.switch_to_block(exit);
        self.b.seal_block(exit);
        self.dead = false;
        Ok(self.b.block_params(exit)[0])
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
                // Mutable field place (`left :: &counters.left`) → write-through
                // handle `[struct, field_idx]` so later `left = 3` updates the owner.
                if let TExprKind::Borrow {
                    place,
                    mutable: true,
                } = &init.kind
                {
                    if let TExprKind::Field {
                        recv,
                        field,
                        boxed: false,
                    } = &place.kind
                    {
                        let structure = self.lower_expr(recv)?;
                        let type_name = record_type_key(&recv.ty).ok_or_else(|| {
                            format!("jit field-mut recv type unsupported: {:?}", recv.ty)
                        })?;
                        let index = self
                            .meta
                            .struct_field_index(&type_name, field)
                            .ok_or_else(|| format!("jit field `{field}` on `{type_name}`"))?;
                        let handle = self.emit_field_mut(structure, index as i64)?;
                        let ty = Type::Apply {
                            name: "__JetFieldMut".to_string(),
                            args: vec![init.ty.clone()],
                        };
                        let clif = types::I64;
                        let var = self.fresh_var(clif);
                        self.b.def_var(var, handle);
                        self.vars.insert(TIR::local_place(name), var);
                        self.var_tys.insert(TIR::local_place(name), ty);
                        return Ok(());
                    }
                }
                let val = self.lower_expr(init)?;
                // TIR often stamps `Unit` on void calls and some handle results;
                // prefer the Cranelift value's real ABI over guessing I8 vs I64.
                let ty = if matches!(&init.ty, Type::Named(n) if n == "Unit" || n == "Void") {
                    self.b.func.dfg.value_type(val)
                } else {
                    init_clif_ty(init, self.meta)?
                };
                let val_ty = self.b.func.dfg.value_type(val);
                if val_ty != ty {
                    return Err(format!("jit let `{name}` lowering type mismatch"));
                }
                let var = self.fresh_var(ty);
                self.b.def_var(var, val);
                self.vars.insert(TIR::local_place(name), var);
                let stored_ty = if matches!(&init.ty, Type::Named(n) if n == "Unit" || n == "Void")
                {
                    Self::recover_core_return_ty(init).unwrap_or_else(|| match ty {
                        types::I8 => Type::Bool,
                        types::F64 => Type::Float,
                        types::I32 => Type::Char,
                        _ => Type::Int,
                    })
                } else {
                    init.ty.clone()
                };
                self.var_tys.insert(TIR::local_place(name), stored_ty);
            }
            TStmt::SplitViews {
                owner,
                root,
                name,
                start,
                end,
                single,
                write,
                elem_ty: split_elem_ty,
                line,
                ..
            } => {
                // Disjoint borrow splitting is an AOT lifetime fact. JIT keeps one
                // shared list handle; write windows are `[list,start,end]` records
                // (same ABI as ViewMutNew) so IndexAssign / deref Assign write through.
                if let Some(owner_expr) = owner {
                    let owner_ty = owner_expr.ty.clone();
                    let val = self.lower_expr(owner_expr)?;
                    let var = self.fresh_var(types::I64);
                    self.b.def_var(var, val);
                    self.vars.insert(root.clone(), var);
                    self.var_tys.insert(root.clone(), owner_ty);
                }
                let list_var = *self
                    .vars
                    .get(root)
                    .ok_or_else(|| format!("jit split views missing owner `{root}`"))?;
                let owner_elem_ty = match self.var_tys.get(root) {
                    Some(Type::List(elem) | Type::FixedList { elem, .. }) => {
                        elem.as_ref().clone()
                    }
                    other => {
                        return Err(format!(
                            "jit split views owner type unsupported: {other:?}"
                        ));
                    }
                };
                let elem_ty = split_elem_ty
                    .as_ref()
                    .filter(|ty| *ty == &owner_elem_ty)
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "jit split views element type mismatch: owner={owner_elem_ty:?}, split={split_elem_ty:?}"
                        )
                    })?;
                let list = self.b.use_var(list_var);
                let start_v = self.b.ins().iconst(types::I64, *start);
                let end_v = self.b.ins().iconst(types::I64, *end);
                let line_c = self.b.ins().iconst(types::I32, *line as i64);
                let (bound, bound_ty) = if *write {
                    if *single {
                        let host_id = match &elem_ty {
                            Type::Float => self.host.coll.list_get_f64,
                            _ => self.host.coll.list_get,
                        };
                        let get_ref = self
                            .module
                            .declare_func_in_func(host_id, self.b.func);
                        self.b.ins().call(get_ref, &[list, start_v, line_c]);
                        self.emit_trap_check()?;
                    }
                    let handle = self.emit_view_mut_window(list, start_v, end_v)?;
                    (
                        handle,
                        Type::Apply {
                            name: "ViewMut".to_string(),
                            args: vec![elem_ty.clone()],
                        },
                    )
                } else if *single {
                    let host_id = match &elem_ty {
                        Type::Float => self.host.coll.list_get_f64,
                        _ => self.host.coll.list_get,
                    };
                    let get_ref = self
                        .module
                        .declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(get_ref, &[list, start_v, line_c]);
                    let result = self.b.inst_results(call)[0];
                    self.emit_trap_check()?;
                    (result, elem_ty.clone())
                } else {
                    let end_excl = self.b.ins().iconst(types::I64, *end + 1);
                    let slice_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.list_slice, self.b.func);
                    let call = self
                        .b
                        .ins()
                        .call(slice_ref, &[list, start_v, end_excl, line_c]);
                    let result = self.b.inst_results(call)[0];
                    self.emit_trap_check()?;
                    (
                        result,
                        Type::Apply {
                            name: "View".to_string(),
                            args: vec![elem_ty],
                        },
                    )
                };
                let place = TIR::local_place(name);
                let clif = if *write || !*single {
                    types::I64
                } else {
                    clif_ty(&bound_ty).ok_or_else(|| {
                        format!("jit split views element type unsupported: {bound_ty:?}")
                    })?
                };
                let var = self.fresh_var(clif);
                self.b.def_var(var, bound);
                self.vars.insert(place.clone(), var);
                self.var_tys.insert(place, bound_ty);
            }
            // D-TUPLE-DESTRUCT1: `(tx, rx) := tasks.channel<T>()` / `tasks.channel<T>(n)`.
            TStmt::TupleDestructure { init, binds, .. } => {
                if let TExprKind::CoreCall {
                    module,
                    method,
                    args,
                    ..
                } = &init.kind
                {
                    if module == "core.tasks" && method == "channel" {
                        let ch_val = if args.is_empty() {
                            let ch_ref = self
                                .module
                                .declare_func_in_func(self.host.conc.channel_new, self.b.func);
                            let ch_call = self.b.ins().call(ch_ref, &[]);
                            self.b.inst_results(ch_call)[0]
                        } else if args.len() == 1 {
                            let cap = self.lower_expr(&args[0])?;
                            let ch_ref = self
                                .module
                                .declare_func_in_func(self.host.conc.channel_bounded, self.b.func);
                            let ch_call = self.b.ins().call(ch_ref, &[cap]);
                            self.b.inst_results(ch_call)[0]
                        } else {
                            return Err("jit tasks.channel arity unsupported".to_string());
                        };
                        let tx_ref = self
                            .module
                            .declare_func_in_func(self.host.conc.channel_sender, self.b.func);
                        let tx_call = self.b.ins().call(tx_ref, &[ch_val]);
                        let tx_val = self.b.inst_results(tx_call)[0];
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
                } else {
                    self.lower_tuple_destructure(init, binds)?;
                }
            }
            TStmt::Assign {
                place, op, value, ..
            } => {
                if let TPlace::Expr(place_expr) = place {
                    if let TExprKind::PoolSlot {
                        pool,
                        id,
                        field: Some(field),
                        ..
                    } = &place_expr.kind
                    {
                        let pool_handle = self.lower_expr(pool)?;
                        let id_value = self.lower_expr(id)?;
                        let getter = self
                            .module
                            .declare_func_in_func(self.host.memory.pool_get, self.b.func);
                        let get_call = self.b.ins().call(getter, &[pool_handle, id_value]);
                        let record = self.b.inst_results(get_call)[0];
                        self.emit_trap_check()?;
                        let elem_ty = match &pool.ty {
                            Type::Apply { args, .. } if !args.is_empty() => &args[0],
                            other => return Err(format!("jit pool type unsupported: {other:?}")),
                        };
                        let type_name = record_type_key(elem_ty)
                            .ok_or_else(|| format!("jit pool element type: {elem_ty:?}"))?;
                        let field_index = self
                            .meta
                            .struct_field_index(&type_name, field)
                            .ok_or_else(|| format!("jit field `{field}` on `{type_name}`"))?;
                        let field_ty = place_expr.ty.clone();
                        let rhs = self.lower_expr(value)?;
                        let assigned = if let Some(op) = op {
                            let current =
                                self.lower_record_field(record, &type_name, field, &field_ty)?;
                            self.apply_binop_to_var(current, *op, rhs, &field_ty)?
                        } else {
                            rhs
                        };
                        let setter_id = match &field_ty {
                            Type::Int => self.host.struct_set_i64,
                            Type::Float => self.host.struct_set_f64,
                            Type::Bool => self.host.struct_set_bool,
                            Type::Char => self.host.struct_set_char,
                            Type::String => self.host.struct_set_str,
                            other if clif_ty(other) == Some(types::I64) => {
                                self.host.struct_set_i64
                            }
                            other => {
                                return Err(format!(
                                    "jit pool field assignment type unsupported: {other:?}"
                                ))
                            }
                        };
                        let field_index = self.b.ins().iconst(types::I64, field_index as i64);
                        let setter = self.module.declare_func_in_func(setter_id, self.b.func);
                        self.b.ins().call(setter, &[record, field_index, assigned]);
                        return Ok(());
                    }
                }
                if let Some((base, field)) = structured_record_field_place(place) {
                    let key = Self::local_key(base);
                    if let (Some(var), Some(base_ty)) = (
                        self.vars.get(&key).copied(),
                        self.var_tys.get(&key).cloned(),
                    ) {
                        let mut handle = self.b.use_var(var);
                        let record_ty = match &base_ty {
                            Type::Apply { name, args }
                                if name == "ViewMut" && args.len() == 1 =>
                            {
                                let (list, start, _) = self.unpack_view_mut(handle)?;
                                let line = self.b.ins().iconst(types::I32, 1);
                                let get = self
                                    .module
                                    .declare_func_in_func(self.host.coll.list_get, self.b.func);
                                let call = self.b.ins().call(get, &[list, start, line]);
                                handle = self.b.inst_results(call)[0];
                                self.emit_trap_check()?;
                                args[0].clone()
                            }
                            other => other.clone(),
                        };
                        let type_name = record_type_key(&record_ty)
                            .ok_or_else(|| format!("jit field assign recv type: {record_ty:?}"))?;
                        let index = self
                            .meta
                            .struct_field_index(&type_name, field)
                            .ok_or_else(|| format!("jit field `{field}` on `{type_name}`"))?;
                        let field_ty = value.ty.clone();
                        let rhs = self.lower_expr(value)?;
                        let assigned = if let Some(op) = op {
                            let current =
                                self.lower_record_field(handle, &type_name, field, &field_ty)?;
                            self.apply_binop_to_var(current, *op, rhs, &field_ty)?
                        } else {
                            rhs
                        };
                        let host_id = match &field_ty {
                            Type::Int => self.host.struct_set_i64,
                            Type::Float => self.host.struct_set_f64,
                            Type::Bool => self.host.struct_set_bool,
                            Type::Char => self.host.struct_set_char,
                            Type::String => self.host.struct_set_str,
                            other if clif_ty(other) == Some(types::I64) => {
                                self.host.struct_set_i64
                            }
                            other => {
                                return Err(format!(
                                    "jit field assignment type unsupported: {other:?}"
                                ));
                            }
                        };
                        let index = self.b.ins().iconst(types::I64, index as i64);
                        let setter = self.module.declare_func_in_func(host_id, self.b.func);
                        self.b.ins().call(setter, &[handle, index, assigned]);
                        return Ok(());
                    }
                }
                if let TPlace::Expr(place_expr) = place {
                    if let TExprKind::Field { recv, field, .. } = &place_expr.kind {
                        if matches!(&recv.kind, TExprKind::PoolSlot { .. }) {
                            let handle = self.lower_expr(recv)?;
                            let type_name = record_type_key(&recv.ty)
                                .ok_or_else(|| format!("jit pool field recv type: {:?}", recv.ty))?;
                            let index = self
                                .meta
                                .struct_field_index(&type_name, field)
                                .ok_or_else(|| format!("jit field `{field}` on `{type_name}`"))?;
                            let field_ty = value.ty.clone();
                            let rhs = self.lower_expr(value)?;
                            let assigned = if let Some(op) = op {
                                let current = self.lower_record_field(
                                    handle,
                                    &type_name,
                                    field,
                                    &field_ty,
                                )?;
                                self.apply_binop_to_var(current, *op, rhs, &field_ty)?
                            } else {
                                rhs
                            };
                            let host_id = match &field_ty {
                                Type::Int => self.host.struct_set_i64,
                                Type::Float => self.host.struct_set_f64,
                                Type::Bool => self.host.struct_set_bool,
                                Type::Char => self.host.struct_set_char,
                                Type::String => self.host.struct_set_str,
                                other if clif_ty(other) == Some(types::I64) => {
                                    self.host.struct_set_i64
                                }
                                other => {
                                    return Err(format!(
                                        "jit pool field assignment type unsupported: {other:?}"
                                    ))
                                }
                            };
                            let index = self.b.ins().iconst(types::I64, index as i64);
                            let setter =
                                self.module.declare_func_in_func(host_id, self.b.func);
                            self.b.ins().call(setter, &[handle, index, assigned]);
                            return Ok(());
                        }
                    }
                }
                let local = place.as_local().ok_or("jit assign to non-local place")?;
                let key = Self::local_key(local);
                let var = self
                    .vars
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit assign to unknown place `{}`", local.name))?;
                // D-MUTSELF1: `(*self) = New{…}` must write through the receiver
                // handle, not replace the local SSA pointer (AOT: `(*self) = …`).
                // ViewMut / field-mut places use the same deref bit for element/
                // field write-through.
                if local.deref {
                    let dst = self.b.use_var(var);
                    if matches!(
                        self.var_tys.get(&key),
                        Some(Type::Apply { name, .. })
                            if name == jet_foundation::Syntax::TYPE_PTR
                    ) {
                        let rhs = self.lower_expr(value)?;
                        self.b.ins().store(MemFlags::trusted(), rhs, dst, 0);
                        return Ok(());
                    }
                    let view_elem_ty = match self.var_tys.get(&key) {
                        Some(Type::Apply { name, args })
                            if name == "ViewMut" && args.len() == 1 =>
                        {
                            Some(args[0].clone())
                        }
                        _ => None,
                    };
                    if let Some(elem_ty) = view_elem_ty {
                        let direct_value = if op.is_none() {
                            Some(self.lower_expr(value)?)
                        } else {
                            None
                        };
                        let (list, start, _) = self.unpack_view_mut(dst)?;
                        let line = self.b.ins().iconst(types::I32, 1);
                        let assigned = if let Some(op) = op {
                            let get_id = match &elem_ty {
                                Type::Float => self.host.coll.list_get_f64,
                                _ => self.host.coll.list_get,
                            };
                            let get = self.module.declare_func_in_func(get_id, self.b.func);
                            let call = self.b.ins().call(get, &[list, start, line]);
                            let current = self.b.inst_results(call)[0];
                            self.emit_trap_check()?;
                            let rhs = self.lower_expr(value)?;
                            self.apply_binop_to_var(current, *op, rhs, &elem_ty)?
                        } else {
                            direct_value
                                .ok_or("jit direct ViewMut assignment missing value")?
                        };
                        let set_id = match &elem_ty {
                            Type::Float => self.host.coll.list_set_f64,
                            _ => self.host.coll.list_set,
                        };
                        let set = self.module.declare_func_in_func(set_id, self.b.func);
                        self.b.ins().call(set, &[list, start, assigned, line]);
                        self.emit_trap_check()?;
                        return Ok(());
                    }
                    if let Some(Type::Apply { name, args }) = self.var_tys.get(&key) {
                        if name == "__JetScalarMut" && args.len() == 1 {
                            let scalar_ty = args[0].clone();
                            let clif = self
                                .meta
                                .clif_ty(&scalar_ty)
                                .ok_or("jit writable scalar type")?;
                            let current = self.b.ins().load(
                                clif,
                                MemFlags::trusted(),
                                dst,
                                0,
                            );
                            let rhs = self.lower_expr(value)?;
                            let assigned = if let Some(op) = op {
                                self.apply_binop_to_var(current, *op, rhs, &scalar_ty)?
                            } else {
                                rhs
                            };
                            self.b.ins().store(MemFlags::trusted(), assigned, dst, 0);
                            return Ok(());
                        }
                    }
                    if op.is_none() {
                        let src = self.lower_expr(value)?;
                        if self.var_tys.get(&key).is_some_and(Self::is_field_mut_ty) {
                            let (structure, idx) = self.unpack_field_mut(dst)?;
                            let set = self
                                .module
                                .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                            self.b.ins().call(set, &[structure, idx, src]);
                            return Ok(());
                        }
                        let assign = self
                            .module
                            .declare_func_in_func(self.host.struct_assign, self.b.func);
                        self.b.ins().call(assign, &[dst, src]);
                        return Ok(());
                    }
                }
                let val = if let Some(op) = op {
                    let current = if let Some(slot) = self.raw_slots.get(&key).copied() {
                        let ty = self
                            .meta
                            .clif_ty(self.var_tys.get(&key).unwrap_or(&value.ty))
                            .ok_or("jit spilled local type")?;
                        self.b.ins().stack_load(ty, slot, 0)
                    } else {
                        self.b.use_var(var)
                    };
                    let rhs = self.lower_expr(value)?;
                    let arithmetic_ty = self
                        .var_tys
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| value.ty.clone());
                    self.apply_binop_to_var(current, *op, rhs, &arithmetic_ty)?
                } else {
                    self.lower_expr(value)?
                };
                if let Some(slot) = self.raw_slots.get(&key).copied() {
                    self.b.ins().stack_store(val, slot, 0);
                } else {
                    self.b.def_var(var, val);
                }
            }
            TStmt::IndexFieldAssign(assign) => {
                if assign.is_map {
                    return Err("jit indexed map field assignment uses AOT fallback".to_string());
                }
                // Match AOT evaluation order: the assignment value is evaluated
                // before the collection place is acquired.
                let rhs = self.lower_expr(&assign.value)?;
                let list = self.lower_expr(&assign.base)?;
                let index = self.lower_expr(&assign.index)?;
                let line = self.b.ins().iconst(types::I32, assign.line as i64);
                let get_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_get, self.b.func);
                let get_call = self.b.ins().call(get_ref, &[list, index, line]);
                let handle = self.b.inst_results(get_call)[0];
                self.emit_trap_check()?;

                let elem_ty = match &assign.base.ty {
                    Type::List(elem) | Type::FixedList { elem, .. } => elem.as_ref(),
                    other => {
                        return Err(format!(
                            "jit indexed field assignment collection type unsupported: {other:?}"
                        ));
                    }
                };
                let type_name = record_type_key(elem_ty).ok_or_else(|| {
                    format!("jit indexed field assignment element type: {elem_ty:?}")
                })?;
                let field_index = self
                    .meta
                    .struct_field_index(&type_name, &assign.field)
                    .ok_or_else(|| {
                        format!(
                            "jit field `{}` on `{type_name}`",
                            assign.field
                        )
                    })?;
                let value = if let Some(op) = assign.op {
                    let current = self.lower_record_field(
                        handle,
                        &type_name,
                        &assign.field,
                        &assign.field_ty,
                    )?;
                    self.apply_binop_to_var(current, op, rhs, &assign.field_ty)?
                } else {
                    rhs
                };
                let setter = match &assign.field_ty {
                    Type::Int => self.host.struct_set_i64,
                    Type::Float => self.host.struct_set_f64,
                    Type::Bool => self.host.struct_set_bool,
                    Type::Char => self.host.struct_set_char,
                    Type::String => self.host.struct_set_str,
                    other if clif_ty(other) == Some(types::I64) => self.host.struct_set_i64,
                    other => {
                        return Err(format!(
                            "jit indexed field assignment type unsupported: {other:?}"
                        ));
                    }
                };
                let field_index = self.b.ins().iconst(types::I64, field_index as i64);
                let setter = self.module.declare_func_in_func(setter, self.b.func);
                self.b.ins().call(setter, &[handle, field_index, value]);
            }
            TStmt::Return(Some(expr)) => {
                let val = self.lower_expr(expr)?;
                // Keep compile-time txn_stack: a `return` in one `if` branch must
                // not pop frames needed by the fallthrough / sibling path.
                self.emit_txn_rollbacks_keep()?;
                self.emit_shared_transaction_aborts_to(0);
                self.emit_shield_leaves_to(0);
                self.emit_scope_guards()?;
                if let Some(sender) = self.yield_sender {
                    let close = self
                        .module
                        .declare_func_in_func(self.host.conc.sender_close, self.b.func);
                    self.b.ins().call(close, &[sender]);
                }
                self.b.ins().return_(&[val]);
                self.dead = true;
            }
            TStmt::Return(None) => {
                self.emit_txn_rollbacks_keep()?;
                self.emit_shared_transaction_aborts_to(0);
                self.emit_shield_leaves_to(0);
                self.emit_scope_guards()?;
                if let Some(sender) = self.yield_sender {
                    let close = self
                        .module
                        .declare_func_in_func(self.host.conc.sender_close, self.b.func);
                    self.b.ins().call(close, &[sender]);
                    let zero = self.b.ins().iconst(types::I64, 0);
                    self.b.ins().return_(&[zero]);
                } else {
                    self.b.ins().return_(&[]);
                }
                self.dead = true;
            }
            TStmt::ExprStmt(expr) => {
                self.lower_expr(expr)?;
            }
            TStmt::DeferClose { close, .. } => {
                // Run the same close expr AOT's Drop guard would.
                self.lower_expr(close)?;
            }
            TStmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                let cond_val = match cond {
                    TIfCond::Plain(e) => self.lower_expr(e)?,
                    TIfCond::IfLet { pattern, subj } => {
                        if matches!(&pattern.pattern, Pattern::Variant { .. })
                            && Self::is_datatree_value_ty(&subj.ty)
                        {
                            self.lower_datatree_if_let(
                                pattern,
                                subj,
                                then_body,
                                else_body.as_deref(),
                            )?;
                        } else if matches!(&pattern.pattern, Pattern::Variant { .. }) {
                            self.lower_enum_if_let(
                                pattern,
                                subj,
                                then_body,
                                else_body.as_deref(),
                            )?;
                        } else {
                            self.lower_result_if_let(
                                pattern,
                                subj,
                                then_body,
                                else_body.as_deref(),
                            )?;
                        }
                        return Ok(());
                    }
                    // IfLet/IsNone/Matches lower to a pre-computed `matches!`/`is_none`
                    // string in the AOT emitter (TIR/emit/statements.rs); the JIT has no
                    // pattern-test lowering, so each is named (not `_`) to keep this match
                    // exhaustive over `TIfCond`.
                    TIfCond::And { .. } => {
                        return Err("jit binding conjunction unsupported".to_string());
                    }
                    TIfCond::IsNone { subj } => {
                        // Option ABI: 0 = None, else bits+1. `x == .None` → IsNone.
                        // Mirror Plain `if`: skip merge jumps after break/return.
                        let packed = self.lower_expr(subj)?;
                        let zero = self.b.ins().iconst(types::I64, 0);
                        let is_none = self.b.ins().icmp(IntCC::Equal, packed, zero);
                        let then_block = self.b.create_block();
                        let else_block = self.b.create_block();
                        let merge_block = self.b.create_block();
                        self.b
                            .ins()
                            .brif(is_none, then_block, &[], else_block, &[]);
                        self.b.switch_to_block(then_block);
                        self.b.seal_block(then_block);
                        self.lower_stmts_scoped(then_body)?;
                        let then_reaches_merge = !self.dead;
                        if then_reaches_merge {
                            self.b.ins().jump(merge_block, &[]);
                        }
                        self.b.switch_to_block(else_block);
                        self.b.seal_block(else_block);
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
                            self.dead = true;
                        }
                        return Ok(());
                    }
                    TIfCond::Matches { pattern, subj } => {
                        let value = self.lower_expr(subj)?;
                        let enum_name = pattern
                            .enum_type
                            .as_deref()
                            .or_else(|| user_type_name(&subj.ty));
                        self.lower_pattern_condition(
                            value,
                            &pattern.pattern,
                            enum_name,
                            false,
                        )?
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
                self.b.ins().jump(body_block, &[]);

                self.loop_stack.push(LoopTargets {
                    label: label.clone(),
                    continue_block: header,
                    break_block: exit,
                    break_value_ty: None,
                    shield_depth: self.shield_depth,
                    shared_transaction_depth: self.shared_transaction_depth,
                });
                self.b.switch_to_block(body_block);
                self.b.seal_block(body_block);
                self.lower_stmts_scoped(body)?;
                self.loop_stack.pop();
                if !self.dead {
                    self.b.ins().jump(header, &[]);
                }
                self.b.seal_block(header);

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
                    break_value_ty: None,
                    shield_depth: self.shield_depth,
                    shared_transaction_depth: self.shared_transaction_depth,
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
                let step_block = self.b.create_block();
                let exit = self.b.create_block();
                self.b.ins().jump(header, &[]);

                self.b.switch_to_block(header);
                let cond_val = self.lower_expr(cond)?;
                self.b.ins().brif(cond_val, body_block, &[], exit, &[]);

                self.loop_stack.push(LoopTargets {
                    label: label.clone(),
                    continue_block: step_block,
                    break_block: exit,
                    break_value_ty: None,
                    shield_depth: self.shield_depth,
                    shared_transaction_depth: self.shared_transaction_depth,
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
                self.dead = false;
                if let Some(step) = step {
                    self.lower_stmt(step)?;
                }
                if !self.dead { self.b.ins().jump(header, &[]); }
                self.b.seal_block(header);

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
                exclusive,
                body,
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
                // Inclusive `..` stops after `end`; exclusive `..<` stops at `end`
                // (D-RANGE-EXCL1=C).
                let past_end = if *exclusive {
                    self.b
                        .ins()
                        .icmp(IntCC::SignedGreaterThanOrEqual, cur, end_val)
                } else {
                    self.b.ins().icmp(IntCC::SignedGreaterThan, cur, end_val)
                };
                self.b.ins().brif(past_end, exit, &[], body_block, &[]);

                self.loop_stack.push(LoopTargets {
                    label: label.clone(),
                    continue_block: step_block,
                    break_block: exit,
                    break_value_ty: None,
                    shield_depth: self.shield_depth,
                    shared_transaction_depth: self.shared_transaction_depth,
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
            TStmt::BreakValue { label, value } => {
                let targets = self.loop_targets(label.as_deref(), "break")?;
                let expected = targets
                    .break_value_ty
                    .as_ref()
                    .ok_or("jit break value targets an effect-only loop")?;
                if expected != &value.ty {
                    return Err(format!(
                        "jit break value type mismatch: expected {expected:?}, got {:?}",
                        value.ty
                    ));
                }
                let value = self.lower_expr(value)?;
                if let Some(status) = self.emit_shield_leaves_to(targets.shield_depth) {
                    let zero = self.b.ins().iconst(types::I64, 0);
                    let pending = self.b.ins().icmp(IntCC::NotEqual, status, zero);
                    let interrupted = self.b.create_block();
                    self.b.ins().brif(
                        pending,
                        interrupted,
                        &[],
                        targets.break_block,
                        &[value],
                    );
                    self.b.switch_to_block(interrupted);
                    self.b.seal_block(interrupted);
                    self.emit_dummy_return();
                } else {
                    self.b.ins().jump(targets.break_block, &[value]);
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
                    let map = self.lower_expr(base)?;
                    let key = self.lower_expr(index)?;
                    let val = self.lower_expr(value)?;
                    let val = match self.meta.clif_ty(&value.ty) {
                        Some(types::I32) => self.b.ins().uextend(types::I64, val),
                        Some(types::I8) => self.b.ins().uextend(types::I64, val),
                        Some(types::F64) => self.b.ins().bitcast(
                            types::I64,
                            Self::scalar_bitcast_memflags(),
                            val,
                        ),
                        _ => val,
                    };
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.map_insert, self.b.func);
                    self.b.ins().call(host_ref, &[map, key, val]);
                } else {
                    // ViewMut write-through: absolute index = window.start + idx.
                    if Self::is_view_mut_ty(&base.ty) {
                        let handle = self.lower_expr(base)?;
                        let (list, start, _) = self.unpack_view_mut(handle)?;
                        let idx = self.lower_expr(index)?;
                        let abs = self.b.ins().iadd(start, idx);
                        let val = self.lower_expr(value)?;
                        let line = self.b.ins().iconst(types::I32, 1);
                        let host_id = match &value.ty {
                            Type::Float => self.host.coll.list_set_f64,
                            _ => self.host.coll.list_set,
                        };
                        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                        self.b.ins().call(host_ref, &[list, abs, val, line]);
                        self.emit_trap_check()?;
                        return Ok(());
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
            }
            TStmt::ForIn {
                label,
                var,
                var2,
                source,
                collection,
                step,
                method_kind,
                columnar,
                by_value,
                body,
            } => {
                if matches!(
                    &collection.ty,
                    Type::Apply { name, .. } if name == "Stream"
                ) {
                    self.lower_stream_for_in(
                        label,
                        var,
                        source,
                        step.as_ref(),
                        body,
                        &collection.ty,
                    )?;
                    return Ok(());
                }
                if let Some(TForInMethod::Iterable {
                    coll_type,
                    iter_type,
                }) = method_kind
                {
                    self.lower_iterable_for_in(
                        label,
                        var,
                        source,
                        step.as_ref(),
                        body,
                        coll_type,
                        iter_type,
                    )?;
                    return Ok(());
                }
                if matches!(
                    method_kind,
                    Some(TForInMethod::LinesFile | TForInMethod::LinesStdin)
                ) {
                    // Fall through — materialize via host then walk as string list.
                } else if *columnar {
                    return Err("jit for-in columnar unsupported".to_string());
                }
                // `by_value` is set for Stream / Iter / HttpBodyChunks. JIT can
                // only walk Iter<T> because producers materialize list handles
                // (true JetIter cannot cross the host-shim ABI yet).
                if *by_value
                    && !jet_foundation::Collections::is_iter_type(&collection.ty)
                    && !matches!(
                        method_kind,
                        Some(
                            TForInMethod::LinesProcessStream
                                | TForInMethod::LinesFile
                                | TForInMethod::LinesStdin
                        )
                    )
                {
                    return Err("jit for-in by-value stream unsupported".to_string());
                }
                let stride = match step {
                    Some(step) => {
                        let stride = self.lower_expr(step)?;
                        let check_ref = self.module.declare_func_in_func(
                            self.host.coll.loop_stride_check,
                            self.b.func,
                        );
                        let checked = self.b.ins().call(check_ref, &[stride]);
                        let stride = self.b.inst_results(checked)[0];
                        self.emit_trap_check()?;
                        stride
                    }
                    None => self.b.ins().iconst(types::I64, 1),
                };
                if let Some(value_name) = var2 {
                    let map_pairs = matches!(
                        &collection.ty,
                        Type::Map { key, .. } if matches!(key.as_ref(), Type::String)
                    );
                    let list_elem_ty = jit_list_iter_elem_type(&collection.ty)
                        .or_else(|| jit_closure_elem_type(&collection.ty));
                    if !map_pairs && list_elem_ty.is_none() {
                        return Err("jit for-in map pairs unsupported".to_string());
                    }
                    let coll_val = self.lower_expr(source)?;
                    let coll_var = self.fresh_var(types::I64);
                    self.b.def_var(coll_var, coll_val);
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
                    let len_id = if map_pairs {
                        self.host.coll.map_len
                    } else {
                        self.host.coll.list_len
                    };
                    let len_ref = self.module.declare_func_in_func(len_id, self.b.func);
                    let len_call = self.b.ins().call(len_ref, &[coll]);
                    let len = self.b.inst_results(len_call)[0];
                    let done = self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
                    self.b.ins().brif(done, exit, &[], body_block, &[]);

                    self.loop_stack.push(LoopTargets {
                        label: label.clone(),
                        continue_block: step_block,
                        break_block: exit,
                        break_value_ty: None,
                        shield_depth: self.shield_depth,
                        shared_transaction_depth: self.shared_transaction_depth,
                    });
                    self.b.switch_to_block(body_block);
                    self.b.seal_block(body_block);
                    if map_pairs {
                        let value_ty = match &collection.ty {
                            Type::Map { value, .. } => value.as_ref().clone(),
                            _ => unreachable!(),
                        };
                        let key_ref = self
                            .module
                            .declare_func_in_func(self.host.coll.map_key_at, self.b.func);
                        let key_call = self.b.ins().call(key_ref, &[coll, idx]);
                        let key_val = self.b.inst_results(key_call)[0];
                        let val_ref = self
                            .module
                            .declare_func_in_func(self.host.coll.map_value_at, self.b.func);
                        let val_call = self.b.ins().call(val_ref, &[coll, idx]);
                        let val_raw = self.b.inst_results(val_call)[0];
                        let key_var = self.fresh_var(types::I64);
                        self.b.def_var(key_var, key_val);
                        self.vars.insert(TIR::local_place(var), key_var);
                        self.var_tys.insert(TIR::local_place(var), Type::String);
                        let val_clif = self.meta.clif_ty(&value_ty).ok_or_else(|| {
                            format!("jit for-in map value type unsupported: {value_ty:?}")
                        })?;
                        let val_coerced = match val_clif {
                            types::I32 => self.b.ins().ireduce(types::I32, val_raw),
                            types::I8 => self.b.ins().ireduce(types::I8, val_raw),
                            types::F64 => self.b.ins().bitcast(
                                types::F64,
                                Self::scalar_bitcast_memflags(),
                                val_raw,
                            ),
                            _ => val_raw,
                        };
                        let val_var = self.fresh_var(val_clif);
                        self.b.def_var(val_var, val_coerced);
                        self.vars.insert(TIR::local_place(value_name), val_var);
                        self.var_tys
                            .insert(TIR::local_place(value_name), value_ty);
                    } else {
                        // D-RANGE-EXCL1=C: sequence two-binding → index then item.
                        let elem_ty = list_elem_ty.expect("list pair elem");
                        let idx_bind = self.fresh_var(types::I64);
                        self.b.def_var(idx_bind, idx);
                        self.vars.insert(TIR::local_place(var), idx_bind);
                        self.var_tys.insert(TIR::local_place(var), Type::Int);
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
                        let elem_clif = self.meta.clif_ty(&elem_ty).ok_or_else(|| {
                            format!("jit for-in list pair elem unsupported: {elem_ty:?}")
                        })?;
                        let elem = match elem_clif {
                            types::I32 => self.b.ins().ireduce(types::I32, elem),
                            types::I8 => self.b.ins().ireduce(types::I8, elem),
                            _ => elem,
                        };
                        let item_var = self.fresh_var(elem_clif);
                        self.b.def_var(item_var, elem);
                        self.vars.insert(TIR::local_place(value_name), item_var);
                        self.var_tys
                            .insert(TIR::local_place(value_name), elem_ty);
                    }
                    self.lower_stmts_scoped(body)?;
                    self.loop_stack.pop();
                    if !self.dead {
                        self.b.ins().jump(step_block, &[]);
                    }

                    self.b.switch_to_block(step_block);
                    self.b.seal_block(step_block);
                    let idx = self.b.use_var(idx_var);
                    let remaining = self.b.ins().isub(len, idx);
                    let at_end = self.b.ins().icmp(
                        IntCC::SignedGreaterThanOrEqual,
                        stride,
                        remaining,
                    );
                    let advanced = self.b.ins().iadd(idx, stride);
                    let next = self.b.ins().select(at_end, len, advanced);
                    self.b.def_var(idx_var, next);
                    self.b.ins().jump(header, &[]);
                    self.b.seal_block(header);

                    self.b.switch_to_block(exit);
                    self.b.seal_block(exit);
                    self.dead = false;
                } else {
                    let elem_ty = if matches!(
                        method_kind,
                        Some(
                            TForInMethod::Chars
                                | TForInMethod::LinesProcessStream
                                | TForInMethod::LinesFile
                                | TForInMethod::LinesStdin
                        )
                    ) {
                        if matches!(method_kind, Some(TForInMethod::Chars)) {
                            Type::Char
                        } else {
                            Type::String
                        }
                    } else {
                        jit_list_iter_elem_type(&collection.ty)
                            .or_else(|| jit_closure_elem_type(&collection.ty))
                            .ok_or_else(|| {
                                format!(
                                    "jit for-in collection type unsupported: {:?}",
                                    collection.ty
                                )
                            })?
                    };
                    // The collection value is computed once, before the loop header, so it
                    // dominates both the header and the body.
                    let coll = if matches!(method_kind, Some(TForInMethod::Chars)) {
                        let text = self.lower_expr(source)?;
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.str_chars, self.b.func);
                        let call = self.b.ins().call(host_ref, &[text]);
                        self.b.inst_results(call)[0]
                    } else if matches!(method_kind, Some(TForInMethod::LinesProcessStream)) {
                        let (child_expr, stream_tag) = match &source.kind {
                            TExprKind::Field {
                                recv,
                                field,
                                boxed: false,
                            } if matches!(&recv.ty, Type::Named(n) if n == "ProcessChild")
                                && (field == "stdout" || field == "stderr") =>
                            {
                                (
                                    recv.as_ref(),
                                    if field == "stderr" { 1i64 } else { 0i64 },
                                )
                            }
                            _ => {
                                return Err(
                                    "jit for-in method-call collection unsupported".to_string(),
                                )
                            }
                        };
                        let child_val = self.lower_expr(child_expr)?;
                        let tag = self.b.ins().iconst(types::I64, stream_tag);
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.process.stream_lines, self.b.func);
                        let call = self.b.ins().call(host_ref, &[child_val, tag]);
                        self.b.inst_results(call)[0]
                    } else if matches!(method_kind, Some(TForInMethod::LinesFile)) {
                        let file = self.lower_expr(source)?;
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.io.file_lines, self.b.func);
                        let call = self.b.ins().call(host_ref, &[file]);
                        self.b.inst_results(call)[0]
                    } else if matches!(method_kind, Some(TForInMethod::LinesStdin)) {
                        let stdin = self.lower_expr(source)?;
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.io.stdin_lines, self.b.func);
                        let call = self.b.ins().call(host_ref, &[stdin]);
                        self.b.inst_results(call)[0]
                    } else {
                        self.lower_expr(source)?
                    };
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
                        break_value_ty: None,
                        shield_depth: self.shield_depth,
                        shared_transaction_depth: self.shared_transaction_depth,
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
                    let elem_clif = self.meta.clif_ty(&elem_ty).ok_or_else(|| {
                        format!("jit for-in element type unsupported: {elem_ty:?}")
                    })?;
                    let elem = match elem_clif {
                        types::I32 => self.b.ins().ireduce(types::I32, elem),
                        types::I8 => self.b.ins().ireduce(types::I8, elem),
                        _ => elem,
                    };
                    let loop_var = self.fresh_var(elem_clif);
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
                    let remaining = self.b.ins().isub(len, idx);
                    let at_end = self.b.ins().icmp(
                        IntCC::SignedGreaterThanOrEqual,
                        stride,
                        remaining,
                    );
                    let advanced = self.b.ins().iadd(idx, stride);
                    let next = self.b.ins().select(at_end, len, advanced);
                    self.b.def_var(idx_var, next);
                    self.b.ins().jump(header, &[]);
                    self.b.seal_block(header);

                    self.b.switch_to_block(exit);
                    self.b.seal_block(exit);
                    self.dead = false;
                }
            }
            TStmt::EnumMatch {
                scrutinee,
                clone_subject: _,
                arms,
                else_body,
                fallthrough,
            } => {
                if matches!(&scrutinee.ty, Type::Option(_)) {
                    self.lower_option_enum_match(
                        scrutinee,
                        arms,
                        else_body.as_deref(),
                        *fallthrough,
                    )?;
                    return Ok(());
                }
                if arms.iter().all(|arm| {
                    matches!(
                        &arm.pattern.pattern,
                        Pattern::Ok { .. } | Pattern::Err { .. }
                    )
                }) {
                    self.lower_result_enum_match(scrutinee, arms, else_body.as_deref(), *fallthrough)?;
                    return Ok(());
                }
                // Ownership clone is a Rust spelling fact; the JIT already owns the
                // value in a register, so the structured scrutinee is enough.
                let subj = self.lower_expr(scrutinee)?;
                let f64_heap = self.enum_match_uses_f64_heap(arms);
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
                    let enum_name = arm
                        .pattern
                        .enum_type
                        .as_deref()
                        .or_else(|| user_type_name(&scrutinee.ty));
                    let then_block = self.b.create_block();
                    let next = self.b.create_block();
                    let eq = self.lower_pattern_condition(
                        subj,
                        &arm.pattern.pattern,
                        enum_name,
                        f64_heap,
                    )?;
                    self.b.ins().brif(eq, then_block, &[], next, &[]);
                    self.b.switch_to_block(then_block);
                    self.b.seal_block(then_block);
                    let bound = if let Some((variant, name)) =
                        Self::pattern_binding(&arm.pattern.pattern)
                    {
                                let enum_name =
                                    enum_name.ok_or("jit enum binding missing type")?;
                                let payload_ty = self
                                    .meta
                                    .enum_variant_payload_types(enum_name, variant)
                                    .and_then(|tys| tys.first())
                                    .cloned()
                                    .unwrap_or(Type::Int);
                                let payload = if f64_heap {
                                    self.unpack_enum_heap_payload(subj, &payload_ty)?
                                } else {
                                    self.unpack_enum_scalar(subj, &payload_ty)?
                                };
                                let payload_clif = self.meta.clif_ty(&payload_ty).unwrap_or(types::I64);
                                let var = self.fresh_var(payload_clif);
                                self.b.def_var(var, payload);
                                let key = TIR::local_place(name);
                                let old_var = self.vars.insert(key.clone(), var);
                                let old_ty = self.var_tys.insert(key.clone(), payload_ty);
                                Some((key, old_var, old_ty))
                    } else {
                        None
                    };
                    self.lower_stmts_scoped(&arm.body)?;
                    if let Some((key, old_var, old_ty)) = bound {
                        match old_var {
                            Some(var) => {
                                self.vars.insert(key.clone(), var);
                            }
                            None => {
                                self.vars.remove(&key);
                            }
                        }
                        match old_ty {
                            Some(ty) => {
                                self.var_tys.insert(key, ty);
                            }
                            None => {
                                self.var_tys.remove(&key);
                            }
                        }
                    }
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
                subject,
                arms,
                else_body,
            } => {
                let subject_value = self.lower_expr(subject)?;
                let saved_subject = self
                    .switch_subject
                    .replace((subject_value, subject.ty.clone()));
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
                self.switch_subject = saved_subject;
            }
            TStmt::Region(body) | TStmt::Impure(body) => {
                self.lower_stmts_scoped(body)?;
            }
            TStmt::DebugOnly(body) => {
                self.lower_stmts_scoped(body)?;
            }
            TStmt::StructDestructure { init, binds, .. } => {
                self.lower_struct_destructure(init, binds)?;
            }
            TStmt::ListDestructure {
                init,
                elems,
                want,
                line,
                ..
            } => {
                self.lower_list_destructure(init, elems, *want, *line)?;
            }
            TStmt::IndexHookAssign { .. } => {
                let TStmt::IndexHookAssign {
                    type_name,
                    base,
                    index,
                    value,
                } = stmt
                else {
                    unreachable!()
                };
                let key = format!("{type_name}::set");
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing method `{key}`"))?;
                let recv = self.lower_expr(base)?;
                let index = self.lower_expr(index)?;
                let value = self.lower_expr(value)?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                self.b.ins().call(func_ref, &[recv, index, value]);
                self.emit_trap_check()?;
            }
            TStmt::GcEdit {
                root,
                replace_all,
                index_temp,
                stmt,
                ..
            } => {
                // D-OPTGC1: AOT wraps the assign in AutomaticRoot::edit_*; JIT
                // stores the same payload handle in the root Variable. Edge-slot
                // bookkeeping is collector-only — print/value semantics use the
                // finite snapshots already packed in the enum/list payload.
                if let Some((temp, value)) = index_temp {
                    let v = self.lower_expr(value)?;
                    let ty = self.b.func.dfg.value_type(v);
                    let var = self.fresh_var(ty);
                    self.b.def_var(var, v);
                    self.vars.insert(temp.clone(), var);
                    self.var_tys.insert(temp.clone(), value.ty.clone());
                }
                if !*replace_all {
                    return Err("jit automatic GC slot edit unsupported".to_string());
                }
                match stmt.as_ref() {
                    TStmt::Assign {
                        op: None,
                        value,
                        ..
                    } => {
                        let val = self.lower_expr(value)?;
                        let root_var = self.vars.get(root).copied().ok_or_else(|| {
                            format!("jit GcEdit unknown root `{root}`")
                        })?;
                        self.b.def_var(root_var, val);
                    }
                    _ => {
                        let root_var = self.vars.get(root).copied().ok_or_else(|| {
                            format!("jit GcEdit unknown root `{root}`")
                        })?;
                        let cur = self.b.use_var(root_var);
                        let jv = self.fresh_var(types::I64);
                        self.b.def_var(jv, cur);
                        self.vars.insert("__jet_value".to_string(), jv);
                        self.var_tys.insert(
                            "__jet_value".to_string(),
                            self.var_tys.get(root).cloned().unwrap_or(Type::Int),
                        );
                        self.lower_gc_edit_body(stmt)?;
                        let new_val = self.b.use_var(jv);
                        self.b.def_var(root_var, new_val);
                    }
                }
            }
            TStmt::MathSwizzleAssign { .. } => {
                return Err("jit math swizzle assign unsupported".to_string());
            }
            TStmt::RangeSwitch {
                subject,
                arms,
                else_body,
            } => {
                let value = self.lower_expr(subject)?;
                let merge = self.b.create_block();
                let mut tail = self.b.create_block();
                self.b.ins().jump(tail, &[]);
                let mut reaches_merge = false;
                for (lo, hi, body) in arms {
                    self.b.switch_to_block(tail);
                    self.b.seal_block(tail);
                    let lo = self.b.ins().iconst(types::I64, *lo);
                    let hi = self.b.ins().iconst(types::I64, *hi);
                    let ge = self.bool_from_icmp(IntCC::SignedGreaterThanOrEqual, value, lo);
                    let le = self.bool_from_icmp(IntCC::SignedLessThanOrEqual, value, hi);
                    let condition = self.b.ins().band(ge, le);
                    let then_block = self.b.create_block();
                    let next = self.b.create_block();
                    self.b
                        .ins()
                        .brif(condition, then_block, &[], next, &[]);
                    self.b.switch_to_block(then_block);
                    self.b.seal_block(then_block);
                    self.lower_stmts_scoped(body)?;
                    if !self.dead {
                        self.b.ins().jump(merge, &[]);
                        reaches_merge = true;
                    }
                    tail = next;
                }
                self.b.switch_to_block(tail);
                self.b.seal_block(tail);
                self.lower_stmts_scoped(else_body)?;
                if !self.dead {
                    self.b.ins().jump(merge, &[]);
                    reaches_merge = true;
                }
                if reaches_merge {
                    self.b.switch_to_block(merge);
                    self.b.seal_block(merge);
                    self.dead = false;
                }
            }
            TStmt::Inline(stmts) => self.lower_stmts_scoped(stmts)?,
            TStmt::Unsafe(body) => {
                self.unsafe_depth += 1;
                self.lower_stmts_scoped(body)?;
                self.unsafe_depth -= 1;
            }
            TStmt::Reactive { .. } => return Err("jit reactive statement unsupported".to_string()),
            TStmt::Layout { .. } => return Err("jit layout block unsupported".to_string()),
            TStmt::ContextBlock { guards, body } => {
                let mut pushed = 0u32;
                for (field, value) in guards {
                    if field == "deadline" {
                        let ms = self.lower_expr(value)?;
                        let push = self
                            .module
                            .declare_func_in_func(self.host.conc.deadline_push, self.b.func);
                        self.b.ins().call(push, &[ms]);
                        self.deadline_depth += 1;
                        pushed += 1;
                    } else {
                        let _ = self.lower_expr(value)?;
                    }
                }
                self.lower_stmts_scoped(body)?;
                if !self.dead {
                    self.emit_deadline_pops_to(self.deadline_depth - pushed);
                    self.deadline_depth -= pushed;
                }
            }
            TStmt::Live { body } => {
                let enter = self
                    .module
                    .declare_func_in_func(self.host.io.term_enter, self.b.func);
                self.b.ins().call(enter, &[]);
                let guard_depth = self.scope_guards.len();
                self.scope_guards.push(self.host.io.term_leave);
                self.lower_stmts_scoped(body)?;
                if !self.dead {
                    while self.scope_guards.len() > guard_depth {
                        let id = self.scope_guards.pop().expect("live guard");
                        let leave = self.module.declare_func_in_func(id, self.b.func);
                        self.b.ins().call(leave, &[]);
                        self.emit_trap_check()?;
                    }
                }
            }
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
            TStmt::Transact {
                snapshots,
                uses_stm,
                body,
                ..
            } => {
                let mut snaps = Vec::new();
                for (local, rollback_ty) in snapshots {
                    let place = TIR::local_place(&local.name);
                    let var = self
                        .vars
                        .get(&place)
                        .copied()
                        .ok_or_else(|| {
                            format!("jit transaction snapshot unknown `{}`", local.name)
                        })?;
                    let current = self.b.use_var(var);
                    // Prefer the TIR-stamped Rollback type; otherwise detect a
                    // compiled `T::snapshot`/`T::restore` pair from the local's type.
                    let rollback_name = rollback_ty
                        .as_ref()
                        .map(|ty| ty.name())
                        .or_else(|| {
                            self.var_tys.get(&place).and_then(|ty| match ty {
                                Type::Named(n)
                                    if self.func_ids.contains_key(&format!("{n}::snapshot"))
                                        && self.func_ids.contains_key(&format!("{n}::restore")) =>
                                {
                                    Some(n.clone())
                                }
                                _ => None,
                            })
                        });
                    let scalar_mut = matches!(
                        self.var_tys.get(&place),
                        Some(Type::Apply { name, .. }) if name == "__JetScalarMut"
                    );
                    let (snap, kind) = if let Some(type_name) = rollback_name {
                        let snap_id = self
                            .func_ids
                            .get(&format!("{type_name}::snapshot"))
                            .copied()
                            .ok_or_else(|| {
                                format!(
                                    "jit transaction snapshot missing `{type_name}::snapshot`"
                                )
                            })?;
                        let snap_ref = self.module.declare_func_in_func(snap_id, self.b.func);
                        let call = self.b.ins().call(snap_ref, &[current]);
                        (self.b.inst_results(call)[0], TxnSnap::Rollback(type_name))
                    } else if scalar_mut {
                        let clif = match self.var_tys.get(&place) {
                            Some(Type::Apply { args, .. }) if !args.is_empty() => self
                                .meta
                                .clif_ty(&args[0])
                                .ok_or("jit scalar-mut snapshot type")?,
                            _ => types::I64,
                        };
                        (
                            self.b.ins().load(clif, MemFlags::trusted(), current, 0),
                            TxnSnap::ScalarMut,
                        )
                    } else {
                        (current, TxnSnap::Plain)
                    };
                    snaps.push((place, snap, kind));
                }
                self.txn_stack.push(TxnFrame {
                    snapshots: snaps,
                    on_commit: Vec::new(),
                    on_rollback: Vec::new(),
                });
                if *uses_stm {
                    let begin = self.module.declare_func_in_func(
                        self.host.memory.shared_txn_begin,
                        self.b.func,
                    );
                    self.b.ins().call(begin, &[]);
                    self.in_shared_transaction = true;
                    self.shared_transaction_depth += 1;
                }
                self.lower_stmts_scoped(body)?;
                if *uses_stm {
                    self.shared_transaction_depth -= 1;
                    self.in_shared_transaction = self.shared_transaction_depth != 0;
                    if !self.dead {
                        let commit = self.module.declare_func_in_func(
                            self.host.memory.shared_txn_commit,
                            self.b.func,
                        );
                        self.b.ins().call(commit, &[]);
                        self.emit_trap_check()?;
                    }
                }
                if !self.dead {
                    self.emit_txn_commit_hooks()?;
                    let _ = self.txn_stack.pop();
                }
            }
            TStmt::LineMarker(_) => return Err("jit line marker unsupported".to_string()),
        }
        Ok(())
    }

    fn lower_iterable_for_in(
        &mut self,
        label: &Option<String>,
        var: &str,
        source: &TExpr,
        step: Option<&TExpr>,
        body: &[TStmt],
        collection_type: &str,
        iterator_type: &str,
    ) -> Result<(), String> {
        let item_type = self
            .meta
            .iterable_item_type(collection_type, iterator_type)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "jit Iterable item type missing for `{collection_type}` / `{iterator_type}`"
                )
            })?;
        let iter_id = self
            .func_ids
            .get(&format!("{collection_type}::iter"))
            .copied()
            .ok_or_else(|| format!("jit missing method `{collection_type}::iter`"))?;
        let next_id = self
            .func_ids
            .get(&format!("{iterator_type}::next"))
            .copied()
            .ok_or_else(|| format!("jit missing method `{iterator_type}::next`"))?;
        let collection = self.lower_expr(source)?;
        let iter_ref = self.module.declare_func_in_func(iter_id, self.b.func);
        let iter_call = self.b.ins().call(iter_ref, &[collection]);
        let iterator = self.b.inst_results(iter_call)[0];
        let iterator_var = self.fresh_var(types::I64);
        self.b.def_var(iterator_var, iterator);
        let stride = match step {
            Some(step) => {
                let value = self.lower_expr(step)?;
                let check = self
                    .module
                    .declare_func_in_func(self.host.coll.loop_stride_check, self.b.func);
                let call = self.b.ins().call(check, &[value]);
                let value = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                value
            }
            None => self.b.ins().iconst(types::I64, 1),
        };
        let remaining_var = self.fresh_var(types::I64);
        let zero = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(remaining_var, zero);
        let header = self.b.create_block();
        let body_block = self.b.create_block();
        let skip_block = self.b.create_block();
        let step_block = self.b.create_block();
        let exit = self.b.create_block();
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        let iterator = self.b.use_var(iterator_var);
        let next_ref = self.module.declare_func_in_func(next_id, self.b.func);
        let next_call = self.b.ins().call(next_ref, &[iterator]);
        let packed = self.b.inst_results(next_call)[0];
        self.emit_trap_check()?;
        let zero = self.b.ins().iconst(types::I64, 0);
        let closed = self.b.ins().icmp(IntCC::Equal, packed, zero);
        let dispatch = self.b.create_block();
        self.b.ins().brif(closed, exit, &[], dispatch, &[]);
        self.b.switch_to_block(dispatch);
        self.b.seal_block(dispatch);
        let remaining = self.b.use_var(remaining_var);
        let deliver = self.b.ins().icmp(IntCC::Equal, remaining, zero);
        self.b
            .ins()
            .brif(deliver, body_block, &[], skip_block, &[]);

        self.b.switch_to_block(skip_block);
        self.b.seal_block(skip_block);
        let one = self.b.ins().iconst(types::I64, 1);
        let remaining = self.b.ins().isub(remaining, one);
        self.b.def_var(remaining_var, remaining);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(body_block);
        self.b.seal_block(body_block);
        let value = self.unpack_option_payload(packed, &item_type)?;
        let value_type = self
            .meta
            .clif_ty(&item_type)
            .ok_or_else(|| format!("jit Iterable item unsupported: {item_type:?}"))?;
        let loop_var = self.fresh_var(value_type);
        self.b.def_var(loop_var, value);
        self.vars.insert(TIR::local_place(var), loop_var);
        self.var_tys
            .insert(TIR::local_place(var), item_type);
        self.loop_stack.push(LoopTargets {
            label: label.clone(),
            continue_block: step_block,
            break_block: exit,
            break_value_ty: None,
            shield_depth: self.shield_depth,
            shared_transaction_depth: self.shared_transaction_depth,
        });
        self.lower_stmts_scoped(body)?;
        self.loop_stack.pop();
        if !self.dead {
            self.b.ins().jump(step_block, &[]);
        }

        self.b.switch_to_block(step_block);
        self.b.seal_block(step_block);
        let one = self.b.ins().iconst(types::I64, 1);
        let remaining = self.b.ins().isub(stride, one);
        self.b.def_var(remaining_var, remaining);
        self.b.ins().jump(header, &[]);
        self.b.seal_block(header);

        self.b.switch_to_block(exit);
        self.b.seal_block(exit);
        self.dead = false;
        Ok(())
    }

    fn lower_stream_for_in(
        &mut self,
        label: &Option<String>,
        var: &str,
        source: &TExpr,
        step: Option<&TExpr>,
        body: &[TStmt],
        stream_type: &Type,
    ) -> Result<(), String> {
        if step.is_some() {
            return Err("jit Stream stride unsupported".to_string());
        }
        let Type::Apply { args, .. } = stream_type else {
            unreachable!()
        };
        let item_type = args
            .first()
            .cloned()
            .ok_or("jit Stream item type missing")?;
        let channel = self.lower_expr(source)?;
        let channel_var = self.fresh_var(types::I64);
        self.b.def_var(channel_var, channel);
        let header = self.b.create_block();
        let body_block = self.b.create_block();
        let cancel = self.b.create_block();
        let exit = self.b.create_block();
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        let channel = self.b.use_var(channel_var);
        let receive = self.module.declare_func_in_func(
            self.host.conc.channel_receive_status,
            self.b.func,
        );
        let call = self.b.ins().call(receive, &[channel]);
        let packed = self.finish_wait_call(self.b.inst_results(call)[0]);
        let zero = self.b.ins().iconst(types::I64, 0);
        let closed = self.b.ins().icmp(IntCC::Equal, packed, zero);
        self.b
            .ins()
            .brif(closed, exit, &[], body_block, &[]);

        self.b.switch_to_block(body_block);
        self.b.seal_block(body_block);
        let value = self.unpack_option_payload(packed, &item_type)?;
        let clif = self
            .meta
            .clif_ty(&item_type)
            .ok_or_else(|| format!("jit Stream item unsupported: {item_type:?}"))?;
        let loop_var = self.fresh_var(clif);
        self.b.def_var(loop_var, value);
        self.vars.insert(TIR::local_place(var), loop_var);
        self.var_tys
            .insert(TIR::local_place(var), item_type);
        self.loop_stack.push(LoopTargets {
            label: label.clone(),
            continue_block: header,
            break_block: cancel,
            break_value_ty: None,
            shield_depth: self.shield_depth,
            shared_transaction_depth: self.shared_transaction_depth,
        });
        self.lower_stmts_scoped(body)?;
        self.loop_stack.pop();
        if !self.dead {
            self.b.ins().jump(header, &[]);
        }
        self.b.switch_to_block(cancel);
        self.b.seal_block(cancel);
        let channel = self.b.use_var(channel_var);
        let close = self
            .module
            .declare_func_in_func(self.host.conc.channel_close, self.b.func);
        self.b.ins().call(close, &[channel]);
        self.b.ins().jump(exit, &[]);
        self.b.seal_block(header);
        self.b.switch_to_block(exit);
        self.b.seal_block(exit);
        self.dead = false;
        Ok(())
    }

    /// Body of `TStmt::GcEdit` when the nested stmt is not a plain assign.
    /// `__jet_value` is already seeded from the root; write-through deref
    /// assigns update that Variable (not `struct_assign`).
    fn lower_gc_edit_body(&mut self, stmt: &TStmt) -> Result<(), String> {
        match stmt {
            TStmt::Assign {
                place,
                op: None,
                value,
                ..
            } => {
                let local = place.as_local().ok_or("jit GcEdit assign to non-local")?;
                let key = Self::local_key(local);
                let var = self
                    .vars
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit GcEdit assign unknown `{key}`"))?;
                let val = self.lower_expr(value)?;
                self.b.def_var(var, val);
                Ok(())
            }
            TStmt::ExprStmt(expr) => {
                self.lower_expr(expr)?;
                Ok(())
            }
            other => {
                // Fall back to normal stmt lowering for rare nested shapes.
                self.lower_stmt(other)
            }
        }
    }

    /// `THostCall` forms that the resident JIT lowers. Other host forms stay
    /// named-unsupported (same gap string as before for CtLit/DefaultLit).

    fn lower_reflect_of(&mut self, arg: &TExpr) -> Result<Value, String> {
        let type_name = match &arg.ty {
            Type::Named(n) => n.clone(),
            Type::Int => "Int".into(),
            Type::Float => "Float".into(),
            Type::Bool => "Bool".into(),
            Type::String => "String".into(),
            other => other.name(),
        };
        let type_h = self.runtime.heap.alloc_string(type_name.clone());
        let type_v = self.b.ins().iconst(types::I64, type_h);

        // display string — reuse Display path / scalar push
        let begin = self.module.declare_func_in_func(self.host.str_begin, self.b.func);
        let call = self.b.ins().call(begin, &[]);
        let buf = self.b.inst_results(call)[0];
        match &arg.ty {
            Type::Named(n) => {
                self.lower_named_str_interp(buf, arg, n, jet_foundation::AST::StrFormat::Display)?;
            }
            Type::Int => {
                let v = self.lower_expr(arg)?;
                let push = self.module.declare_func_in_func(self.host.str_push_i64, self.b.func);
                self.b.ins().call(push, &[buf, v]);
            }
            Type::Float => {
                let v = self.lower_expr(arg)?;
                let push = self.module.declare_func_in_func(self.host.str_push_f64, self.b.func);
                self.b.ins().call(push, &[buf, v]);
            }
            Type::Bool => {
                let v = self.lower_expr(arg)?;
                let push = self.module.declare_func_in_func(self.host.str_push_bool, self.b.func);
                self.b.ins().call(push, &[buf, v]);
            }
            Type::String => {
                let v = self.lower_expr(arg)?;
                let push = self.module.declare_func_in_func(self.host.str_push_str, self.b.func);
                self.b.ins().call(push, &[buf, v]);
            }
            _ => {
                let v = self.lower_expr(arg)?;
                let push = self.module.declare_func_in_func(self.host.str_push_i64, self.b.func);
                self.b.ins().call(push, &[buf, v]);
            }
        }
        let display_v = buf; // str_begin buffer IS the string handle

        // fields
        let list_new = self.module.declare_func_in_func(self.host.coll.list_new, self.b.func);
        let fields_call = self.b.ins().call(list_new, &[]);
        let fields = self.b.inst_results(fields_call)[0];
        if let Type::Named(n) = &arg.ty {
            if let Some((names, tys)) = self.meta.struct_layout(n) {
                let recv = self.lower_expr(arg)?;
                let push = self.module.declare_func_in_func(self.host.coll.list_push, self.b.func);
                let field_new = self
                    .module
                    .declare_func_in_func(self.host.reflect_field_new, self.b.func);
                for (i, (fname, fty)) in names.iter().zip(tys.iter()).enumerate() {
                    let jet_name = fname.strip_prefix("user_").unwrap_or(fname);
                    let name_h = self.runtime.heap.alloc_string(jet_name.to_string());
                    let name_v = self.b.ins().iconst(types::I64, name_h);
                    let field_val = self.lower_record_field(recv, n, fname, fty)?;
                    let begin = self.module.declare_func_in_func(self.host.str_begin, self.b.func);
                    let bcall = self.b.ins().call(begin, &[]);
                    let fbuf = self.b.inst_results(bcall)[0];
                    match fty {
                        Type::Int | Type::IntN { .. } => {
                            let p = self.module.declare_func_in_func(self.host.str_push_i64, self.b.func);
                            self.b.ins().call(p, &[fbuf, field_val]);
                        }
                        Type::Float | Type::Float32 => {
                            let p = self.module.declare_func_in_func(self.host.str_push_f64, self.b.func);
                            self.b.ins().call(p, &[fbuf, field_val]);
                        }
                        Type::Bool => {
                            let p = self.module.declare_func_in_func(self.host.str_push_bool, self.b.func);
                            self.b.ins().call(p, &[fbuf, field_val]);
                        }
                        Type::String => {
                            let p = self.module.declare_func_in_func(self.host.str_push_str, self.b.func);
                            self.b.ins().call(p, &[fbuf, field_val]);
                        }
                        _ => {
                            let p = self.module.declare_func_in_func(self.host.str_push_i64, self.b.func);
                            self.b.ins().call(p, &[fbuf, field_val]);
                        }
                    }
                    let _ = i;
                    let fnew = self.b.ins().call(field_new, &[name_v, fbuf]);
                    let fh = self.b.inst_results(fnew)[0];
                    self.b.ins().call(push, &[fields, fh]);
                }
            }
        }
        let finish = self
            .module
            .declare_func_in_func(self.host.reflect_of_finish, self.b.func);
        let call = self.b.ins().call(finish, &[type_v, display_v, fields]);
        Ok(self.b.inst_results(call)[0])
    }

    fn lower_testing_call(
        &mut self,
        method: &str,
        args: &[TExpr],
        _ty: &Type,
    ) -> Result<Value, String> {
        match method {
            "temp_dir" if args.len() == 1 => {
                let p = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.testing_temp_dir, self.b.func);
                let call = self.b.ins().call(host, &[p]);
                Ok(self.b.inst_results(call)[0])
            }
            "snap" if args.len() == 2 => {
                let a = self.lower_expr(&args[0])?;
                let b = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.testing_snap, self.b.func);
                let call = self.b.ins().call(host, &[a, b]);
                Ok(self.b.inst_results(call)[0])
            }
            "fake_clock" if args.len() == 1 => {
                let ms = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.clock_new, self.b.func);
                let call = self.b.ins().call(host, &[ms]);
                Ok(self.b.inst_results(call)[0])
            }
            "fake_rng" if args.len() == 1 => {
                let seed = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_new, self.b.func);
                let call = self.b.ins().call(host, &[seed]);
                Ok(self.b.inst_results(call)[0])
            }
            other => Err(format!("jit core call unsupported: core.testing.{other}")),
        }
    }

    fn lower_data_call(
        &mut self,
        method: &str,
        args: &[TExpr],
        ty: &Type,
    ) -> Result<Value, String> {
        match method {
            "csv" if args.len() == 1 => {
                let elem = match ty {
                    Type::Result { ok, .. } => match ok.as_ref() {
                        Type::List(e) => e.as_ref(),
                        _ => return Err("jit data.csv needs Result<[T], _>".into()),
                    },
                    Type::List(e) => e.as_ref(),
                    _ => return Err("jit data.csv needs list result".into()),
                };
                self.lower_typed_csv_decode(&args[0], elem)
            }
            "json" if args.len() == 1 => {
                let ok = match ty {
                    Type::Result { ok, .. } => ok.as_ref(),
                    other => other,
                };
                let elem = match ok {
                    Type::List(e) => e.as_ref(),
                    _ => return Err("jit data.json needs list result".into()),
                };
                // reuse typed json decode into list-of-struct via decode path
                let text = &args[0];
                let tree_ty = Type::List(Box::new(elem.clone()));
                // lower_typed_json_decode expects ok_ty of the Result payload
                self.lower_typed_json_decode(text, ok)
            }
            "count" if args.len() == 1 => {
                let recv = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(self.host.coll.list_len, self.b.func);
                // If Table/LazyFrame/Series, count via field "rows"/"values"
                match &args[0].ty {
                    Type::Named(n) if n == "Table" || n == "LazyFrame" => {
                        if n == "LazyFrame" {
                            let host = self
                                .module
                                .declare_func_in_func(self.host.data.lazy_count, self.b.func);
                            let call = self.b.ins().call(host, &[recv]);
                            return Ok(self.b.inst_results(call)[0]);
                        }
                        let rows = self.lower_record_field(recv, n, "rows", &Type::List(Box::new(Type::Int)))?;
                        let call = self.b.ins().call(host, &[rows]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    Type::Named(n) if n == "Series" => {
                        let vals = self.lower_record_field(recv, n, "values", &Type::List(Box::new(Type::Float)))?;
                        let call = self.b.ins().call(host, &[vals]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    Type::Apply { name, .. } if name == "Table" || name == "LazyFrame" => {
                        if name == "LazyFrame" {
                            let host = self
                                .module
                                .declare_func_in_func(self.host.data.lazy_count, self.b.func);
                            let call = self.b.ins().call(host, &[recv]);
                            return Ok(self.b.inst_results(call)[0]);
                        }
                        let rows = self.lower_record_field(recv, name, "rows", &Type::List(Box::new(Type::Int)))?;
                        let call = self.b.ins().call(host, &[rows]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    Type::Apply { name, .. } if name == "Series" => {
                        let vals = self.lower_record_field(
                            recv,
                            name,
                            "values",
                            &Type::List(Box::new(Type::Float)),
                        )?;
                        let call = self.b.ins().call(host, &[vals]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    _ => {
                        let call = self.b.ins().call(host, &[recv]);
                        Ok(self.b.inst_results(call)[0])
                    }
                }
            }
            "status" if args.is_empty() => {
                let host = self.module.declare_func_in_func(self.host.data.status, self.b.func);
                let call = self.b.ins().call(host, &[]);
                Ok(self.b.inst_results(call)[0])
            }
            "require_bridge" if args.len() == 1 => {
                let p = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(self.host.data.require_bridge, self.b.func);
                let call = self.b.ins().call(host, &[p]);
                Ok(self.b.inst_results(call)[0])
            }
            "mean" | "sum" | "min" | "max" | "median" | "variance" | "stddev" if args.len() == 1 => {
                let v = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(self.host.data.stat, self.b.func);
                let op = match method {
                    "mean" => 0,
                    "sum" => 1,
                    "min" => 2,
                    "max" => 3,
                    "median" => 4,
                    "variance" => 5,
                    _ => 6,
                };
                let op_v = self.b.ins().iconst(types::I64, op);
                let call = self.b.ins().call(host, &[v, op_v]);
                Ok(self.b.inst_results(call)[0])
            }
            "quantile" if args.len() == 2 => {
                let v = self.lower_expr(&args[0])?;
                let q = self.lower_expr(&args[1])?;
                let q_bits = self.b.ins().bitcast(
                    types::I64,
                    Self::scalar_bitcast_memflags(),
                    q,
                );
                let host = self.module.declare_func_in_func(self.host.data.quantile, self.b.func);
                let call = self.b.ins().call(host, &[v, q_bits]);
                Ok(self.b.inst_results(call)[0])
            }
            "filter" if args.len() == 2 => self.lower_data_filter(&args[0], &args[1]),
            "sort_by" if args.len() == 2 => self.lower_data_sort_by(&args[0], &args[1], ty),
            "group_count" if args.len() == 2 => {
                self.lower_data_group(&args[0], &args[1], None, 0)
            }
            "group_sum" if args.len() == 3 => {
                self.lower_data_group(&args[0], &args[1], Some(&args[2]), 1)
            }
            "group_mean" if args.len() == 3 => {
                if matches!(
                    &args[0].ty,
                    Type::Apply { name, .. } if name == "DataStream"
                ) || matches!(&args[0].ty, Type::Named(n) if n == "DataStream")
                {
                    self.lower_data_stream_group_mean(&args[0], &args[1], &args[2])
                } else {
                    self.lower_data_group(&args[0], &args[1], Some(&args[2]), 1)
                }
            }
            "describe" if args.len() == 1 => {
                let v = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.data.describe, self.b.func);
                let call = self.b.ins().call(host, &[v]);
                Ok(self.b.inst_results(call)[0])
            }
            "bar_text" | "bar_svg" if args.len() == 1 => {
                let v = self.lower_expr(&args[0])?;
                let host_id = if method == "bar_text" {
                    self.host.data.bar_text
                } else {
                    self.host.data.bar_svg
                };
                let host = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host, &[v]);
                Ok(self.b.inst_results(call)[0])
            }
            "table" if args.len() == 1 => {
                let rows = self.lower_expr(&args[0])?;
                // DataTable { rows, missing: 0, plan: ["table"] }
                let rec = self.new_record(3);
                self.set_record_slot(rec, 0, rows, &Type::List(Box::new(Type::Int)))?;
                let zero = self.b.ins().iconst(types::I64, 0);
                self.set_record_slot(rec, 1, zero, &Type::Int)?;
                let plan = {
                    let host = self
                        .module
                        .declare_func_in_func(self.host.coll.list_new, self.b.func);
                    let call = self.b.ins().call(host, &[]);
                    let list = self.b.inst_results(call)[0];
                    let step = self.runtime.heap.alloc_string("table");
                    let step_v = self.b.ins().iconst(types::I64, step);
                    let push = self
                        .module
                        .declare_func_in_func(self.host.coll.list_push, self.b.func);
                    self.b.ins().call(push, &[list, step_v]);
                    list
                };
                self.set_record_slot(rec, 2, plan, &Type::List(Box::new(Type::String)))?;
                Ok(rec)
            }
            "rows" if args.len() == 1 => {
                let table = self.lower_expr(&args[0])?;
                let type_name = match &args[0].ty {
                    Type::Named(n) | Type::Apply { name: n, .. } => n.as_str(),
                    _ => "DataTable",
                };
                self.lower_record_field(
                    table,
                    type_name,
                    "rows",
                    &Type::List(Box::new(Type::Int)),
                )
            }
            "series" if args.len() == 1 => {
                let values = self.lower_expr(&args[0])?;
                let rec = self.new_record(2);
                self.set_record_slot(rec, 0, values, &Type::List(Box::new(Type::Float)))?;
                let zero = self.b.ins().iconst(types::I64, 0);
                self.set_record_slot(rec, 1, zero, &Type::Int)?;
                Ok(rec)
            }
            "schema" if args.len() == 1 => self.lower_data_schema(&args[0]),
            "missing_count" if args.len() == 1 => {
                let recv = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.data.missing_count, self.b.func);
                let call = self.b.ins().call(host, &[recv]);
                Ok(self.b.inst_results(call)[0])
            }
            "plan" if args.len() == 1 => {
                let recv = self.lower_expr(&args[0])?;
                let type_name = match &args[0].ty {
                    Type::Named(n) | Type::Apply { name: n, .. } => n.as_str(),
                    _ => "LazyFrame",
                };
                self.lower_record_field(
                    recv,
                    type_name,
                    "plan",
                    &Type::List(Box::new(Type::String)),
                )
            }
            "lazy" if args.len() == 1 => {
                // LazyFrame shares Table layout {rows, missing, plan}.
                let table = self.lower_expr(&args[0])?;
                let type_name = match &args[0].ty {
                    Type::Named(n) | Type::Apply { name: n, .. } => n.as_str(),
                    _ => "Table",
                };
                let rows = self.lower_record_field(
                    table,
                    type_name,
                    "rows",
                    &Type::List(Box::new(Type::Int)),
                )?;
                let missing =
                    self.lower_record_field(table, type_name, "missing", &Type::Int)?;
                let plan = self.lower_record_field(
                    table,
                    type_name,
                    "plan",
                    &Type::List(Box::new(Type::String)),
                )?;
                // Clone plan list so later appends don't mutate the table.
                let clone = self
                    .module
                    .declare_func_in_func(self.host.coll.list_clone, self.b.func);
                let plan = {
                    let call = self.b.ins().call(clone, &[plan]);
                    self.b.inst_results(call)[0]
                };
                let rec = self.new_record(3);
                self.set_record_slot(rec, 0, rows, &Type::List(Box::new(Type::Int)))?;
                self.set_record_slot(rec, 1, missing, &Type::Int)?;
                self.set_record_slot(rec, 2, plan, &Type::List(Box::new(Type::String)))?;
                Ok(rec)
            }
            "lazy_filter" if args.len() == 2 => {
                self.lower_data_lazy_op(&args[0], &args[1], "filter", 0)
            }
            "lazy_sort_by" if args.len() == 2 => {
                self.lower_data_lazy_op(&args[0], &args[1], "sort_by", 1)
            }
            "collect" if args.len() == 1 => {
                let frame = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.data.collect, self.b.func);
                let call = self.b.ins().call(host, &[frame]);
                Ok(self.b.inst_results(call)[0])
            }
            "inner_join" if args.len() == 4 => {
                self.lower_data_join(&args[0], &args[1], &args[2], &args[3], false)
            }
            "left_join" if args.len() == 4 => {
                self.lower_data_join(&args[0], &args[1], &args[2], &args[3], true)
            }
            "pivot_sum" if args.len() == 4 => {
                let rows = &args[0];
                let row_keys =
                    self.lower_iter_map_filter(rows, std::slice::from_ref(&args[1]), false)?;
                let col_keys =
                    self.lower_iter_map_filter(rows, std::slice::from_ref(&args[2]), false)?;
                let values =
                    self.lower_iter_map_filter(rows, std::slice::from_ref(&args[3]), false)?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.data.pivot_sum, self.b.func);
                let call = self.b.ins().call(host, &[row_keys, col_keys, values]);
                Ok(self.b.inst_results(call)[0])
            }
            "rolling_mean" if args.len() == 2 => {
                let v = self.lower_expr(&args[0])?;
                let w = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.data.rolling_mean, self.b.func);
                let call = self.b.ins().call(host, &[v, w]);
                Ok(self.b.inst_results(call)[0])
            }
            "csv_reader" if args.len() == 2 => {
                let file = self.lower_expr(&args[0])?;
                let limits = self.lower_expr(&args[1])?;
                let max_groups =
                    self.lower_record_field(limits, "DataLimits", "max_groups", &Type::Int)?;
                let encoding = self.lower_record_field(
                    limits,
                    "DataLimits",
                    "encoding",
                    &Type::Named("EncodingLimits".into()),
                )?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.data.csv_reader, self.b.func);
                let call = self.b.ins().call(host, &[file, encoding, max_groups]);
                Ok(self.b.inst_results(call)[0])
            }
            other => Err(format!("jit core call unsupported: core.data.{other}")),
        }
    }

    fn lower_data_stream_group_mean(
        &mut self,
        stream: &TExpr,
        key_fn: &TExpr,
        value_fn: &TExpr,
    ) -> Result<Value, String> {
        let stream_v = self.lower_expr(stream)?;
        // Remaining rows as a list handle (advances stream to EOF).
        let rest_host = self
            .module
            .declare_func_in_func(self.host.data.stream_rest, self.b.func);
        let rest_call = self.b.ins().call(rest_host, &[stream_v]);
        let rest = self.b.inst_results(rest_call)[0];
        // rest is Result<list, DataError>
        // For group we need Ok list — use result payload helpers.
        let is_ok = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let ok_call = self.b.ins().call(is_ok, &[rest]);
        let ok = self.b.inst_results(ok_call)[0];
        // Branch: if err, return rest; else group_reduce_limited
        let err_block = self.b.create_block();
        let ok_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        self.b.ins().brif(ok, ok_block, &[], err_block, &[]);

        self.b.switch_to_block(err_block);
        self.b.seal_block(err_block);
        self.b.ins().jump(merge, &[rest]);

        self.b.switch_to_block(ok_block);
        self.b.seal_block(ok_block);
        let payload = self
            .module
            .declare_func_in_func(self.host.result_get_i64, self.b.func);
        let rows_call = self.b.ins().call(payload, &[rest]);
        let rows = self.b.inst_results(rows_call)[0];
        let elem_ty = match &stream.ty {
            Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
            _ => Type::Named("Event".into()),
        };
        let max_g_host = self
            .module
            .declare_func_in_func(self.host.data.stream_max_groups, self.b.func);
        let max_call = self.b.ins().call(max_g_host, &[stream_v]);
        let max_g = self.b.inst_results(max_call)[0];
        let key_list = self.lower_list_value_map_only(rows, &elem_ty, key_fn)?;
        let val_list = self.lower_list_value_map_only(rows, &elem_ty, value_fn)?;
        let host = self
            .module
            .declare_func_in_func(self.host.data.group_reduce_limited, self.b.func);
        let call = self
            .b
            .ins()
            .call(host, &[key_list, val_list, max_g]);
        let grouped = self.b.inst_results(call)[0];
        self.b.ins().jump(merge, &[grouped]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_list_value_map_only(
        &mut self,
        list: Value,
        elem_ty: &Type,
        lambda: &TExpr,
    ) -> Result<Value, String> {
        // Map without sorting: reuse filter=false path but only return key list
        // by cloning the map portion. Implement by calling map_filter with a
        // local flag — duplicate loop with push mapped only.
        let (param_place, body_expr) = self.closure_unary_lambda(std::slice::from_ref(lambda))?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, list);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_val = self.b.inst_results(out_call)[0];
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, out_val);
        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);
        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let mapped = self.with_bound_local(&param_place, elem_ty.clone(), elem, |this| {
            this.lower_expr(body_expr)
        })?;
        let out = self.b.use_var(out_var);
        let mapped_ty = self.erase_distinct_ty(&body_expr.ty);
        let (push_id, push_val) = if matches!(mapped_ty, Type::Float | Type::Float32) {
            (self.host.coll.list_push_f64, mapped)
        } else {
            (self.host.coll.list_push, mapped)
        };
        let push_ref = self.module.declare_func_in_func(push_id, self.b.func);
        self.b.ins().call(push_ref, &[out, push_val]);
        self.b.ins().jump(step, &[]);
        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);
        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(out_var))
    }

    fn lower_data_join(
        &mut self,
        left: &TExpr,
        right: &TExpr,
        left_key: &TExpr,
        right_key: &TExpr,
        is_left: bool,
    ) -> Result<Value, String> {
        let left_v = self.lower_expr(left)?;
        let right_v = self.lower_expr(right)?;
        let left_keys =
            self.lower_iter_map_filter(left, std::slice::from_ref(left_key), false)?;
        let right_keys =
            self.lower_iter_map_filter(right, std::slice::from_ref(right_key), false)?;
        let host_id = if is_left {
            self.host.data.left_join
        } else {
            self.host.data.inner_join
        };
        let host = self.module.declare_func_in_func(host_id, self.b.func);
        let call = self
            .b
            .ins()
            .call(host, &[left_v, right_v, left_keys, right_keys]);
        Ok(self.b.inst_results(call)[0])
    }

    fn lower_data_lazy_op(
        &mut self,
        frame: &TExpr,
        lambda: &TExpr,
        plan_step: &str,
        kind: i64,
    ) -> Result<Value, String> {
        let src = self.lower_expr(frame)?;
        let type_name = match &frame.ty {
            Type::Named(n) | Type::Apply { name: n, .. } => n.as_str(),
            _ => "LazyFrame",
        };
        let elem_ty = match &frame.ty {
            Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
            _ => Type::Named("Ticket".into()),
        };
        let rows = self.lower_record_field(
            src,
            type_name,
            "rows",
            &Type::List(Box::new(Type::Int)),
        )?;
        let missing = self.lower_record_field(src, type_name, "missing", &Type::Int)?;
        let plan = self.lower_record_field(
            src,
            type_name,
            "plan",
            &Type::List(Box::new(Type::String)),
        )?;
        let clone = self
            .module
            .declare_func_in_func(self.host.coll.list_clone, self.b.func);
        let plan = {
            let call = self.b.ins().call(clone, &[plan]);
            self.b.inst_results(call)[0]
        };
        let step = self.runtime.heap.alloc_string(plan_step);
        let step_v = self.b.ins().iconst(types::I64, step);
        let push = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);
        self.b.ins().call(push, &[plan, step_v]);

        // Skip applying lambdas that call named helpers (deferred probe).
        let defer_body = match &lambda.kind {
            TExprKind::Lambda(lam) => match &lam.executable {
                jet_codegen::Codegen::TIR::TLambdaBody::Expr(e) => {
                    matches!(&e.kind, TExprKind::Call { .. })
                }
                _ => true,
            },
            _ => true,
        };

        let rows = if defer_body {
            let call = self.b.ins().call(clone, &[rows]);
            self.b.inst_results(call)[0]
        } else {
            self.lower_list_value_map_filter(rows, &elem_ty, lambda, kind == 0)?
        };

        let rec = self.new_record(3);
        self.set_record_slot(rec, 0, rows, &Type::List(Box::new(Type::Int)))?;
        self.set_record_slot(rec, 1, missing, &Type::Int)?;
        self.set_record_slot(rec, 2, plan, &Type::List(Box::new(Type::String)))?;
        Ok(rec)
    }

    /// Map/filter/sort-by over an already-lowered list handle.
    /// `filter==true` keeps elems; `filter==false` builds key list then stable-sorts.
    fn lower_list_value_map_filter(
        &mut self,
        list: Value,
        elem_ty: &Type,
        lambda: &TExpr,
        filter: bool,
    ) -> Result<Value, String> {
        let (param_place, body_expr) = self.closure_unary_lambda(std::slice::from_ref(lambda))?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, list);

        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_val = self.b.inst_results(out_call)[0];
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, out_val);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let mapped = self.with_bound_local(&param_place, elem_ty.clone(), elem, |this| {
            this.lower_expr(body_expr)
        })?;

        if filter {
            let keep_block = self.b.create_block();
            let zero_b = self.b.ins().iconst(types::I8, 0);
            let keep = self.b.ins().icmp(IntCC::NotEqual, mapped, zero_b);
            self.b.ins().brif(keep, keep_block, &[], step, &[]);
            self.b.switch_to_block(keep_block);
            self.b.seal_block(keep_block);
            let out = self.b.use_var(out_var);
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[out, elem]);
            self.b.ins().jump(step, &[]);
        } else {
            let out = self.b.use_var(out_var);
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[out, mapped]);
            self.b.ins().jump(step, &[]);
        }

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        let out = self.b.use_var(out_var);
        if filter {
            Ok(out)
        } else {
            // out holds keys; sort a clone of the input list by those keys.
            let clone = self
                .module
                .declare_func_in_func(self.host.coll.list_clone, self.b.func);
            let cloned = {
                let call = self.b.ins().call(clone, &[list]);
                self.b.inst_results(call)[0]
            };
            let key_is_str = match &lambda.kind {
                TExprKind::Lambda(lam) => matches!(lam.ret.as_ref(), Some(Type::String))
                    || matches!(
                        &lam.executable,
                        jet_codegen::Codegen::TIR::TLambdaBody::Expr(e) if matches!(e.ty, Type::String)
                    ),
                _ => true,
            };
            let sort_id = if key_is_str {
                self.host.coll.list_sort_by_str_keys
            } else {
                self.host.coll.list_sort_by_i64_keys
            };
            let host = self.module.declare_func_in_func(sort_id, self.b.func);
            self.b.ins().call(host, &[cloned, out]);
            Ok(cloned)
        }
    }

    fn lower_data_schema(&mut self, table: &TExpr) -> Result<Value, String> {
        let row_ty = match &table.ty {
            Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
            Type::List(inner) => inner.as_ref().clone(),
            _ => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.list_new, self.b.func);
                let call = self.b.ins().call(host, &[]);
                return Ok(self.b.inst_results(call)[0]);
            }
        };
        let host_new = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(host_new, &[]);
        let out = self.b.inst_results(out_call)[0];
        let push = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);

        let push_col = |this: &mut Self, out: Value, name: &str, ty_name: &str| -> Result<(), String> {
            let col = this.new_record(2);
            let name_h = this.runtime.heap.alloc_string(name);
            let name_v = this.b.ins().iconst(types::I64, name_h);
            this.set_record_slot(col, 0, name_v, &Type::String)?;
            let ty_h = this.runtime.heap.alloc_string(ty_name);
            let ty_v = this.b.ins().iconst(types::I64, ty_h);
            this.set_record_slot(col, 1, ty_v, &Type::String)?;
            this.b.ins().call(push, &[out, col]);
            Ok(())
        };

        let type_label = |ty: &Type| -> String {
            match ty {
                Type::String => "String".into(),
                Type::Float | Type::Float32 => "Float".into(),
                Type::Int | Type::IntN { .. } => "Int".into(),
                Type::Bool => "Bool".into(),
                Type::Named(n) => n.clone(),
                Type::Apply { name, args } if args.len() == 1 => {
                    format!("{name}<{}>", match &args[0] {
                        Type::Int | Type::IntN { .. } => "Int".into(),
                        Type::String => "String".into(),
                        Type::Float | Type::Float32 => "Float".into(),
                        Type::Bool => "Bool".into(),
                        Type::Named(n) => n.clone(),
                        other => format!("{other:?}"),
                    })
                }
                other => format!("{other:?}"),
            }
        };

        match &row_ty {
            Type::Named(type_name) => {
                let Some((names, tys)) = self.meta.struct_layout(type_name) else {
                    // Empty struct or unknown — zero columns.
                    let _ = table;
                    return Ok(out);
                };
                for (fname, fty) in names.iter().zip(tys.iter()) {
                    let bare = fname.strip_prefix("user_").unwrap_or(fname);
                    push_col(self, out, bare, &type_label(fty))?;
                }
            }
            Type::Apply { name, args } => {
                // Generic row (e.g. Box<Int>): expand registered fields when present.
                if let Some((names, tys)) = self.meta.struct_layout(name) {
                    for (fname, fty) in names.iter().zip(tys.iter()) {
                        let bare = fname.strip_prefix("user_").unwrap_or(fname);
                        // Substitute lone type-param fields with Apply args when possible.
                        let label = if matches!(fty, Type::Named(n) if n.starts_with('T') || n == "T")
                            && args.len() == 1
                        {
                            type_label(&args[0])
                        } else if names.len() == 1 && args.len() == 1 {
                            // Common single-field generic wrapper: field type is the arg.
                            type_label(&args[0])
                        } else {
                            type_label(fty)
                        };
                        push_col(self, out, bare, &label)?;
                    }
                } else if name == "Box" && args.len() == 1 {
                    push_col(self, out, "value", &type_label(&args[0]))?;
                } else {
                    push_col(self, out, "value", &type_label(&row_ty))?;
                }
            }
            other => {
                push_col(self, out, "value", &type_label(other))?;
            }
        }
        let _ = table;
        Ok(out)
    }

    /// Extract key (+ optional float value) columns via lambdas, then host-reduce.
    fn lower_data_group(
        &mut self,
        rows: &TExpr,
        key_fn: &TExpr,
        value_fn: Option<&TExpr>,
        mode: i64,
    ) -> Result<Value, String> {
        let keys = self.lower_iter_map_filter(rows, std::slice::from_ref(key_fn), false)?;
        let values = if let Some(vf) = value_fn {
            // Map may yield Float — push via float-capable list path by
            // bit-casting through the same list_push (stores raw i64 bits).
            self.lower_iter_map_filter(rows, std::slice::from_ref(vf), false)?
        } else {
            let host = self
                .module
                .declare_func_in_func(self.host.coll.list_new, self.b.func);
            let call = self.b.ins().call(host, &[]);
            self.b.inst_results(call)[0]
        };
        let mode_v = self.b.ins().iconst(types::I64, mode);
        let host = self
            .module
            .declare_func_in_func(self.host.data.group_reduce, self.b.func);
        let call = self.b.ins().call(host, &[keys, values, mode_v]);
        Ok(self.b.inst_results(call)[0])
    }

    fn lower_data_filter(&mut self, rows: &TExpr, pred: &TExpr) -> Result<Value, String> {
        // Reuse list filter loop for Named/Int/String elems.
        let fake_recv = rows;
        self.lower_iter_map_filter(fake_recv, std::slice::from_ref(pred), true)
    }

    fn lower_data_sort_by(
        &mut self,
        rows: &TExpr,
        key: &TExpr,
        _ty: &Type,
    ) -> Result<Value, String> {
        let list = self.lower_expr(rows)?;
        let keys = self.lower_iter_map_filter(rows, std::slice::from_ref(key), false)?;
        let key_is_str = match &key.kind {
            TExprKind::Lambda(lam) => matches!(lam.ret.as_ref(), Some(Type::String))
                || matches!(&lam.executable, jet_codegen::Codegen::TIR::TLambdaBody::Expr(e) if matches!(e.ty, Type::String)),
            _ => false,
        };
        let sort_id = if key_is_str {
            self.host.coll.list_sort_by_str_keys
        } else {
            self.host.coll.list_sort_by_i64_keys
        };
        let clone_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_clone, self.b.func);
        let cloned = {
            let call = self.b.ins().call(clone_ref, &[list]);
            self.b.inst_results(call)[0]
        };
        let host = self.module.declare_func_in_func(sort_id, self.b.func);
        self.b.ins().call(host, &[cloned, keys]);
        let ok = self.b.ins().iconst(types::I8, 1);
        let pack = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let packed = self.b.ins().call(pack, &[ok, cloned]);
        Ok(self.b.inst_results(packed)[0])
    }

    fn lower_host_call(&mut self, host: &THostCall, ty: &Type) -> Result<Value, String> {
        match host {
            THostCall::Method { recv, method, args }
                if matches!(&recv.ty, Type::Apply { name, .. } if name == "Pool") =>
            {
                let handle = self.lower_expr(recv)?;
                let host_id = match method.as_str() {
                    "add" if args.len() == 1 => self.host.memory.pool_add,
                    "remove" if args.len() == 1 => self.host.memory.pool_remove,
                    "ids" if args.is_empty() => self.host.memory.pool_ids,
                    _ => return Err(format!("jit Pool method unsupported: {method}")),
                };
                let mut values = vec![handle];
                for arg in args {
                    values.push(self.lower_expr(arg)?);
                }
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host_ref, &values);
                Ok(self.b.inst_results(call)[0])
            }
            THostCall::Method { recv, method, args }
                if matches!(&recv.ty, Type::Shared(_))
                    && matches!(method.as_str(), "read" | "edit" | "edit_txn")
                    && args.len() == 1 =>
            {
                let handle = self.lower_expr(recv)?;
                let transactional = method == "edit_txn" && self.in_shared_transaction;
                let begin_id = if transactional {
                    self.host.memory.shared_txn_get
                } else {
                    self.host.memory.shared_begin
                };
                let begin = self.module.declare_func_in_func(begin_id, self.b.func);
                let call = self.b.ins().call(begin, &[handle]);
                let payload = self.b.inst_results(call)[0];
                let TExprKind::Lambda(lambda) = &args[0].kind else {
                    return Err("jit Shared callback must be a lambda".to_string());
                };
                let result = self.lower_inline_lambda(lambda, payload)?;
                let host_id = if transactional {
                    self.host.memory.shared_txn_set
                } else if method == "read" {
                    self.host.memory.shared_end_read
                } else {
                    self.host.memory.shared_end_write
                };
                let end = self.module.declare_func_in_func(host_id, self.b.func);
                if method == "read" {
                    self.b.ins().call(end, &[handle]);
                } else {
                    self.b.ins().call(end, &[handle, payload]);
                }
                Ok(result)
            }
            THostCall::Method { recv, method, args }
                if (matches!(&recv.ty, Type::Apply { name, .. } if name == "ExpiringSecret")
                    || matches!(
                        &recv.ty,
                        Type::Tagged { inner, .. }
                            if matches!(inner.as_ref(), Type::Apply { name, .. } if name == "ExpiringSecret")
                    ))
                    && method == "with"
                    && args.len() == 1 =>
            {
                let handle = self.lower_expr(recv)?;
                let zero_clock = self.b.ins().iconst(types::I64, 0);
                let get = self
                    .module
                    .declare_func_in_func(self.host.memory.expiring_get, self.b.func);
                let call = self.b.ins().call(get, &[handle, zero_clock]);
                let status = self.b.inst_results(call)[0];
                let zero = self.b.ins().iconst(types::I64, 0);
                let present = self.b.ins().icmp(IntCC::NotEqual, status, zero);
                let one = self.b.ins().iconst(types::I64, 1);
                let payload = self.b.ins().isub(status, one);
                let TExprKind::Lambda(lambda) = &args[0].kind else {
                    return Err("jit ExpiringSecret callback must be a lambda".to_string());
                };

                let available = self.b.create_block();
                let expired = self.b.create_block();
                let merge = self.b.create_block();
                self.b.append_block_param(merge, types::I64);
                self.b.ins().brif(present, available, &[], expired, &[]);

                self.b.switch_to_block(available);
                self.b.seal_block(available);
                let callback = self.lower_inline_lambda(lambda, payload)?;
                let callback_plus_one = self.b.ins().iadd(callback, one);
                self.b.ins().jump(merge, &[callback_plus_one]);

                self.b.switch_to_block(expired);
                self.b.seal_block(expired);
                self.b.ins().jump(merge, &[zero]);

                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                let packed = self.b.block_params(merge)[0];
                Ok(self.result_from_packed_i64(packed))
            }
            THostCall::YieldSend { value } => {
                let sender = self
                    .yield_sender
                    .ok_or("jit yield outside generator body")?;
                let value = self.lower_expr(value)?;
                if self.b.func.dfg.value_type(value) != types::I64 {
                    return Err("jit generator item type unsupported".to_string());
                }
                let send = self
                    .module
                    .declare_func_in_func(self.host.conc.sender_send, self.b.func);
                let call = self.b.ins().call(send, &[sender, value]);
                let sent = self.finish_wait_call(self.b.inst_results(call)[0]);
                let zero = self.b.ins().iconst(types::I64, 0);
                let closed = self.b.ins().icmp(IntCC::Equal, sent, zero);
                let stop = self.b.create_block();
                let resume = self.b.create_block();
                self.b.ins().brif(closed, stop, &[], resume, &[]);
                self.b.switch_to_block(stop);
                self.b.seal_block(stop);
                let close = self
                    .module
                    .declare_func_in_func(self.host.conc.sender_close, self.b.func);
                self.b.ins().call(close, &[sender]);
                let zero = self.b.ins().iconst(types::I64, 0);
                self.b.ins().return_(&[zero]);
                self.b.switch_to_block(resume);
                self.b.seal_block(resume);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THostCall::GcRead { root } => {
                let var = self
                    .vars
                    .get(root)
                    .copied()
                    .ok_or_else(|| format!("jit GcRead unknown root `{root}`"))?;
                Ok(self.b.use_var(var))
            }
            THostCall::GcEdit {
                root,
                edit,
                index_temp,
                ..
            } => {
                if let Some((temp, value)) = index_temp {
                    let v = self.lower_expr(value)?;
                    let vty = self.b.func.dfg.value_type(v);
                    let var = self.fresh_var(vty);
                    self.b.def_var(var, v);
                    self.vars.insert(temp.clone(), var);
                    self.var_tys.insert(temp.clone(), value.ty.clone());
                }
                let root_var = self
                    .vars
                    .get(root)
                    .copied()
                    .ok_or_else(|| format!("jit GcEdit unknown root `{root}`"))?;
                let cur = self.b.use_var(root_var);
                let jv = self.fresh_var(types::I64);
                self.b.def_var(jv, cur);
                self.vars.insert("__jet_value".to_string(), jv);
                self.var_tys.insert(
                    "__jet_value".to_string(),
                    self.var_tys.get(root).cloned().unwrap_or(Type::Int),
                );
                let result = self.lower_expr(edit)?;
                let new_val = self.b.use_var(jv);
                self.b.def_var(root_var, new_val);
                let _ = ty;
                Ok(result)
            }
            THostCall::Helper { helper, args } if helper.ends_with("jet_std_clock_new") => {
                let ms = match args.first() {
                    Some(THostArg::Expr(e) | THostArg::Borrow(e)) => self.lower_expr(e)?,
                    _ => return Err("jit Clock.new args unsupported".to_string()),
                };
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.clock_new, self.b.func);
                let call = self.b.ins().call(host_ref, &[ms]);
                Ok(self.b.inst_results(call)[0])
            }
            THostCall::EnvSet { name, value, .. } => {
                let name_v = self.lower_expr(name)?;
                let value_v = self.lower_expr(value)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.env_set, self.b.func);
                let call = self.b.ins().call(host_ref, &[name_v, value_v]);
                let _ = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                let _ = ty;
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THostCall::NumericBounds {
                ty: Type::Float,
                member,
            } => {
                let value = match member.as_str() {
                    "INFINITY" => f64::INFINITY,
                    "NAN" => f64::NAN,
                    "EPSILON" => f64::EPSILON,
                    _ => return Err("jit numeric bound unsupported".to_string()),
                };
                Ok(self.b.ins().f64const(value))
            }
            THostCall::NumericBounds { ty, member }
                if matches!(member.as_str(), "MIN" | "MAX") =>
            {
                let (signed, bits) =
                    jet_codegen::Comptime::MathLayout::integer_type_layout(ty)
                        .ok_or_else(|| "jit numeric bound unsupported".to_string())?;
                Ok(self.b.ins().iconst(
                    types::I64,
                    jet_codegen::Comptime::MathLayout::integer_bound(
                        signed,
                        bits,
                        member == "MAX",
                    ),
                ))
            }
            THostCall::ExpiringValueNew {
                value,
                duration,
                clock,
            } => {
                let value = self.lower_expr(value)?;
                let duration = self.lower_expr(duration)?;
                let clock = self.lower_expr(clock)?;
                let secret = self.b.ins().iconst(types::I64, 0);
                let host = self
                    .module
                    .declare_func_in_func(self.host.memory.expiring_new, self.b.func);
                let call = self
                    .b
                    .ins()
                    .call(host, &[value, duration, clock, secret]);
                Ok(self.b.inst_results(call)[0])
            }
            THostCall::ExpiringSecretNew {
                value,
                duration,
                clock,
                ..
            } => {
                let value = self.lower_expr(value)?;
                let duration = self.lower_expr(duration)?;
                let clock = self.lower_expr(clock)?;
                let secret = self.b.ins().iconst(types::I64, 1);
                let host = self
                    .module
                    .declare_func_in_func(self.host.memory.expiring_new, self.b.func);
                let call = self
                    .b
                    .ins()
                    .call(host, &[value, duration, clock, secret]);
                Ok(self.b.inst_results(call)[0])
            }
            THostCall::FixedListIndex { base, index } => {
                let list = self.lower_expr(base)?;
                let idx = self.lower_expr(index)?;
                let line = self.b.ins().iconst(types::I32, 1);
                let host_id = if self.meta.clif_ty(ty) == Some(types::F64) {
                    self.host.coll.list_get_f64
                } else {
                    self.host.coll.list_get
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host_ref, &[list, idx, line]);
                let value = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(value)
            }
            THostCall::TupleIndex { base, index } => {
                let handle = self.lower_expr(base)?;
                // Prefer the ordinal — named match tuples (`version`, `ihl`, …)
                // are packed in declaration order; looking up `user_<name>` in
                // struct_fields fails when the shape was not registered.
                let idx_val = self.b.ins().iconst(types::I64, *index as i64);
                let host_id = match ty {
                    t if Self::is_string_abi_ty(t) => self.host.struct_get_str,
                    Type::Int => self.host.struct_get_i64,
                    Type::Float => self.host.struct_get_f64,
                    Type::Bool => self.host.struct_get_bool,
                    Type::Char => self.host.struct_get_char,
                    other if clif_ty(other) == Some(types::I64) => self.host.struct_get_i64,
                    other => {
                        return Err(format!("jit tuple index field type unsupported: {other:?}"))
                    }
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host_ref, &[handle, idx_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THostCall::SwitchSubjectField { field } => {
                let (handle, subject_ty) = self
                    .switch_subject
                    .clone()
                    .ok_or("jit switch subject field outside switch")?;
                let type_name =
                    record_type_key(&subject_ty).ok_or("jit switch subject field type")?;
                self.lower_record_field(handle, &type_name, field, ty)
            }
            THostCall::Helper { helper, args } if helper.ends_with("jet_context") => {
                let recv = match args.first() {
                    Some(THostArg::Expr(e)) => self.lower_expr(e)?,
                    _ => return Err("jit jet_context missing result".to_string()),
                };
                let msg = match args.get(1) {
                    Some(THostArg::Expr(e)) => self.lower_expr(e)?,
                    _ => return Err("jit jet_context missing message".to_string()),
                };
                let host = self
                    .module
                    .declare_func_in_func(self.host.result_context, self.b.func);
                let call = self.b.ins().call(host, &[recv, msg]);
                Ok(self.b.inst_results(call)[0])
            }
            THostCall::TypedText { kind, arg } => {
                let val = self.lower_expr(arg)?;
                match kind {
                    TTypedTextForm::SqlRaw => {
                        let n = self.b.ins().iconst(types::I64, 2);
                        let new_ref = self
                            .module
                            .declare_func_in_func(self.host.struct_new, self.b.func);
                        let call = self.b.ins().call(new_ref, &[n]);
                        let rec = self.b.inst_results(call)[0];
                        let zero = self.b.ins().iconst(types::I64, 0);
                        let one = self.b.ins().iconst(types::I64, 1);
                        let set = self
                            .module
                            .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                        self.b.ins().call(set, &[rec, zero, val]);
                        let list_new = self
                            .module
                            .declare_func_in_func(self.host.coll.list_new, self.b.func);
                        let call = self.b.ins().call(list_new, &[]);
                        let empty = self.b.inst_results(call)[0];
                        self.b.ins().call(set, &[rec, one, empty]);
                        Ok(rec)
                    }
                    TTypedTextForm::SqlTemplate => {
                        let zero = self.b.ins().iconst(types::I64, 0);
                        let get = self
                            .module
                            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
                        let call = self.b.ins().call(get, &[val, zero]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    TTypedTextForm::SqlParams => {
                        let one = self.b.ins().iconst(types::I64, 1);
                        let get = self
                            .module
                            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
                        let call = self.b.ins().call(get, &[val, one]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    TTypedTextForm::HtmlRaw | TTypedTextForm::HtmlText | TTypedTextForm::ShRaw => {
                        Ok(val)
                    }
                }
            }
            THostCall::TypedTextInterp {
                kind: TTypedTextInterpKind::Sql,
                literals,
                holes,
            } => {
                let template = literals.join("?");
                let template_id = self.runtime.heap.alloc_string(template);
                let template_v = self.b.ins().iconst(types::I64, template_id);
                let list_new = self
                    .module
                    .declare_func_in_func(self.host.coll.list_new, self.b.func);
                let call = self.b.ins().call(list_new, &[]);
                let params = self.b.inst_results(call)[0];
                let push = self
                    .module
                    .declare_func_in_func(self.host.coll.list_push, self.b.func);
                for hole in holes {
                    let shown = self.lower_jet_show(hole)?;
                    self.b.ins().call(push, &[params, shown]);
                }
                let n = self.b.ins().iconst(types::I64, 2);
                let new_ref = self
                    .module
                    .declare_func_in_func(self.host.struct_new, self.b.func);
                let call = self.b.ins().call(new_ref, &[n]);
                let rec = self.b.inst_results(call)[0];
                let zero = self.b.ins().iconst(types::I64, 0);
                let one = self.b.ins().iconst(types::I64, 1);
                let set = self
                    .module
                    .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                self.b.ins().call(set, &[rec, zero, template_v]);
                self.b.ins().call(set, &[rec, one, params]);
                Ok(rec)
            }
            THostCall::TypedTextInterp { kind, .. } => {
                let label = match kind {
                    TTypedTextInterpKind::Sql => "Sql",
                    TTypedTextInterpKind::Html => "Html",
                    TTypedTextInterpKind::Sh => "Sh",
                };
                Err(format!("jit TypedTextInterp unsupported: {label}"))
            }
            THostCall::Helper { helper, .. } => {
                Err(format!("jit helper unsupported: {helper}"))
            }
            THostCall::StrMatchScan { parts, probe } => {
                let (subject, _) = self
                    .switch_subject
                    .clone()
                    .ok_or("jit StrMatchScan outside switch")?;
                let pid = crate::Parse::install_str_pattern(parts.clone());
                let pid_v = self.b.ins().iconst(types::I64, pid);
                match probe {
                    TIR::TMatchProbe::IsSome => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.parse.str_match_is_some, self.b.func);
                        let call = self.b.ins().call(host, &[subject, pid_v]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    TIR::TMatchProbe::Unwrap => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.parse.str_match_unwrap, self.b.func);
                        let call = self.b.ins().call(host, &[subject, pid_v]);
                        Ok(self.b.inst_results(call)[0])
                    }
                }
            }
            THostCall::BinMatchScan { parts, probe } => {
                let (subject, _) = self
                    .switch_subject
                    .clone()
                    .ok_or("jit BinMatchScan outside switch")?;
                let pid = crate::Parse::install_bin_pattern(parts.clone());
                let pid_v = self.b.ins().iconst(types::I64, pid);
                match probe {
                    TIR::TMatchProbe::IsSome => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.parse.bin_match_is_some, self.b.func);
                        let call = self.b.ins().call(host, &[subject, pid_v]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    TIR::TMatchProbe::Unwrap => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.parse.bin_match_unwrap, self.b.func);
                        let call = self.b.ins().call(host, &[subject, pid_v]);
                        Ok(self.b.inst_results(call)[0])
                    }
                }
            }
            THostCall::Method { method, .. } => {
                Err(format!("jit host method unsupported: {method}"))
            }
            _ => Err("jit host call unsupported".to_string()),
        }
    }

    /// `jet_show` for Sql hole params — Int/String/Bool/Float cover checked Sql.
    fn lower_jet_show(&mut self, expr: &TExpr) -> Result<Value, String> {
        match &expr.ty {
            Type::String => self.lower_expr(expr),
            Type::Int | Type::IntN { .. } => {
                let v = self.lower_expr(expr)?;
                let signed = self.b.ins().iconst(types::I64, 1);
                let host = self
                    .module
                    .declare_func_in_func(self.host.intn_to_string, self.b.func);
                let call = self.b.ins().call(host, &[v, signed]);
                Ok(self.b.inst_results(call)[0])
            }
            Type::Bool => {
                let v = self.lower_expr(expr)?;
                let is_true = if self.b.func.dfg.value_type(v) == types::I8 {
                    let zero = self.b.ins().iconst(types::I8, 0);
                    self.b.ins().icmp(IntCC::NotEqual, v, zero)
                } else {
                    let zero = self.b.ins().iconst(types::I64, 0);
                    self.b.ins().icmp(IntCC::NotEqual, v, zero)
                };
                let t_id = self.runtime.heap.alloc_string("true".to_string());
                let f_id = self.runtime.heap.alloc_string("false".to_string());
                let t_v = self.b.ins().iconst(types::I64, t_id);
                let f_v = self.b.ins().iconst(types::I64, f_id);
                Ok(self.b.ins().select(is_true, t_v, f_v))
            }
            other => Err(format!("jit Sql hole show unsupported: {other:?}")),
        }
    }

    fn lower_inline_lambda(&mut self, lambda: &TLambda, argument: Value) -> Result<Value, String> {
        let name = lambda
            .source_params
            .first()
            .ok_or("jit inline callback missing parameter")?;
        let ty = lambda
            .param_types
            .first()
            .cloned()
            .ok_or("jit inline callback missing parameter type")?;
        let key = TIR::local_place(name);
        let old_var = self.vars.get(&key).copied();
        let old_ty = self.var_tys.get(&key).cloned();
        let expected = self
            .meta
            .clif_ty(&ty)
            .ok_or_else(|| format!("jit inline callback parameter unsupported: {ty:?}"))?;
        let actual = self.b.func.dfg.value_type(argument);
        if actual != expected {
            return Err(format!(
                "jit inline callback parameter ABI mismatch: {ty:?} expects {expected}, got {actual}"
            ));
        }
        let var = self.fresh_var(expected);
        self.b.def_var(var, argument);
        self.vars.insert(key.clone(), var);
        self.var_tys.insert(key.clone(), ty);
        let result = match &lambda.executable {
            TLambdaBody::Expr(expr) => self.lower_expr(expr)?,
            TLambdaBody::Block(body) => {
                self.lower_stmts_scoped(body)?;
                if self.dead {
                    return Err("jit inline callback cannot transfer control".to_string());
                }
                self.b.ins().iconst(types::I64, 0)
            }
        };
        match old_var {
            Some(var) => {
                self.vars.insert(key.clone(), var);
            }
            None => {
                self.vars.remove(&key);
            }
        }
        match old_ty {
            Some(ty) => {
                self.var_tys.insert(key, ty);
            }
            None => {
                self.var_tys.remove(&key);
            }
        }
        Ok(result)
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
        let rhs_ty = self.erase_distinct_ty(rhs_ty);
        if let Type::IntN { signed, bits } = rhs_ty {
            return self.lower_intn_values(
                op,
                INTN_MODE_TRAP,
                current,
                rhs,
                signed,
                bits,
                signed,
            );
        }
        Ok(match (op, &rhs_ty) {
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

    /// `TCallArg` wrappers: `arc_clone` / `fn_coerce` stay unsupported.
    /// `borrow` / `mut_borrow` pass the same heap handle (JIT mutates in place).
    /// `clone` lowers through `lower_clone` (structs/lists/strings — no silent drop).
    /// `widen_to_vec` is `[T#N]` → `[T]`: both are arena list handles.
    fn lower_call_arg(&mut self, arg: &TCallArg) -> Result<Value, String> {
        if arg.arc_clone {
            return Err("jit call arg wrapper unsupported".to_string());
        }
        let ty = &arg.value.ty;
        let handle_pass = jit_value_type(ty)
            || jit_struct_type(ty)
            || jit_tuple_type(ty)
            || matches!(
                ty,
                Type::String
                    | Type::List(_)
                    | Type::FixedList { .. }
                    | Type::Option(_)
                    | Type::Map { .. }
            );
        if (arg.borrow || arg.mut_borrow) && !handle_pass {
            return Err("jit call arg borrow unsupported".to_string());
        }
        let val = if arg.clone {
            self.lower_clone(&arg.value)?
        } else {
            self.lower_expr(&arg.value)?
        };
        if let Some(Type::Union(members)) = &arg.widen_to_union {
            let enum_name = jet_codegen::AST::union_enum_name(members);
            let variant = jet_codegen::AST::union_member_tag(ty);
            let disc = self
                .meta
                .enum_variant_index(&enum_name, &variant)
                .ok_or_else(|| format!("jit union arg `{enum_name}::{variant}`"))?;
            return self.pack_enum_scalar(disc, val, ty);
        }
        if arg.widen_to_vec {
            let host_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        Ok(val)
    }

    fn lower_fn_call(
        &mut self,
        callee: Value,
        fn_ty: &Type,
        args: &[TCallArg],
    ) -> Result<Value, String> {
        let signature = fn_value_signature(self.module, fn_ty, self.meta)?;
        let sig_ref = self.b.import_signature(signature);
        let values: Result<Vec<_>, _> =
            args.iter().map(|arg| self.lower_call_arg(arg)).collect();
        let call = self.b.ins().call_indirect(sig_ref, callee, &values?);
        let result = match fn_ty {
            Type::Fn { ret: Some(ret), .. } if self.meta.clif_ty(ret).is_some() => {
                Some(self.b.inst_results(call)[0])
            }
            _ => None,
        };
        self.emit_trap_check()?;
        Ok(result.unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
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
                TStrPart::Interp(e, fmt) => {
                    let push_ty = Self::recover_core_return_ty(e)
                        .unwrap_or_else(|| {
                            let erased = self.erase_distinct_ty(&e.ty);
                            if matches!(&erased, Type::Named(n) if n == "Unit" || n == "Void") {
                                self.print_result_ty(e)
                            } else {
                                erased
                            }
                        });
                    if matches!(&push_ty, Type::Named(n) if n == "Unit" || n == "Void") {
                        continue;
                    }
                    if let Type::Named(type_name) = &push_ty {
                        self.lower_named_str_interp(buf_id, e, type_name, *fmt)?;
                        continue;
                    }
                    if let Type::Apply { name, .. } = &push_ty {
                        if name == "KeyRef" && matches!(fmt, StrFormat::Display) {
                            let recv = self.lower_expr(e)?;
                            let host_ref = self.module.declare_func_in_func(
                                self.host.crypto.vault_key_ref_show,
                                self.b.func,
                            );
                            let call = self.b.ins().call(host_ref, &[recv]);
                            let text = self.b.inst_results(call)[0];
                            let push_ref = self
                                .module
                                .declare_func_in_func(self.host.str_push_str, self.b.func);
                            self.b.ins().call(push_ref, &[buf_id, text]);
                            continue;
                        }
                    }
                    let val = self.lower_expr(e)?;
                    if let Type::Option(inner) = &push_ty {
                        let zero = self.b.ins().iconst(types::I64, 0);
                        let is_none = self.bool_from_icmp(IntCC::Equal, val, zero);
                        let none_block = self.b.create_block();
                        let some_block = self.b.create_block();
                        let done = self.b.create_block();
                        self.b
                            .ins()
                            .brif(is_none, none_block, &[], some_block, &[]);
                        self.b.switch_to_block(none_block);
                        self.b.seal_block(none_block);
                        let none_id = self.runtime.heap.alloc_string("None".to_string());
                        let none_id = self.b.ins().iconst(types::I64, none_id);
                        let push_none = self
                            .module
                            .declare_func_in_func(self.host.str_push_lit, self.b.func);
                        self.b.ins().call(push_none, &[buf_id, none_id]);
                        self.b.ins().jump(done, &[]);
                        self.b.switch_to_block(some_block);
                        self.b.seal_block(some_block);
                        let payload = self.unpack_option_payload(val, inner)?;
                        let push = match inner.as_ref() {
                            Type::Int => self.host.str_push_i64,
                            Type::Float => self.host.str_push_f64,
                            Type::Bool => self.host.str_push_bool,
                            Type::Char => self.host.str_push_char,
                            Type::String => self.host.str_push_str,
                            other => {
                                return Err(format!(
                                    "jit string interp type unsupported: Option({other:?})"
                                ));
                            }
                        };
                        let push = self.module.declare_func_in_func(push, self.b.func);
                        self.b.ins().call(push, &[buf_id, payload]);
                        self.b.ins().jump(done, &[]);
                        self.b.switch_to_block(done);
                        self.b.seal_block(done);
                        continue;
                    }
                    if let Some(elem) = jit_list_iter_elem_type(&push_ty).or_else(|| {
                        match &push_ty {
                            Type::List(inner)
                                if matches!(inner.as_ref(), Type::Int | Type::String) =>
                            {
                                Some(inner.as_ref().clone())
                            }
                            _ => None,
                        }
                    }) {
                        let kind = match elem {
                            Type::String => 1,
                            Type::IntN { signed: true, .. } => 2,
                            Type::IntN { signed: false, .. } => 3,
                            _ => 0,
                        };
                        let flag = self
                            .b
                            .ins()
                            .iconst(types::I64, kind);
                        let show_ref = self
                            .module
                            .declare_func_in_func(self.host.coll.list_show, self.b.func);
                        let show_call = self.b.ins().call(show_ref, &[val, flag]);
                        let text = self.b.inst_results(show_call)[0];
                        let push_ref = self
                            .module
                            .declare_func_in_func(self.host.str_push_str, self.b.func);
                        self.b.ins().call(push_ref, &[buf_id, text]);
                        continue;
                    }
                    if let Type::IntN { signed, .. } = &push_ty {
                        let signed = self
                            .b
                            .ins()
                            .iconst(types::I64, i64::from(*signed));
                        let show = self
                            .module
                            .declare_func_in_func(self.host.intn_to_string, self.b.func);
                        let call = self.b.ins().call(show, &[val, signed]);
                        let text = self.b.inst_results(call)[0];
                        let push = self
                            .module
                            .declare_func_in_func(self.host.str_push_str, self.b.func);
                        self.b.ins().call(push, &[buf_id, text]);
                        continue;
                    }
                    let host_id = match &push_ty {
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
                    self.b.ins().call(host_ref, &[buf_id, val]);
                }
            }
        }
        Ok(buf_id)
    }

    /// `{named}` / `{named#Debug}` — Display prefers `Type::display` when compiled;
    /// otherwise JetShow-style mangled Debug (`user_Type { user_field: … }`).
    /// Debug format uses unmangled JetDebug shape (`Type { field: … }`) and
    /// `#[Redact]` → `[redacted]` when bundle metadata is installed.
    fn lower_named_str_interp(
        &mut self,
        buf_id: Value,
        expr: &TExpr,
        type_name: &str,
        fmt: StrFormat,
    ) -> Result<(), String> {
        if matches!(fmt, StrFormat::Display) {
            if type_name == "EncodingError" {
                let recv = self.lower_expr(expr)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.encoding_error_show, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv]);
                let text = self.b.inst_results(call)[0];
                let push_ref = self
                    .module
                    .declare_func_in_func(self.host.str_push_str, self.b.func);
                self.b.ins().call(push_ref, &[buf_id, text]);
                return Ok(());
            }
            if matches!(type_name, "GameImage" | "GameSound") {
                let recv = self.lower_expr(expr)?;
                let kind = self
                    .b
                    .ins()
                    .iconst(types::I64, i64::from(type_name == "GameSound"));
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.asset_show, self.b.func);
                let call = self.b.ins().call(host, &[kind, recv]);
                let text = self.b.inst_results(call)[0];
                let push_ref = self
                    .module
                    .declare_func_in_func(self.host.str_push_str, self.b.func);
                self.b.ins().call(push_ref, &[buf_id, text]);
                return Ok(());
            }
            if type_name == "DataError" {
                let recv = self.lower_expr(expr)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.data.error_show, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv]);
                let text = self.b.inst_results(call)[0];
                let push_ref = self
                    .module
                    .declare_func_in_func(self.host.str_push_str, self.b.func);
                self.b.ins().call(push_ref, &[buf_id, text]);
                return Ok(());
            }
            let display_key = format!("{type_name}::display");
            if let Some(&func_id) = self.func_ids.get(&display_key) {
                let recv = self.lower_expr(expr)?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &[recv]);
                let text = self.b.inst_results(call)[0];
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_push_str, self.b.func);
                self.b.ins().call(host_ref, &[buf_id, text]);
                return Ok(());
            }
        }
        if matches!(fmt, StrFormat::Debug) && matches!(type_name, "GameImage" | "GameSound") {
            let recv = self.lower_expr(expr)?;
            let kind = self
                .b
                .ins()
                .iconst(types::I64, i64::from(type_name == "GameSound"));
            let host = self
                .module
                .declare_func_in_func(self.host.game.asset_show, self.b.func);
            let call = self.b.ins().call(host, &[kind, recv]);
            let text = self.b.inst_results(call)[0];
            let push_ref = self
                .module
                .declare_func_in_func(self.host.str_push_str, self.b.func);
            self.b.ins().call(push_ref, &[buf_id, text]);
            return Ok(());
        }
        let (field_names, field_tys) = self.meta.struct_layout(type_name).ok_or_else(|| {
            format!("jit string interp type unsupported: Named({type_name:?})")
        })?;
        // JetDebug needs #[Redact] metadata from the ProgramBundle. Refuse when
        // missing rather than leak secrets (Display/JetShow path above is fine).
        if matches!(fmt, StrFormat::Debug)
            && !field_names.is_empty()
            && super::types_meta::struct_field_redacted(type_name, 0).is_none()
        {
            return Err(format!(
                "jit string interp Debug type unsupported: Named({type_name:?})"
            ));
        }
        let handle = self.lower_expr(expr)?;
        let mangled_show = matches!(fmt, StrFormat::Display);
        let head = if mangled_show {
            format!("user_{type_name} {{ ")
        } else {
            format!("{type_name} {{ ")
        };
        self.push_str_lit(buf_id, &head)?;
        for (i, (fname, fty)) in field_names.iter().zip(field_tys.iter()).enumerate() {
            if i > 0 {
                self.push_str_lit(buf_id, ", ")?;
            }
            let label = if mangled_show {
                fname.clone()
            } else {
                fname
                    .strip_prefix("user_")
                    .unwrap_or(fname.as_str())
                    .to_string()
            };
            self.push_str_lit(buf_id, &format!("{label}: "))?;
            if matches!(fmt, StrFormat::Debug)
                && super::types_meta::struct_field_redacted(type_name, i) == Some(true)
            {
                self.push_str_lit(buf_id, "[redacted]")?;
                continue;
            }
            let idx = self.b.ins().iconst(types::I64, i as i64);
            match fty {
                Type::Int => {
                    let get = self
                        .module
                        .declare_func_in_func(self.host.struct_get_i64, self.b.func);
                    let call = self.b.ins().call(get, &[handle, idx]);
                    let val = self.b.inst_results(call)[0];
                    let push = self
                        .module
                        .declare_func_in_func(self.host.str_push_i64, self.b.func);
                    self.b.ins().call(push, &[buf_id, val]);
                }
                Type::Float => {
                    let get = self
                        .module
                        .declare_func_in_func(self.host.struct_get_f64, self.b.func);
                    let call = self.b.ins().call(get, &[handle, idx]);
                    let val = self.b.inst_results(call)[0];
                    let push = self
                        .module
                        .declare_func_in_func(self.host.str_push_f64, self.b.func);
                    self.b.ins().call(push, &[buf_id, val]);
                }
                Type::Bool => {
                    let get = self
                        .module
                        .declare_func_in_func(self.host.struct_get_bool, self.b.func);
                    let call = self.b.ins().call(get, &[handle, idx]);
                    let val = self.b.inst_results(call)[0];
                    let push = self
                        .module
                        .declare_func_in_func(self.host.str_push_bool, self.b.func);
                    self.b.ins().call(push, &[buf_id, val]);
                }
                Type::Char => {
                    let get = self
                        .module
                        .declare_func_in_func(self.host.struct_get_char, self.b.func);
                    let call = self.b.ins().call(get, &[handle, idx]);
                    let val = self.b.inst_results(call)[0];
                    let push = self
                        .module
                        .declare_func_in_func(self.host.str_push_char, self.b.func);
                    self.b.ins().call(push, &[buf_id, val]);
                }
                Type::String => {
                    // Rust Debug quotes string fields; JetShow/`{:?}` matches.
                    self.push_str_lit(buf_id, "\"")?;
                    let get = self
                        .module
                        .declare_func_in_func(self.host.struct_get_str, self.b.func);
                    let call = self.b.ins().call(get, &[handle, idx]);
                    let val = self.b.inst_results(call)[0];
                    let push = self
                        .module
                        .declare_func_in_func(self.host.str_push_str, self.b.func);
                    self.b.ins().call(push, &[buf_id, val]);
                    self.push_str_lit(buf_id, "\"")?;
                }
                other => {
                    return Err(format!(
                        "jit string interp named field unsupported: {type_name}.{fname}: {other:?}"
                    ));
                }
            }
        }
        self.push_str_lit(buf_id, " }")?;
        Ok(())
    }

    fn push_str_lit(&mut self, buf_id: Value, text: &str) -> Result<(), String> {
        let lit_id = self.runtime.heap.alloc_string(text.to_string());
        let lit_const = self.b.ins().iconst(types::I64, lit_id);
        let host_ref = self
            .module
            .declare_func_in_func(self.host.str_push_lit, self.b.func);
        self.b.ins().call(host_ref, &[buf_id, lit_const]);
        Ok(())
    }

    /// Variable-map key for a local slot. User bindings live under their mangled
    /// Rust spelling (`user_x`); generated temps keep their reserved names.
    fn local_key(local: &TLocal) -> String {
        local.rust_name()
    }

    fn is_view_mut_ty(ty: &Type) -> bool {
        matches!(ty, Type::Apply { name, .. } if name == "ViewMut")
    }

    fn is_string_abi_ty(ty: &Type) -> bool {
        matches!(ty, Type::String)
            || matches!(
                ty,
                Type::Apply { name, args }
                    if name == "View"
                        && args.len() == 1
                        && (matches!(&args[0], Type::String)
                            || matches!(&args[0], Type::Named(name) if name == "str"))
            )
    }

    fn is_field_mut_ty(ty: &Type) -> bool {
        matches!(ty, Type::Apply { name, .. } if name == "__JetFieldMut")
    }

    /// Write-through window: heap record `[list, start, end]` (inclusive ends).
    fn emit_view_mut_window(
        &mut self,
        list: Value,
        start: Value,
        end: Value,
    ) -> Result<Value, String> {
        let n = self.b.ins().iconst(types::I64, 3);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let call = self.b.ins().call(new_ref, &[n]);
        let handle = self.b.inst_results(call)[0];
        let set = self
            .module
            .declare_func_in_func(self.host.struct_set_i64, self.b.func);
        let zero = self.b.ins().iconst(types::I64, 0);
        let one = self.b.ins().iconst(types::I64, 1);
        let two = self.b.ins().iconst(types::I64, 2);
        self.b.ins().call(set, &[handle, zero, list]);
        self.b.ins().call(set, &[handle, one, start]);
        self.b.ins().call(set, &[handle, two, end]);
        Ok(handle)
    }

    fn unpack_view_mut(&mut self, handle: Value) -> Result<(Value, Value, Value), String> {
        let get = self
            .module
            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
        let zero = self.b.ins().iconst(types::I64, 0);
        let one = self.b.ins().iconst(types::I64, 1);
        let two = self.b.ins().iconst(types::I64, 2);
        let call0 = self.b.ins().call(get, &[handle, zero]);
        let list = self.b.inst_results(call0)[0];
        let call1 = self.b.ins().call(get, &[handle, one]);
        let start = self.b.inst_results(call1)[0];
        let call2 = self.b.ins().call(get, &[handle, two]);
        let end = self.b.inst_results(call2)[0];
        Ok((list, start, end))
    }

    /// Write-through field place: heap record `[struct, field_index]`.
    fn emit_field_mut(&mut self, structure: Value, field_idx: i64) -> Result<Value, String> {
        let n = self.b.ins().iconst(types::I64, 2);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let call = self.b.ins().call(new_ref, &[n]);
        let handle = self.b.inst_results(call)[0];
        let set = self
            .module
            .declare_func_in_func(self.host.struct_set_i64, self.b.func);
        let zero = self.b.ins().iconst(types::I64, 0);
        let one = self.b.ins().iconst(types::I64, 1);
        let idx = self.b.ins().iconst(types::I64, field_idx);
        self.b.ins().call(set, &[handle, zero, structure]);
        self.b.ins().call(set, &[handle, one, idx]);
        Ok(handle)
    }

    fn unpack_field_mut(&mut self, handle: Value) -> Result<(Value, Value), String> {
        let get = self
            .module
            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
        let zero = self.b.ins().iconst(types::I64, 0);
        let one = self.b.ins().iconst(types::I64, 1);
        let call0 = self.b.ins().call(get, &[handle, zero]);
        let structure = self.b.inst_results(call0)[0];
        let call1 = self.b.ins().call(get, &[handle, one]);
        let idx = self.b.inst_results(call1)[0];
        Ok((structure, idx))
    }

    fn load_local(&mut self, local: &TLocal) -> Result<Value, String> {
        let key = Self::local_key(local);
        let var = self
            .vars
            .get(&key)
            .copied()
            .ok_or_else(|| format!("jit unknown local `{}`", local.name))?;
        let raw = if let Some(slot) = self.raw_slots.get(&key).copied() {
            let ty = self
                .var_tys
                .get(&key)
                .and_then(|ty| self.meta.clif_ty(ty))
                .ok_or("jit spilled local type")?;
            self.b.ins().stack_load(ty, slot, 0)
        } else {
            self.b.use_var(var)
        };
        if !local.deref {
            return Ok(raw);
        }
        let ty = self.var_tys.get(&key).cloned();
        let view_elem_ty = match ty.as_ref() {
            Some(Type::Apply { name, args }) if name == "ViewMut" && args.len() == 1 => {
                Some(&args[0])
            }
            _ => None,
        };
        if let Some(elem_ty) = view_elem_ty {
            let (list, start, _) = self.unpack_view_mut(raw)?;
            let line = self.b.ins().iconst(types::I32, 1);
            let host_id = match elem_ty {
                Type::Float => self.host.coll.list_get_f64,
                _ => self.host.coll.list_get,
            };
            let get = self
                .module
                .declare_func_in_func(host_id, self.b.func);
            let call = self.b.ins().call(get, &[list, start, line]);
            let result = self.b.inst_results(call)[0];
            self.emit_trap_check()?;
            return Ok(result);
        }
        if let Some(Type::Apply { name, args }) = ty.as_ref() {
            if name == "__JetScalarMut" && args.len() == 1 {
                let clif = self
                    .meta
                    .clif_ty(&args[0])
                    .ok_or("jit writable scalar type")?;
                return Ok(self
                    .b
                    .ins()
                    .load(clif, MemFlags::trusted(), raw, 0));
            }
        }
        if ty.as_ref().is_some_and(Self::is_field_mut_ty) {
            let (structure, idx) = self.unpack_field_mut(raw)?;
            let get = self
                .module
                .declare_func_in_func(self.host.struct_get_i64, self.b.func);
            let call = self.b.ins().call(get, &[structure, idx]);
            return Ok(self.b.inst_results(call)[0]);
        }
        Ok(raw)
    }

    /// `TExprKind::MethodCall`'s `func_ids` lookup key: JIT compiles user
    /// methods into plain functions named `Type::method`. The method's Jet
    /// name is already on `TMethodRef` — no Rust mangle stripping.
    fn method_key(&self, recv_ty: &Type, method: &TMethodRef) -> Option<String> {
        let base = user_type_name(recv_ty)?;
        if matches!(recv_ty, Type::Apply { .. }) {
            let concrete = format!("{}::{}", recv_ty.name(), method.name);
            if self.func_ids.contains_key(&concrete) {
                return Some(concrete);
            }
        }
        Some(format!("{}::{}", base, method.name))
    }

    fn require_raw_bag_key(&self, recv_ty: &Type) -> Result<(), String> {
        match recv_ty {
            Type::Apply { name, args }
                if name == "Bag"
                    && args.len() == 1
                    && self.meta.raw_bag_key_type(&args[0]) =>
            {
                Ok(())
            }
            _ => Err(format!("jit Bag key type unsupported: {recv_ty:?}")),
        }
    }

    fn lower_trait_object_method(
        &mut self,
        recv: &TExpr,
        method: &TMethodRef,
        args: &[TCallArg],
        ret_ty: &Type,
    ) -> Result<Value, String> {
        let object = self.lower_expr(recv)?;
        let type_id = self.record_slot(object, 0, &Type::Int)?;
        let concrete = self.record_slot(object, 1, &Type::Int)?;
        let arg_values: Result<Vec<_>, _> =
            args.iter().map(|arg| self.lower_call_arg(arg)).collect();
        let arg_values = arg_values?;
        let trait_name = method
            .trait_owner
            .as_deref()
            .ok_or_else(|| format!("jit dynamic method `{}` has no trait owner", method.name))?;
        let mut candidates: Vec<(i64, FuncId)> = self
            .meta
            .trait_method_owners(trait_name, &method.name)
            .into_iter()
            .filter_map(|owner| {
                let func = self.func_ids.get(&format!("{owner}::{}", method.name))?;
                Some((self.meta.struct_type_id(owner)?, *func))
            })
            .collect();
        candidates.sort_unstable_by_key(|(id, _)| *id);
        candidates.dedup_by_key(|(id, _)| *id);
        if candidates.is_empty() {
            return Err(format!("jit missing dynamic method `{}`", method.name));
        }
        let merge = self.b.create_block();
        let ret_clif = self.meta.clif_ty(ret_ty).or_else(|| clif_ty(ret_ty));
        if let Some(ret_clif) = ret_clif {
            self.b.append_block_param(merge, ret_clif);
        }
        for (candidate_id, func_id) in candidates {
            let arm = self.b.create_block();
            let next = self.b.create_block();
            let expected = self.b.ins().iconst(types::I64, candidate_id);
            let matched = self.bool_from_icmp(IntCC::Equal, type_id, expected);
            self.b.ins().brif(matched, arm, &[], next, &[]);
            self.b.switch_to_block(arm);
            self.b.seal_block(arm);
            let mut values = Vec::with_capacity(arg_values.len() + 1);
            values.push(concrete);
            values.extend_from_slice(&arg_values);
            let func = self.module.declare_func_in_func(func_id, self.b.func);
            let call = self.b.ins().call(func, &values);
            let result = ret_clif.map(|_| self.b.inst_results(call)[0]);
            self.emit_trap_check()?;
            match result {
                Some(result) => {
                    self.b.ins().jump(merge, &[result]);
                }
                None => {
                    self.b.ins().jump(merge, &[]);
                }
            }
            self.b.switch_to_block(next);
            self.b.seal_block(next);
        }
        self.b.ins().trap(TrapCode::UnreachableCodeReached);
        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(ret_clif
            .map(|_| self.b.block_params(merge)[0])
            .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)))
    }

    fn static_method_key(owner: &TStaticOwner, owner_type: Option<&Type>, method: &TMethodRef) -> Option<String> {
        let type_name = match owner {
            TStaticOwner::User(name) => name.as_str(),
            TStaticOwner::Prelude { .. } => return None,
        };
        Some(format!(
            "{}::{}",
            owner_type.map_or_else(|| type_name.to_string(), Type::name),
            method.name
        ))
    }

    fn new_record(&mut self, field_count: usize) -> Value {
        let count = self.b.ins().iconst(types::I64, field_count as i64);
        let host = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let call = self.b.ins().call(host, &[count]);
        self.b.inst_results(call)[0]
    }

    fn record_slot(&mut self, handle: Value, index: usize, ty: &Type) -> Result<Value, String> {
        let index = self.b.ins().iconst(types::I64, index as i64);
        let host = match ty {
            Type::String => self.host.struct_get_str,
            _ => match self.meta.clif_ty(ty).or_else(|| clif_ty(ty)) {
            Some(kind) if kind == types::F64 => self.host.struct_get_f64,
            Some(kind) if kind == types::I8 => self.host.struct_get_bool,
            Some(kind) if kind == types::I32 => self.host.struct_get_char,
            Some(kind) if kind == types::I64 => self.host.struct_get_i64,
            other => return Err(format!("jit patch field type unsupported: {ty:?} ({other:?})")),
            },
        };
        let host = self.module.declare_func_in_func(host, self.b.func);
        let call = self.b.ins().call(host, &[handle, index]);
        Ok(self.b.inst_results(call)[0])
    }

    fn set_record_slot(
        &mut self,
        handle: Value,
        index: usize,
        value: Value,
        ty: &Type,
    ) -> Result<(), String> {
        let index = self.b.ins().iconst(types::I64, index as i64);
        let host = match ty {
            Type::String => self.host.struct_set_str,
            _ => match self.meta.clif_ty(ty).or_else(|| clif_ty(ty)) {
            Some(kind) if kind == types::F64 => self.host.struct_set_f64,
            Some(kind) if kind == types::I8 => self.host.struct_set_bool,
            Some(kind) if kind == types::I32 => self.host.struct_set_char,
            Some(kind) if kind == types::I64 => self.host.struct_set_i64,
            other => return Err(format!("jit patch field type unsupported: {ty:?} ({other:?})")),
            },
        };
        let host = self.module.declare_func_in_func(host, self.b.func);
        self.b.ins().call(host, &[handle, index, value]);
        Ok(())
    }

    fn lower_patch_apply(&mut self, recv: &TExpr, patch: &TCallArg) -> Result<Value, String> {
        let base = record_type_key(&recv.ty).ok_or("jit patch apply receiver")?;
        let field_types = self
            .meta
            .struct_layout(&base)
            .ok_or_else(|| format!("jit patch base `{base}`"))?
            .1
            .to_vec();
        let base_value = self.lower_expr(recv)?;
        let patch_value = self.lower_call_arg(patch)?;
        let out = self.new_record(field_types.len());
        for (index, field_ty) in field_types.iter().enumerate() {
            let option_ty = Type::Option(Box::new(field_ty.clone()));
            let packed = self.record_slot(patch_value, index, &option_ty)?;
            let zero = self.b.ins().iconst(types::I64, 0);
            let missing = self.bool_from_icmp(IntCC::Equal, packed, zero);
            let old_block = self.b.create_block();
            let patch_block = self.b.create_block();
            let merge = self.b.create_block();
            let clif = self
                .meta
                .clif_ty(field_ty)
                .or_else(|| clif_ty(field_ty))
                .ok_or_else(|| format!("jit patch field type unsupported: {field_ty:?}"))?;
            self.b.append_block_param(merge, clif);
            self.b
                .ins()
                .brif(missing, old_block, &[], patch_block, &[]);
            self.b.switch_to_block(old_block);
            self.b.seal_block(old_block);
            let old = self.record_slot(base_value, index, field_ty)?;
            self.b.ins().jump(merge, &[old]);
            self.b.switch_to_block(patch_block);
            self.b.seal_block(patch_block);
            let updated = self.unpack_option_payload(packed, field_ty)?;
            self.b.ins().jump(merge, &[updated]);
            self.b.switch_to_block(merge);
            self.b.seal_block(merge);
            let selected = self.b.block_params(merge)[0];
            self.set_record_slot(out, index, selected, field_ty)?;
        }
        Ok(out)
    }

    fn lower_patch_diff(
        &mut self,
        base: &str,
        new_value: &TCallArg,
        old_value: &TCallArg,
    ) -> Result<Value, String> {
        let field_types = self
            .meta
            .struct_layout(base)
            .ok_or_else(|| format!("jit patch base `{base}`"))?
            .1
            .to_vec();
        let new_value = self.lower_call_arg(new_value)?;
        let old_value = self.lower_call_arg(old_value)?;
        let out = self.new_record(field_types.len());
        for (index, field_ty) in field_types.iter().enumerate() {
            let new_field = self.record_slot(new_value, index, field_ty)?;
            let old_field = self.record_slot(old_value, index, field_ty)?;
            let equal = match self.meta.clif_ty(field_ty).or_else(|| clif_ty(field_ty)) {
                Some(kind) if kind == types::F64 => {
                    self.b.ins().fcmp(FloatCC::Equal, new_field, old_field)
                }
                Some(_) => self.bool_from_icmp(IntCC::Equal, new_field, old_field),
                None => return Err(format!("jit patch field type unsupported: {field_ty:?}")),
            };
            let none_block = self.b.create_block();
            let some_block = self.b.create_block();
            let merge = self.b.create_block();
            self.b.append_block_param(merge, types::I64);
            self.b
                .ins()
                .brif(equal, none_block, &[], some_block, &[]);
            self.b.switch_to_block(none_block);
            self.b.seal_block(none_block);
            let none = self.b.ins().iconst(types::I64, 0);
            self.b.ins().jump(merge, &[none]);
            self.b.switch_to_block(some_block);
            self.b.seal_block(some_block);
            let some = self.pack_option_payload(new_field, field_ty)?;
            self.b.ins().jump(merge, &[some]);
            self.b.switch_to_block(merge);
            self.b.seal_block(merge);
            let selected = self.b.block_params(merge)[0];
            self.set_record_slot(
                out,
                index,
                selected,
                &Type::Option(Box::new(field_ty.clone())),
            )?;
        }
        Ok(out)
    }

    fn lower_patch_merge(&mut self, recv: &TExpr, other: &TCallArg) -> Result<Value, String> {
        let patch_name = record_type_key(&recv.ty).ok_or("jit patch merge receiver")?;
        let field_types = self
            .meta
            .struct_layout(&patch_name)
            .ok_or_else(|| format!("jit patch type `{patch_name}`"))?
            .1
            .to_vec();
        let current = self.lower_expr(recv)?;
        let other = self.lower_call_arg(other)?;
        let out = self.new_record(field_types.len());
        for (index, option_ty) in field_types.iter().enumerate() {
            let incoming = self.record_slot(other, index, option_ty)?;
            let zero = self.b.ins().iconst(types::I64, 0);
            let missing = self.bool_from_icmp(IntCC::Equal, incoming, zero);
            let old_block = self.b.create_block();
            let incoming_block = self.b.create_block();
            let merge = self.b.create_block();
            self.b.append_block_param(merge, types::I64);
            self.b
                .ins()
                .brif(missing, old_block, &[], incoming_block, &[]);
            self.b.switch_to_block(old_block);
            self.b.seal_block(old_block);
            let old = self.record_slot(current, index, option_ty)?;
            self.b.ins().jump(merge, &[old]);
            self.b.switch_to_block(incoming_block);
            self.b.seal_block(incoming_block);
            self.b.ins().jump(merge, &[incoming]);
            self.b.switch_to_block(merge);
            self.b.seal_block(merge);
            let selected = self.b.block_params(merge)[0];
            self.set_record_slot(out, index, selected, option_ty)?;
        }
        Ok(out)
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
                ty if Self::is_string_abi_ty(ty) => self.host.struct_set_str,
                Type::Int => self.host.struct_set_i64,
                Type::Float => self.host.struct_set_f64,
                Type::Bool => self.host.struct_set_bool,
                Type::Char => self.host.struct_set_char,
                other if clif_ty(other) == Some(types::I64) => self.host.struct_set_i64,
                _ => return Err(format!("jit record field unsupported: {:?}", value.ty)),
            };
            let idx = self.b.ins().iconst(types::I64, i as i64);
            let set_ref = self.module.declare_func_in_func(host_id, self.b.func);
            self.b.ins().call(set_ref, &[handle, idx, raw]);
        }
        Ok(handle)
    }

    fn lower_struct_lit(
        &mut self,
        fields: &[(String, TExpr, bool)],
        as_trait: Option<&(String, String)>,
    ) -> Result<Value, String> {
        let concrete = self.lower_record_fields(fields.iter().map(|(_, value, _)| value))?;
        let Some((_, concrete_name)) = as_trait else {
            return Ok(concrete);
        };
        let type_id = self
            .meta
            .struct_type_id(concrete_name)
            .ok_or_else(|| format!("jit trait object concrete type `{concrete_name}`"))?;
        let object = self.new_record(2);
        let type_id = self.b.ins().iconst(types::I64, type_id);
        self.set_record_slot(object, 0, type_id, &Type::Int)?;
        self.set_record_slot(object, 1, concrete, &Type::Int)?;
        Ok(object)
    }

    fn lower_tuple_lit(&mut self, fields: &[(String, TExpr)]) -> Result<Value, String> {
        self.lower_record_fields(fields.iter().map(|(_, value)| value))
    }

    fn lower_record_field(
        &mut self,
        handle: Value,
        type_name: &str,
        field: &str,
        fallback_ty: &Type,
    ) -> Result<Value, String> {
        let idx = self
            .meta
            .struct_field_index(type_name, field)
            .or_else(|| core_struct_field_index(type_name, field))
            .ok_or_else(|| format!("jit field `{field}` on `{type_name}`"))?
            as i64;
        let idx_val = self.b.ins().iconst(types::I64, idx);
        // TIR has already substituted any generic owner arguments. The
        // metadata type is only the declaration (`T` for `Box<T>`), so use
        // the total expression fact for ABI selection.
        let host_id = match fallback_ty {
            ty if Self::is_string_abi_ty(ty) => self.host.struct_get_str,
            Type::Int => self.host.struct_get_i64,
            Type::Float => self.host.struct_get_f64,
            Type::Bool => self.host.struct_get_bool,
            Type::Char => self.host.struct_get_char,
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

    /// D-DESTRUCT1: `Incident.{id, severity: sev, ..} :: incident`.
    fn lower_struct_destructure(
        &mut self,
        init: &TExpr,
        binds: &[(String, String)],
    ) -> Result<(), String> {
        let handle = self.lower_expr(init)?;
        let type_name = record_type_key(&init.ty).ok_or("jit struct destructure type")?;
        for (local, field_rust) in binds {
            let field_jet = field_rust
                .strip_prefix("user_")
                .unwrap_or(field_rust.as_str());
            let fallback_ty = self
                .meta
                .struct_field_ty(&type_name, field_jet)
                .or_else(|| self.meta.struct_field_ty(&type_name, field_rust))
                .unwrap_or(Type::Int);
            let value = self.lower_record_field(handle, &type_name, field_rust, &fallback_ty)?;
            let clif = clif_ty(&fallback_ty)
                .ok_or_else(|| format!("jit struct destructure unsupported: {fallback_ty:?}"))?;
            let var = self.fresh_var(clif);
            self.b.def_var(var, value);
            self.vars.insert(local.clone(), var);
            self.var_tys.insert(local.clone(), fallback_ty);
        }
        Ok(())
    }

    /// `[a, b, c] :: xs` — bounds-checked element binds via the list-get host.
    fn lower_list_destructure(
        &mut self,
        init: &TExpr,
        elems: &[String],
        want: usize,
        line: usize,
    ) -> Result<(), String> {
        let elem_ty = match &init.ty {
            Type::List(inner) | Type::FixedList { elem: inner, .. } => inner.as_ref().clone(),
            _ => {
                return Err(format!(
                    "jit list destructure collection type unsupported: {:?}",
                    init.ty
                ))
            }
        };
        if elems.len() != want {
            return Err("jit list destructure bind count mismatch".to_string());
        }
        let list = self.lower_expr(init)?;
        let line_const = self.b.ins().iconst(types::I32, line as i64);
        let host_id = match &elem_ty {
            Type::Float => self.host.coll.list_get_f64,
            _ => self.host.coll.list_get,
        };
        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
        let clif = clif_ty(&elem_ty).ok_or_else(|| {
            format!("jit list destructure element type unsupported: {elem_ty:?}")
        })?;
        for (i, local) in elems.iter().enumerate() {
            let idx = self.b.ins().iconst(types::I64, i as i64);
            let call = self.b.ins().call(host_ref, &[list, idx, line_const]);
            let val = self.b.inst_results(call)[0];
            self.emit_trap_check()?;
            let var = self.fresh_var(clif);
            self.b.def_var(var, val);
            self.vars.insert(local.clone(), var);
            self.var_tys.insert(local.clone(), elem_ty.clone());
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

    fn lower_ct_value(&mut self, value: &jet_foundation::AST::CtValue) -> Result<Value, String> {
        use jet_foundation::AST::CtValue;
        match value {
            CtValue::Int(value) => Ok(self.b.ins().iconst(types::I64, *value)),
            CtValue::Bool(value) => Ok(self.b.ins().iconst(types::I8, i64::from(*value))),
            CtValue::Char(value) => Ok(self.b.ins().iconst(types::I32, u32::from(*value) as i64)),
            CtValue::Str(value) => {
                let handle = self.runtime.heap.alloc_string(value.clone());
                Ok(self.b.ins().iconst(types::I64, handle))
            }
            CtValue::List(values) => {
                let new_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_new, self.b.func);
                let new_call = self.b.ins().call(new_ref, &[]);
                let handle = self.b.inst_results(new_call)[0];
                let push_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_push, self.b.func);
                for value in values {
                    let value = self.lower_ct_value(value)?;
                    self.b.ins().call(push_ref, &[handle, value]);
                }
                Ok(handle)
            }
            other => Err(format!("jit comptime value unsupported: {other:?}")),
        }
    }

    fn lower_list_spread(
        &mut self,
        list_ty: &Type,
        parts: &[ListSpreadPart],
    ) -> Result<Value, String> {
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[]);
        let out = self.b.inst_results(new_call)[0];
        let push_id = if jit_list_float_type(list_ty) {
            self.host.coll.list_push_f64
        } else {
            self.host.coll.list_push
        };
        let push_ref = self.module.declare_func_in_func(push_id, self.b.func);
        for part in parts {
            match part {
                ListSpreadPart::Elem(elem) => {
                    let value = self.lower_expr(elem)?;
                    self.b.ins().call(push_ref, &[out, value]);
                }
                ListSpreadPart::Spread(list) => {
                    let input = self.lower_expr(list)?;
                    let len_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.list_len, self.b.func);
                    let len_call = self.b.ins().call(len_ref, &[input]);
                    let len = self.b.inst_results(len_call)[0];
                    let idx_var = self.fresh_var(types::I64);
                    let zero = self.b.ins().iconst(types::I64, 0);
                    self.b.def_var(idx_var, zero);
                    let head = self.b.create_block();
                    let body = self.b.create_block();
                    let done = self.b.create_block();
                    self.b.ins().jump(head, &[]);
                    self.b.switch_to_block(head);
                    let idx = self.b.use_var(idx_var);
                    let at_end =
                        self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
                    self.b.ins().brif(at_end, done, &[], body, &[]);
                    self.b.switch_to_block(body);
                    self.b.seal_block(body);
                    let line = self.b.ins().iconst(types::I32, 1);
                    let get_id = if jit_list_float_type(list_ty) {
                        self.host.coll.list_get_f64
                    } else {
                        self.host.coll.list_get
                    };
                    let get_ref = self.module.declare_func_in_func(get_id, self.b.func);
                    let get_call = self.b.ins().call(get_ref, &[input, idx, line]);
                    let value = self.b.inst_results(get_call)[0];
                    self.emit_trap_check()?;
                    self.b.ins().call(push_ref, &[out, value]);
                    let one = self.b.ins().iconst(types::I64, 1);
                    let next = self.b.ins().iadd(idx, one);
                    self.b.def_var(idx_var, next);
                    self.b.ins().jump(head, &[]);
                    self.b.seal_block(head);
                    self.b.switch_to_block(done);
                    self.b.seal_block(done);
                }
            }
        }
        Ok(out)
    }


    fn lower_map_lit(&mut self, entries: &[(TExpr, TExpr)]) -> Result<Value, String> {
        self.lower_map_lit_pairs(entries.iter().map(|(k, v)| (k, v)))
    }

    /// Direct map construction on an SSA handle (no shared `__jet_m` local).
    fn lower_map_lit_pairs<'a, I>(&mut self, entries: I) -> Result<Value, String>
    where
        I: IntoIterator<Item = (&'a TExpr, &'a TExpr)>,
    {
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.map_new, self.b.func);
        let new_call = self.b.ins().call(new_ref, &[]);
        let handle = self.b.inst_results(new_call)[0];
        for (k, v) in entries {
            if !matches!(&k.ty, Type::String) {
                return Err(format!("jit map key type unsupported: {:?}", k.ty));
            }
            let key = self.lower_expr(k)?;
            let val = self.lower_expr(v)?;
            let val = match self.meta.clif_ty(&v.ty) {
                Some(types::I32) => self.b.ins().uextend(types::I64, val),
                Some(types::I8) => self.b.ins().uextend(types::I64, val),
                Some(types::F64) => self.b.ins().bitcast(
                    types::I64,
                    Self::scalar_bitcast_memflags(),
                    val,
                ),
                Some(types::I64) | None => val,
                Some(other) => {
                    return Err(format!(
                        "jit map value type unsupported: {:?} ({other:?})",
                        v.ty
                    ));
                }
            };
            let insert_ref = self
                .module
                .declare_func_in_func(self.host.coll.map_insert, self.b.func);
            self.b.ins().call(insert_ref, &[handle, key, val]);
        }
        Ok(handle)
    }

    /// #779 MapLit desugar: `IfExpr(true) { let m = {}; m[k]=v; …; m }`.
    /// Nested map lits reuse one `__jet_m` local; the generic IfExpr path then
    /// returns the innermost map. Rebuild from IndexAssigns on an SSA handle.
    fn map_lit_desugar_entries<'a>(
        cond: &'a TIfCond,
        then_body: &'a [TStmt],
        then_value: &'a TExpr,
    ) -> Option<Vec<(&'a TExpr, &'a TExpr)>> {
        let TIfCond::Plain(c) = cond else {
            return None;
        };
        if !matches!(&c.kind, TExprKind::BoolLit(true)) {
            return None;
        }
        let TExprKind::Local(result) = &then_value.kind else {
            return None;
        };
        let (first, rest) = then_body.split_first()?;
        let TStmt::Let { name, init, .. } = first else {
            return None;
        };
        if name != &result.name {
            return None;
        }
        if !matches!(&init.kind, TExprKind::MapLit(entries) if entries.is_empty()) {
            return None;
        }
        let mut entries = Vec::with_capacity(rest.len());
        for stmt in rest {
            let TStmt::IndexAssign {
                base,
                index,
                is_map: true,
                value,
            } = stmt
            else {
                return None;
            };
            let TExprKind::Local(base_local) = &base.kind else {
                return None;
            };
            if base_local.name != *name {
                return None;
            }
            entries.push((index, value));
        }
        Some(entries)
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
            TExprKind::FloatLit(v) => Ok(self.b.ins().f64const(if expr.ty == Type::Float32 {
                (*v as f32) as f64
            } else {
                *v
            })),
            TExprKind::BoolLit(v) => Ok(self.b.ins().iconst(types::I8, if *v { 1 } else { 0 })),
            TExprKind::CharLit(v) => Ok(self.b.ins().iconst(types::I32, *v as i64)),
            TExprKind::StrLit(parts) => self.lower_string_lit(parts),
            TExprKind::Local(local) => self.load_local(local),
            TExprKind::InlineBlock(stmts) => self.lower_inline_block(stmts, &expr.ty),
            TExprKind::Borrow { place, .. } => {
                // JIT has no borrow ABI — materialize the place value (scalar /
                // field / already-materialized view handle).
                self.lower_expr(place)
            }
            TExprKind::Unary { op, operand } => {
                let inner = self.lower_expr(operand)?;
                Ok(match op {
                    UnOp::Neg => match &operand.ty {
                        Type::Int => self.b.ins().ineg(inner),
                        Type::IntN {
                            signed: true,
                            bits,
                        } => {
                            let zero = self.b.ins().iconst(types::I64, 0);
                            self.lower_intn_values(
                                BinOp::Sub,
                                INTN_MODE_TRAP,
                                zero,
                                inner,
                                true,
                                *bits,
                                true,
                            )?
                        }
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
            TExprKind::CompareChain { operands, ops, hooks } => {
                self.lower_compare_chain(operands, ops, hooks)
            }
            TExprKind::Call { name, args } => {
                let func_id = self
                    .func_ids
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("jit call to unknown function `{name}`"))?;
                let mut arg_vals = Vec::with_capacity(args.len());
                let mut copy_back = Vec::new();
                for arg in args {
                    let scalar_write = arg.mut_borrow
                        && matches!(
                            &arg.value.ty,
                            Type::Int
                                | Type::IntN { .. }
                                | Type::Float
                                | Type::Float32
                                | Type::Bool
                                | Type::Char
                        );
                    if !scalar_write {
                        arg_vals.push(self.lower_call_arg(arg)?);
                        continue;
                    }
                    let TExprKind::Local(local) = &arg.value.kind else {
                        return Err("jit writable scalar argument must be a local".to_string());
                    };
                    let key = Self::local_key(local);
                    let var = self
                        .vars
                        .get(&key)
                        .copied()
                        .ok_or_else(|| format!("jit writable scalar unknown `{key}`"))?;
                    let value = self.b.use_var(var);
                    let clif = self
                        .meta
                        .clif_ty(&arg.value.ty)
                        .ok_or("jit writable scalar argument type")?;
                    let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        u32::from(clif.bytes()),
                        0,
                    ));
                    self.b.ins().stack_store(value, slot, 0);
                    let pointer = self.b.ins().stack_addr(
                        self.module.target_config().pointer_type(),
                        slot,
                        0,
                    );
                    arg_vals.push(pointer);
                    copy_back.push((var, slot, clif));
                }
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &arg_vals);
                for (var, slot, clif) in copy_back {
                    let value = self.b.ins().stack_load(clif, slot, 0);
                    self.b.def_var(var, value);
                }
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
                if let Some(entries) =
                    Self::map_lit_desugar_entries(cond.as_ref(), then_body, then_value)
                {
                    return self.lower_map_lit_pairs(entries);
                }
                let cond_val = match cond.as_ref() {
                    TIfCond::Plain(cond) => self.lower_expr(cond)?,
                    TIfCond::And { .. } => return Err("jit if-expression binding conjunction unsupported".to_string()),
                    TIfCond::IfLet { .. } => return Err("jit if-expression if-let unsupported".to_string()),
                    TIfCond::IsNone { .. } => return Err("jit if-expression is-none unsupported".to_string()),
                    TIfCond::Matches { .. } => return Err("jit if-expression pattern match unsupported".to_string()),
                };
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
                self.lower_stmts_scoped(then_body)?;
                if !self.dead {
                    let then_val = self.lower_expr(then_value)?;
                    if !self.dead {
                        self.b.ins().jump(merge_block, &[then_val]);
                    }
                }
                let then_reaches_merge = !self.dead;

                self.b.switch_to_block(else_block);
                self.b.seal_block(else_block);
                self.lower_stmts_scoped(else_body)?;
                if !self.dead {
                    let else_val = self.lower_expr(else_value)?;
                    if !self.dead {
                        self.b.ins().jump(merge_block, &[else_val]);
                    }
                }
                let else_reaches_merge = !self.dead;

                self.b.switch_to_block(merge_block);
                self.b.seal_block(merge_block);
                self.dead = !(then_reaches_merge || else_reaches_merge);
                let phi = self.b.block_params(merge_block)[0];
                Ok(phi)
            }
            TExprKind::Clone(inner) => self.lower_clone(inner),
            TExprKind::CoreCall {
                module,
                method,
                args,
                ..
            } => {
                if module == "core.reflect" && method == "of" && args.len() == 1 {
                    return self.lower_reflect_of(&args[0]);
                }
                if module == "core.testing" {
                    return self.lower_testing_call(method, args, &expr.ty);
                }
                if module == "core.data" {
                    return self.lower_data_call(method, args, &expr.ty);
                }
                if module == "core.mem" && method == "volatile_read" && args.len() == 1 {
                    if self.unsafe_depth == 0 {
                        return Err("jit volatile read outside #Unsafe".to_string());
                    }
                    let pointer = self.lower_expr(&args[0])?;
                    let clif = self.meta.clif_ty(&expr.ty).ok_or_else(|| {
                        format!("jit volatile read result unsupported: {:?}", expr.ty)
                    })?;
                    return Ok(self.b.ins().load(clif, MemFlags::trusted(), pointer, 0));
                }
                if module == "core.mem" && method == "volatile_write" && args.len() == 2 {
                    if self.unsafe_depth == 0 {
                        return Err("jit volatile write outside #Unsafe".to_string());
                    }
                    let pointer = self.lower_expr(&args[0])?;
                    let value = self.lower_expr(&args[1])?;
                    self.b
                        .ins()
                        .store(MemFlags::trusted(), value, pointer, 0);
                    return Ok(self.b.ins().iconst(types::I8, 0));
                }
                if module == "jet.crypto" {
                    let (host_id, arg_values): (FuncId, Vec<Value>) =
                        match (method.as_str(), args.as_slice()) {
                            ("__signing_generate", []) => {
                                (self.host.crypto.signing_generate, Vec::new())
                            }
                            ("__x25519_generate", []) => {
                                (self.host.crypto.x25519_generate, Vec::new())
                            }
                            ("__signing_public", [key]) => {
                                (self.host.crypto.signing_public, vec![self.lower_expr(key)?])
                            }
                            ("__x25519_public", [key]) => {
                                (self.host.crypto.x25519_public, vec![self.lower_expr(key)?])
                            }
                            ("sign", [key, message]) => (
                                self.host.crypto.sign,
                                vec![self.lower_expr(key)?, self.lower_expr(message)?],
                            ),
                            ("verify", [key, message, signature]) => (
                                self.host.crypto.verify,
                                vec![
                                    self.lower_expr(key)?,
                                    self.lower_expr(message)?,
                                    self.lower_expr(signature)?,
                                ],
                            ),
                            ("sha256", [data]) => {
                                (self.host.crypto.sha256, vec![self.lower_expr(data)?])
                            }
                            ("seal", [recipients, plaintext, aad]) => (
                                self.host.crypto.seal,
                                vec![
                                    self.lower_expr(recipients)?,
                                    self.lower_expr(plaintext)?,
                                    self.lower_expr(aad)?,
                                ],
                            ),
                            ("open", [recipient, sealed, aad]) => (
                                self.host.crypto.open,
                                vec![
                                    self.lower_expr(recipient)?,
                                    self.lower_expr(sealed)?,
                                    self.lower_expr(aad)?,
                                ],
                            ),
                            ("password_hash", [password]) => (
                                self.host.crypto.password_hash,
                                vec![self.lower_expr(password)?],
                            ),
                            ("password_verify", [password, stored]) => (
                                self.host.crypto.password_verify,
                                vec![self.lower_expr(password)?, self.lower_expr(stored)?],
                            ),
                            ("file_open", [recipient, source, dest]) => (
                                self.host.crypto.file_open,
                                vec![
                                    self.lower_expr(recipient)?,
                                    self.lower_expr(source)?,
                                    self.lower_expr(dest)?,
                                ],
                            ),
                            ("__digest256_hex", [digest]) => (
                                self.host.crypto.digest256_hex,
                                vec![self.lower_expr(digest)?],
                            ),
                            ("__digest256_bytes", [digest]) => (
                                self.host.crypto.digest256_bytes,
                                vec![self.lower_expr(digest)?],
                            ),
                            ("__signature_bytes", [signature]) => (
                                self.host.crypto.signature_bytes,
                                vec![self.lower_expr(signature)?],
                            ),
                            ("__sealed_bytes", [sealed]) => (
                                self.host.crypto.sealed_bytes,
                                vec![self.lower_expr(sealed)?],
                            ),
                            ("__x25519_public_bytes", [key]) => (
                                self.host.crypto.x25519_public_bytes,
                                vec![self.lower_expr(key)?],
                            ),
                            ("__x25519_public_text", [key]) => (
                                self.host.crypto.x25519_public_text,
                                vec![self.lower_expr(key)?],
                            ),
                            ("__x25519_public_from_text", [text]) => (
                                self.host.crypto.x25519_public_from_text,
                                vec![self.lower_expr(text)?],
                            ),
                            ("__secret_from_text", [text]) => (
                                self.host.crypto.secret_from_text,
                                vec![self.lower_expr(text)?],
                            ),
                            ("__vault_wrapped_from_bytes", [bytes]) => (
                                self.host.crypto.vault_wrapped_from_bytes,
                                vec![self.lower_expr(bytes)?],
                            ),
                            ("__vault_wrapped_bytes", [wrapped]) => (
                                self.host.crypto.vault_wrapped_bytes,
                                vec![self.lower_expr(wrapped)?],
                            ),
                            ("__vault_unlock_recipient", [identity]) => (
                                self.host.crypto.vault_unlock_recipient,
                                vec![self.lower_expr(identity)?],
                            ),
                            ("__vault_unlock_passphrase", [passphrase]) => (
                                self.host.crypto.vault_unlock_passphrase,
                                vec![self.lower_expr(passphrase)?],
                            ),
                            _ => {
                                return Err(format!(
                                    "jit core call unsupported: {module}.{method}"
                                ))
                            }
                        };
                    let host = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host, &arg_values);
                    let value = self.b.inst_results(call)[0];
                    self.emit_trap_check()?;
                    return Ok(value);
                }
                if module == "core.crypto.expert" {
                    let (host_id, arg_values): (FuncId, Vec<Value>) =
                        match (method.as_str(), args.as_slice()) {
                            ("aes256gcm_seal", [key, nonce, plaintext, aad]) => (
                                self.host.crypto.expert_aes256gcm_seal,
                                vec![
                                    self.lower_expr(key)?,
                                    self.lower_expr(nonce)?,
                                    self.lower_expr(plaintext)?,
                                    self.lower_expr(aad)?,
                                ],
                            ),
                            ("aes256gcm_open", [key, nonce, ciphertext, aad]) => (
                                self.host.crypto.expert_aes256gcm_open,
                                vec![
                                    self.lower_expr(key)?,
                                    self.lower_expr(nonce)?,
                                    self.lower_expr(ciphertext)?,
                                    self.lower_expr(aad)?,
                                ],
                            ),
                            ("open_v1", [key, blob]) => (
                                self.host.crypto.expert_open_v1,
                                vec![self.lower_expr(key)?, self.lower_expr(blob)?],
                            ),
                            ("migrate_v1", [key, source, recipients, dest]) => (
                                self.host.crypto.expert_migrate_v1,
                                vec![
                                    self.lower_expr(key)?,
                                    self.lower_expr(source)?,
                                    self.lower_expr(recipients)?,
                                    self.lower_expr(dest)?,
                                ],
                            ),
                            ("x25519", [secret, public]) => (
                                self.host.crypto.expert_x25519,
                                vec![
                                    self.lower_expr(secret)?,
                                    self.lower_expr(public)?,
                                    self.b.ins().iconst(types::I64, 1),
                                ],
                            ),
                            ("x25519", [secret, public, reject]) => {
                                let secret_val = self.lower_expr(secret)?;
                                let public_val = self.lower_expr(public)?;
                                let reject_val = self.lower_expr(reject)?;
                                let reject_i64 = if self.meta.clif_ty(&reject.ty)
                                    == Some(types::I8)
                                {
                                    self.b.ins().uextend(types::I64, reject_val)
                                } else {
                                    reject_val
                                };
                                (
                                    self.host.crypto.expert_x25519,
                                    vec![secret_val, public_val, reject_i64],
                                )
                            }
                            ("secret_bytes", [secret]) => (
                                self.host.crypto.expert_secret_bytes,
                                vec![self.lower_expr(secret)?],
                            ),
                            _ => {
                                return Err(format!(
                                    "jit core call unsupported: {module}.{method}"
                                ))
                            }
                        };
                    let host = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host, &arg_values);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.io" && method == "args" && args.is_empty() {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.io_args, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.io" && method == "print" && args.len() == 1 {
                    self.emit_print(&args[0])?;
                    return Ok(self.b.ins().iconst(types::I8, 0));
                }
                if module == "core.science.measurement"
                    && method == "from"
                    && args.len() == 2
                {
                    let value = self.lower_expr(&args[0])?;
                    let uncertainty = self.lower_expr(&args[1])?;
                    let host = self
                        .module
                        .declare_func_in_func(self.host.measurement_new, self.b.func);
                    let call = self.b.ins().call(host, &[value, uncertainty]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.io" && method == "input" && args.len() <= 1 {
                    let (has_prompt, prompt) = if args.is_empty() {
                        (
                            self.b.ins().iconst(types::I8, 0),
                            self.b.ins().iconst(types::I64, 0),
                        )
                    } else {
                        (
                            self.b.ins().iconst(types::I8, 1),
                            self.lower_expr(&args[0])?,
                        )
                    };
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.core.io_input, self.b.func);
                    let call = self.b.ins().call(host_ref, &[has_prompt, prompt]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.io" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "stdout" if args.is_empty() => (self.host.io.stdout, Vec::new()),
                        "stderr" if args.is_empty() => (self.host.io.stderr, Vec::new()),
                        "stdin" if args.is_empty() => (self.host.io.stdin, Vec::new()),
                        "terminal_width" if args.is_empty() => {
                            (self.host.io.terminal_width, Vec::new())
                        }
                        "terminal_height" if args.is_empty() => {
                            (self.host.io.terminal_height, Vec::new())
                        }
                        "style" if args.len() == 2 => (
                            self.host.io.style,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "style_force" if args.len() == 2 => (
                            self.host.io.style_force,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "progress" if args.len() == 1 => {
                            (self.host.io.progress, vec![self.lower_expr(&args[0])?])
                        }
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.os" && args.is_empty() {
                    let host_id = match method.as_str() {
                        "name" => self.host.core.os_name,
                        "family" => self.host.core.os_family,
                        "arch" => self.host.core.os_arch,
                        "cpu_count" => self.host.core.os_cpu_count,
                        "temp_dir" => self.host.core.os_temp_dir,
                        "executable" => self.host.core.os_executable,
                        "pid" => self.host.core.os_pid,
                        "hostname" => self.host.core.os_hostname,
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.event" && method == "scope" && args.is_empty() {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.watcher.event_scope, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.net" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "tcp_listen" if args.len() == 1 => (
                            self.host.net_http.tcp_listen_str,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "tcp_listen_addr" if args.len() == 1 => (
                            self.host.net_http.tcp_listen_addr,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "tcp_connect" if args.len() == 1 => (
                            self.host.net_http.tcp_connect,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "listener_local_socket_addr" if args.len() == 1 => (
                            self.host.net_http.listener_local_socket_addr,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "socket_port" if args.len() == 1 => (
                            self.host.net_http.socket_port_typed,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "socket_host" if args.len() == 1 => (
                            self.host.net_http.socket_host,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "socket_to_string" if args.len() == 1 => (
                            self.host.net_http.socket_to_string,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "socket_addr" if args.len() == 2 => (
                            self.host.net_http.socket_addr,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "set_timeout" if args.len() == 2 => (
                            self.host.net_http.set_timeout,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "tcp_reply" if args.len() == 3 => (
                            self.host.net_http.tcp_reply,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "udp_bind" if args.len() == 1 => (
                            self.host.net_http.udp_bind,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "udp_local_addr" if args.len() == 1 => (
                            self.host.net_http.udp_local_addr,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "udp_set_timeout" if args.len() == 2 => (
                            self.host.net_http.udp_set_timeout,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "udp_send_bytes_to" if args.len() == 3 => (
                            self.host.net_http.udp_send_bytes_to,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "udp_receive" if args.len() == 2 => (
                            self.host.net_http.udp_receive,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "udp_packet_bytes" if args.len() == 1 => (
                            self.host.net_http.udp_packet_bytes,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "udp_packet_original_len" if args.len() == 1 => (
                            self.host.net_http.udp_packet_original_len,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "udp_packet_truncated" if args.len() == 1 => (
                            self.host.net_http.udp_packet_truncated,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "unix_listen" if args.len() == 1 => (
                            self.host.net_http.unix_listen,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "unix_accept" if args.len() == 1 => (
                            self.host.net_http.unix_accept,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "unix_connect" if args.len() == 1 => (
                            self.host.net_http.unix_connect,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "unix_read" if args.len() == 1 => (
                            self.host.net_http.unix_read,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "unix_write" if args.len() == 2 => (
                            self.host.net_http.unix_write,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "unix_write_all_bytes" if args.len() == 2 => (
                            self.host.net_http.unix_write_all_bytes,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "unix_close" if args.len() == 1 => (
                            self.host.net_http.unix_close,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    let v = self.b.inst_results(call)[0];
                    return if matches!(expr.ty, Type::Bool) {
                        Ok(self.b.ins().ireduce(types::I8, v))
                    } else {
                        Ok(v)
                    };
                }
                if module == "core.http.server" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "mux" if args.is_empty() => (self.host.net_http.http_mux_new, vec![]),
                        "response" if args.len() == 2 => (
                            self.host.net_http.http_response,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "bind" if args.len() == 2 => (
                            self.host.net_http.http_server_bind,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "serve_once_listener" if args.len() == 2 => (
                            self.host.net_http.http_serve_once_listener,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "request_id" if args.len() == 1 => (
                            self.host.net_http.http_request_id,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "sse" if args.len() == 1 => (
                            self.host.net_http.http_sse,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "static_file_range" if args.len() == 3 => (
                            self.host.net_http.http_static_file_range,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if matches!(module.as_str(), "jet.http" | "core.http" | "core.http.client") {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "get" if args.len() == 1 => (
                            self.host.net_http.http_client_get,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "post" if args.len() == 2 => (
                            self.host.net_http.http_client_post,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "request" if args.len() == 2 => (
                            self.host.net_http.http_client_request_new,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.ws" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "upgrade" if args.len() == 1 => (
                            self.host.net_http.ws_upgrade,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "connect" if args.len() == 1 => (
                            self.host.net_http.ws_connect,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.watcher" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "files" if args.len() == 1 => (
                            self.host.watcher.watcher_files,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "process_pid" if args.len() == 1 => (
                            self.host.watcher.watcher_process_pid,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "port" if args.len() == 2 => (
                            self.host.watcher.watcher_port,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "set" if args.is_empty() => (self.host.watcher.watcher_set, vec![]),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "jet.log" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "set_level" if args.len() == 1 => {
                            (self.host.core.log_set_level, vec![self.lower_expr(&args[0])?])
                        }
                        "setup" if args.len() == 1 => {
                            (self.host.core.log_setup, vec![self.lower_expr(&args[0])?])
                        }
                        "debug" if args.len() == 1 => {
                            (self.host.core.log_debug, vec![self.lower_expr(&args[0])?])
                        }
                        "info" if args.len() == 1 => {
                            (self.host.core.log_info, vec![self.lower_expr(&args[0])?])
                        }
                        "warn" if args.len() == 1 => {
                            (self.host.core.log_warn, vec![self.lower_expr(&args[0])?])
                        }
                        "error" if args.len() == 1 => {
                            (self.host.core.log_error, vec![self.lower_expr(&args[0])?])
                        }
                        "set_trace_id" if args.len() == 1 => (
                            self.host.core.log_set_trace_id,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "field" if args.len() == 2 => (
                            self.host.core.log_field,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "int" if args.len() == 2 => (
                            self.host.core.log_int_field,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "bool" if args.len() == 2 => (
                            self.host.core.log_bool_field,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "counter" if args.len() == 2 => (
                            self.host.core.log_counter,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "span" if args.len() == 1 => {
                            (self.host.core.log_span, vec![self.lower_expr(&args[0])?])
                        }
                        "enter" if args.len() == 1 => {
                            (self.host.core.log_enter, vec![self.lower_expr(&args[0])?])
                        }
                        "close" if args.len() == 1 => {
                            (self.host.core.log_close, vec![self.lower_expr(&args[0])?])
                        }
                        "info_fields" if args.len() == 2 => (
                            self.host.core.log_info_fields,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(clif_ty(&expr.ty)
                        .map(|_| self.b.inst_results(call)[0])
                        .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)));
                }
                if module == "core.files" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "create" if args.len() == 1 => {
                            (self.host.stream.fs_create, vec![self.lower_expr(&args[0])?])
                        }
                        "open" if args.len() == 1 => {
                            (self.host.stream.fs_open, vec![self.lower_expr(&args[0])?])
                        }
                        "exists" if args.len() == 1 => {
                            (self.host.core.fs_exists, vec![self.lower_expr(&args[0])?])
                        }
                        "read" if args.len() == 1 => {
                            (self.host.core.fs_read, vec![self.lower_expr(&args[0])?])
                        }
                        "write" if args.len() == 2 => (
                            self.host.core.fs_write,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "create_dir" | "create_dir_all" if args.len() == 1 => {
                            (self.host.core.fs_create_dir, vec![self.lower_expr(&args[0])?])
                        }
                        "list_dir" if args.len() == 1 => {
                            (self.host.core.fs_list_dir, vec![self.lower_expr(&args[0])?])
                        }
                        "remove_all" if args.len() == 1 => {
                            (self.host.core.fs_remove_all, vec![self.lower_expr(&args[0])?])
                        }
                        "remove" if args.len() == 1 => {
                            (self.host.core.fs_remove, vec![self.lower_expr(&args[0])?])
                        }
                        "stat" if args.len() == 1 => {
                            (self.host.core.fs_stat, vec![self.lower_expr(&args[0])?])
                        }
                        "read_at" if args.len() == 3 => (
                            self.host.core.fs_read_at,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "write_at" if args.len() == 3 => (
                            self.host.core.fs_write_at,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "fsync" if args.len() == 1 => {
                            (self.host.core.fs_fsync, vec![self.lower_expr(&args[0])?])
                        }
                        "write_atomic" if args.len() == 2 => (
                            self.host.core.fs_write_atomic,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "walk" if args.len() == 1 => {
                            (self.host.core.fs_walk, vec![self.lower_expr(&args[0])?])
                        }
                        "glob" if args.len() == 1 => {
                            (self.host.core.fs_glob, vec![self.lower_expr(&args[0])?])
                        }
                        "symlink" if args.len() == 2 => (
                            self.host.core.fs_symlink,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "read_link" if args.len() == 1 => {
                            (self.host.core.fs_read_link, vec![self.lower_expr(&args[0])?])
                        }
                        "hard_link" if args.len() == 2 => (
                            self.host.core.fs_hard_link,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "canonicalize" if args.len() == 1 => {
                            (self.host.core.fs_canonicalize, vec![self.lower_expr(&args[0])?])
                        }
                        "absolute" if args.len() == 1 => {
                            (self.host.core.fs_absolute, vec![self.lower_expr(&args[0])?])
                        }
                        "copy_dir" if args.len() == 2 => (
                            self.host.core.fs_copy_dir,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "temp_dir" if args.len() == 1 => {
                            (self.host.core.fs_temp_dir, vec![self.lower_expr(&args[0])?])
                        }
                        "temp_file" if args.len() == 1 => {
                            (self.host.core.fs_temp_file, vec![self.lower_expr(&args[0])?])
                        }
                        "lock" if args.len() == 1 => {
                            (self.host.core.fs_lock, vec![self.lower_expr(&args[0])?])
                        }
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.encoding.hex"
                    || module == "core.encoding.base64"
                    || module == "core.encoding.base32"
                {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match (module.as_str(), method.as_str()) {
                        ("core.encoding.hex", "encode") if args.len() == 1 => {
                            (self.host.encoding.hex_encode, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.encoding.hex", "decode") if args.len() == 1 => {
                            (self.host.encoding.hex_decode, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.encoding.base64", "encode") if args.len() == 1 => {
                            (self.host.encoding.b64_encode, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.encoding.base64", "encode_url") if args.len() == 1 => (
                            self.host.encoding.b64_encode_url,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        ("core.encoding.base64", "decode") if args.len() == 1 => {
                            (self.host.encoding.b64_decode, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.encoding.base64", "decode_url") if args.len() == 1 => (
                            self.host.encoding.b64_decode_url,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        ("core.encoding.base32", "encode") if args.len() == 1 => (
                            self.host.encoding.base32_encode,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        ("core.encoding.base32", "decode") if args.len() == 1 => (
                            self.host.encoding.base32_decode,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.encoding.csv" {
                    // Typed `csv.decode<T>` → Result[[T], …]
                    if method == "decode" && args.len() == 1 {
                        if let Type::Result { ok, .. } = &expr.ty {
                            if let Type::List(elem) = ok.as_ref() {
                                return self.lower_typed_csv_decode(&args[0], elem);
                            }
                        }
                    }
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "parse" if args.len() == 1 => {
                            (self.host.encoding.csv_parse, vec![self.lower_expr(&args[0])?])
                        }
                        "to_string" if args.len() == 1 => (
                            self.host.encoding.csv_to_string,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "writer" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.csv_writer, vec![file, limits])
                        }
                        "reader" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.csv_reader, vec![file, limits])
                        }
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.encoding.json" {
                    let datatree_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. }
                            if matches!(
                                ok.as_ref(),
                                Type::Named(n)
                                    if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
                            )
                    );
                    let datatree_arg = args.first().is_some_and(|a| {
                        matches!(
                            &a.ty,
                            Type::Named(n)
                                if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
                        )
                    });
                    // Typed Codable decode/to_string (Encode-type path).
                    if method == "decode" && args.len() == 1 && !datatree_ok {
                        if let Type::Result { ok, .. } = &expr.ty {
                            return self.lower_typed_json_decode(&args[0], ok);
                        }
                    }
                    if method == "decode_traced" && args.len() == 1 {
                        if let Type::Result { ok, .. } = &expr.ty {
                            return self.lower_typed_json_decode_traced(&args[0], ok);
                        }
                    }
                    if matches!(method.as_str(), "to_string" | "to_string_pretty")
                        && args.len() == 1
                        && !datatree_arg
                    {
                        return self.lower_typed_json_to_string(
                            &args[0],
                            method == "to_string_pretty",
                        );
                    }
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "parse" if args.len() == 1 && datatree_ok => {
                            (self.host.encoding.json_parse, vec![self.lower_expr(&args[0])?])
                        }
                        "decode" if args.len() == 1 && datatree_ok => {
                            (self.host.encoding.json_decode, vec![self.lower_expr(&args[0])?])
                        }
                        "to_string" if args.len() == 1 && datatree_arg => (
                            self.host.encoding.json_to_string,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "to_string_pretty" if args.len() == 1 && datatree_arg => (
                            self.host.encoding.json_to_string_pretty,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "canonical" if args.len() == 1 && datatree_arg => (
                            self.host.encoding.json_canonical,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "events" if args.len() == 1 && datatree_arg => (
                            self.host.encoding.json_events,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "writer" if (2..=3).contains(&args.len()) => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = self.lower_expr(&args[1])?;
                            let canon = if args.len() == 3 {
                                let b = self.lower_expr(&args[2])?;
                                self.b.ins().uextend(types::I64, b)
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (
                                self.host.stream.json_writer,
                                vec![file, limits, canon],
                            )
                        }
                        "reader" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.json_reader, vec![file, limits])
                        }
                        _ => {
                            return Err(format!(
                                "jit core call unsupported: core.encoding.json.{method}"
                            ))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.encoding.jsonl" {
                    let datatree_list_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. }
                            if matches!(ok.as_ref(), Type::List(elem)
                                if matches!(
                                    elem.as_ref(),
                                    Type::Named(n)
                                        if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
                                ))
                    );
                    let datatree_list_arg = args.first().is_some_and(|a| {
                        matches!(
                            &a.ty,
                            Type::List(elem)
                                if matches!(
                                    elem.as_ref(),
                                    Type::Named(n)
                                        if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
                                )
                        )
                    });
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "parse" if args.len() == 1 && datatree_list_ok => {
                            (self.host.encoding.jsonl_parse, vec![self.lower_expr(&args[0])?])
                        }
                        "to_string" if args.len() == 1 && datatree_list_arg => (
                            self.host.encoding.jsonl_to_string,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "writer" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.jsonl_writer, vec![file, limits])
                        }
                        "reader" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.jsonl_reader, vec![file, limits])
                        }
                        _ => {
                            return Err(format!(
                                "jit core call unsupported: core.encoding.jsonl.{method}"
                            ))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.encoding.xml" {
                    let datatree_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. }
                            if matches!(
                                ok.as_ref(),
                                Type::Named(n)
                                    if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv" | "Xml")
                            )
                    );
                    let datatree_arg = args.first().is_some_and(|a| {
                        matches!(
                            &a.ty,
                            Type::Named(n)
                                if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv" | "Xml")
                        )
                    });
                    let datatree_list_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. }
                            if matches!(
                                ok.as_ref(),
                                Type::List(elem)
                                    if matches!(
                                        elem.as_ref(),
                                        Type::Named(n)
                                            if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv" | "Xml")
                                    )
                            )
                    );
                    let option_string_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. }
                            if matches!(ok.as_ref(), Type::Option(inner) if matches!(inner.as_ref(), Type::String))
                    );
                    let bytes_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. }
                            if matches!(
                                ok.as_ref(),
                                Type::List(elem)
                                    if matches!(
                                        elem.as_ref(),
                                        Type::IntN {
                                            signed: false,
                                            bits: 8
                                        }
                                    ) || matches!(elem.as_ref(), Type::Named(n) if n == "U8")
                            )
                    );
                    let bytes_arg = args.first().is_some_and(|a| {
                        matches!(
                            &a.ty,
                            Type::List(elem)
                                if matches!(
                                    elem.as_ref(),
                                    Type::IntN {
                                        signed: false,
                                        bits: 8
                                    }
                                ) || matches!(elem.as_ref(), Type::Named(n) if n == "U8")
                        )
                    });
                    let expanded_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. } if matches!(ok.as_ref(), Type::Tuple(_))
                    );
                    // Typed `xml.decode<T>` / `decode_bytes<T>` → project then Codable decode.
                    if method == "decode" && args.len() == 1 && !datatree_ok {
                        if let Type::Result { ok, .. } = &expr.ty {
                            return self.lower_typed_tree_decode(
                                &args[0],
                                ok,
                                self.host.encoding.xml_project,
                            );
                        }
                    }
                    if method == "decode_bytes" && args.len() == 1 && bytes_arg {
                        if let Type::Result { ok, .. } = &expr.ty {
                            return self.lower_typed_tree_decode(
                                &args[0],
                                ok,
                                self.host.encoding.xml_project_bytes,
                            );
                        }
                    }
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "parse" if args.len() == 1 && datatree_ok => {
                            (self.host.encoding.xml_parse, vec![self.lower_expr(&args[0])?])
                        }
                        "to_string" if args.len() == 1 && datatree_arg => (
                            self.host.encoding.xml_to_string,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "root" if args.len() == 1 && datatree_ok && datatree_arg => {
                            (self.host.encoding.xml_root, vec![self.lower_expr(&args[0])?])
                        }
                        "expanded_name" if args.len() == 1 && expanded_ok && datatree_arg => (
                            self.host.encoding.xml_expanded_name,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "attribute" if args.len() == 2 && option_string_ok && datatree_arg => (
                            self.host.encoding.xml_attribute,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "content" if args.len() == 1 && datatree_list_ok && datatree_arg => (
                            self.host.encoding.xml_content,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "to_bytes" if args.len() == 1 && bytes_ok && datatree_arg => (
                            self.host.encoding.xml_to_bytes,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "writer" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.xml_writer, vec![file, limits])
                        }
                        "reader" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.xml_reader, vec![file, limits])
                        }
                        _ => {
                            return Err(format!(
                                "jit core call unsupported: core.encoding.xml.{method}"
                            ))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.encoding.cbor" {
                    let datatree_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. }
                            if matches!(
                                ok.as_ref(),
                                Type::Named(n)
                                    if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
                            )
                    );
                    let datatree_arg = args.first().is_some_and(|a| {
                        matches!(
                            &a.ty,
                            Type::Named(n)
                                if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
                        )
                    });
                    let bytes_arg = args.first().is_some_and(|a| {
                        matches!(
                            &a.ty,
                            Type::List(elem)
                                if matches!(
                                    elem.as_ref(),
                                    Type::IntN {
                                        signed: false,
                                        bits: 8
                                    }
                                ) || matches!(elem.as_ref(), Type::Named(n) if n == "U8")
                        )
                    });
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "to_bytes" if args.len() == 1 && datatree_arg => (
                            self.host.encoding.cbor_to_bytes,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "parse" if args.len() == 1 && bytes_arg && datatree_ok => {
                            (self.host.encoding.cbor_parse, vec![self.lower_expr(&args[0])?])
                        }
                        "writer" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.cbor_writer, vec![file, limits])
                        }
                        "reader" if args.len() == 1 || args.len() == 2 => {
                            let file = self.lower_expr(&args[0])?;
                            let limits = if args.len() == 2 {
                                self.lower_expr(&args[1])?
                            } else {
                                self.b.ins().iconst(types::I64, 0)
                            };
                            (self.host.stream.cbor_reader, vec![file, limits])
                        }
                        _ => {
                            return Err(format!(
                                "jit core call unsupported: core.encoding.cbor.{method}"
                            ))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.encoding.toml" || module == "core.encoding.yaml" {
                    let datatree_ok = matches!(
                        &expr.ty,
                        Type::Result { ok, .. }
                            if matches!(
                                ok.as_ref(),
                                Type::Named(n)
                                    if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
                            )
                    );
                    let datatree_arg = args.first().is_some_and(|a| {
                        matches!(
                            &a.ty,
                            Type::Named(n)
                                if matches!(n.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
                        )
                    });
                    if method == "decode" && args.len() == 1 && !datatree_ok {
                        if let Type::Result { ok, .. } = &expr.ty {
                            let parse_host = if module == "core.encoding.toml" {
                                self.host.encoding.toml_parse
                            } else {
                                self.host.encoding.yaml_parse
                            };
                            return self.lower_typed_tree_decode(&args[0], ok, parse_host);
                        }
                    }
                    if method == "to_string" && args.len() == 1 && !datatree_arg {
                        let render_host = if module == "core.encoding.toml" {
                            self.host.encoding.toml_to_string
                        } else {
                            self.host.encoding.yaml_to_string
                        };
                        return self.lower_typed_tree_to_string(&args[0], render_host);
                    }
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match (module.as_str(), method.as_str()) {
                        ("core.encoding.toml", "parse") if args.len() == 1 && datatree_ok => {
                            (self.host.encoding.toml_parse, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.encoding.toml", "to_string") if args.len() == 1 && datatree_arg => (
                            self.host.encoding.toml_to_string,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        ("core.encoding.yaml", "parse") if args.len() == 1 && datatree_ok => {
                            (self.host.encoding.yaml_parse, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.encoding.yaml", "to_string") if args.len() == 1 && datatree_arg => (
                            self.host.encoding.yaml_to_string,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!(
                                "jit core call unsupported: {module}.{method}"
                            ))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.uuid" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "v4" if args.is_empty() => (self.host.encoding.uuid_v4, Vec::new()),
                        "v7" if args.len() == 1 => {
                            (self.host.encoding.uuid_v7, vec![self.lower_expr(&args[0])?])
                        }
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.env" && method == "get" && args.len() == 1 {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.core.env_get, self.b.func);
                    let a0 = self.lower_expr(&args[0])?;
                    let call = self.b.ins().call(host_ref, &[a0]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.env" && method == "set" && args.len() == 2 {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.core.env_set, self.b.func);
                    let a0 = self.lower_expr(&args[0])?;
                    let a1 = self.lower_expr(&args[1])?;
                    let call = self.b.ins().call(host_ref, &[a0, a1]);
                    let handle = self.b.inst_results(call)[0];
                    self.emit_trap_check()?;
                    return Ok(handle);
                }
                if module == "core.env" && method == "unset" && args.len() == 1 {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.core.env_unset, self.b.func);
                    let a0 = self.lower_expr(&args[0])?;
                    let call = self.b.ins().call(host_ref, &[a0]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.env" && method == "vars" && args.is_empty() {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.core.env_vars, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.process" && method == "exit" && args.len() == 1 {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.core.process_exit, self.b.func);
                    let a0 = self.lower_expr(&args[0])?;
                    self.b.ins().call(host_ref, &[a0]);
                    return Ok(self.b.ins().iconst(types::I8, 0));
                }
                if module == "core.process" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "cmd" if args.len() == 1 => {
                            (self.host.process.cmd, vec![self.lower_expr(&args[0])?])
                        }
                        "run" if args.len() == 1 => {
                            (self.host.process.run, vec![self.lower_expr(&args[0])?])
                        }
                        "pipeline" if args.len() == 1 => {
                            (self.host.process.pipeline, vec![self.lower_expr(&args[0])?])
                        }
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.game" && method == "run" {
                    let scene = self.lower_expr(&args[0])?;
                    let zero = self.b.ins().iconst(types::I64, 0);
                    let mut replay = zero;
                    let mut backend = zero;
                    // Named kwargs: replay: / backend: — TIR may pass 1–3 args.
                    if args.len() >= 2 {
                        replay = self.lower_expr(&args[1])?;
                    }
                    if args.len() >= 3 {
                        backend = self.lower_expr(&args[2])?;
                    }
                    let host = self
                        .module
                        .declare_func_in_func(self.host.game.run, self.b.func);
                    let call = self.b.ins().call(host, &[scene, replay, backend]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.raylib" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "window_open" if args.len() == 3 => (
                            self.host.raylib.window_open,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "color" if args.len() == 4 => (
                            self.host.raylib.color,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                                self.lower_expr(&args[3])?,
                            ],
                        ),
                        "set_target_fps" if args.len() == 1 => {
                            (self.host.raylib.set_target_fps, vec![self.lower_expr(&args[0])?])
                        }
                        "key_down" if args.len() == 1 => {
                            (self.host.raylib.key_down, vec![self.lower_expr(&args[0])?])
                        }
                        "begin_drawing" if args.len() == 1 => {
                            (self.host.raylib.begin_drawing, vec![self.lower_expr(&args[0])?])
                        }
                        "clear_background" if args.len() == 1 => {
                            (
                                self.host.raylib.clear_background,
                                vec![self.lower_expr(&args[0])?],
                            )
                        }
                        "draw_rectangle" if args.len() == 5 => (
                            self.host.raylib.draw_rectangle,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                                self.lower_expr(&args[3])?,
                                self.lower_expr(&args[4])?,
                            ],
                        ),
                        "draw_text" if args.len() == 5 => (
                            self.host.raylib.draw_text,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                                self.lower_expr(&args[3])?,
                                self.lower_expr(&args[4])?,
                            ],
                        ),
                        "end_drawing" if args.is_empty() => (self.host.raylib.end_drawing, vec![]),
                        "close_window" if args.len() == 1 => {
                            (self.host.raylib.close_window, vec![self.lower_expr(&args[0])?])
                        }
                        _ => {
                            return Err(format!(
                                "jit core call unsupported: core.raylib.{method}"
                            ))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(match method.as_str() {
                        "window_open" | "color" | "key_down" => self.b.inst_results(call)[0],
                        _ => self.b.ins().iconst(types::I8, 0),
                    });
                }
                if module == "core.compress.gzip" || module == "core.compress.zstd" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match (module.as_str(), method.as_str()) {
                        ("core.compress.gzip", "compress") if args.len() == 1 => {
                            (self.host.compress.gzip_compress, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.compress.gzip", "decompress") if args.len() == 1 => {
                            (self.host.compress.gzip_decompress, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.compress.zstd", "compress") if args.len() == 1 => {
                            (self.host.compress.zstd_compress, vec![self.lower_expr(&args[0])?])
                        }
                        ("core.compress.zstd", "decompress") if args.len() == 1 => {
                            (self.host.compress.zstd_decompress, vec![self.lower_expr(&args[0])?])
                        }
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.archive" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "zip_compress" if args.len() == 2 => (
                            self.host.archive.zip_compress,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "zip_decompress" if args.len() == 1 => {
                            (self.host.archive.zip_decompress, vec![self.lower_expr(&args[0])?])
                        }
                        "tar_add" if args.len() == 3 => (
                            self.host.archive.tar_add,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "tar_get" if args.len() == 2 => (
                            self.host.archive.tar_get,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "tar_names_json" if args.len() == 1 => {
                            (self.host.archive.tar_names_json, vec![self.lower_expr(&args[0])?])
                        }
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.path" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "join" if args.len() == 2 => (
                            self.host.core.path_join,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "parent" if args.len() == 1 => (
                            self.host.core.path_parent_str,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "extension" if args.len() == 1 => (
                            self.host.core.path_extension_str,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "normalize" if args.len() == 1 => (
                            self.host.core.path_normalize_str,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.random" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "seed" if args.len() == 1 => {
                            (self.host.random.seed, vec![self.lower_expr(&args[0])?])
                        }
                        "bool" if args.len() == 1 => {
                            (self.host.random.bool_p, vec![self.lower_expr(&args[0])?])
                        }
                        "float_range" if args.len() == 2 => (
                            self.host.random.float_range,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "normal" if args.len() == 2 => (
                            self.host.random.normal,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "exponential" if args.len() == 1 => {
                            (self.host.random.exponential, vec![self.lower_expr(&args[0])?])
                        }
                        "bytes" if args.len() == 1 => {
                            (self.host.random.bytes, vec![self.lower_expr(&args[0])?])
                        }
                        "weighted_pick" if args.len() == 2 => {
                            if matches!(
                                &args[0].ty,
                                Type::List(inner) | Type::FixedList { elem: inner, .. }
                                    if matches!(inner.as_ref(), Type::IntN { .. })
                            ) {
                                return Err(
                                    "jit random weighted_pick<IntN> needs typed Option lowering"
                                        .to_string(),
                                );
                            }
                            (
                                self.host.random.weighted_pick,
                                vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                            )
                        }
                        "sample" if args.len() == 2 => (
                            self.host.random.sample,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "rng" if args.len() == 1 => {
                            (self.host.random.rng_new, vec![self.lower_expr(&args[0])?])
                        }
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    // Prefer method return shape over TIR `expr.ty` (can be Unit).
                    let ret_ty = self
                        .expr_arith_type_from_op(expr)
                        .unwrap_or_else(|| expr.ty.clone());
                    return Ok(clif_ty(&ret_ty)
                        .map(|_| self.b.inst_results(call)[0])
                        .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0)));
                }
                if module == "core.crypto.random" && method == "bytes" && args.len() == 1 {
                    let count = self.lower_expr(&args[0])?;
                    let host = self
                        .module
                        .declare_func_in_func(self.host.crypto.random_bytes, self.b.func);
                    let call = self.b.ins().call(host, &[count]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.auth" && method == "verify_jwt" && (3..=5).contains(&args.len()) {
                    let token = self.lower_expr(&args[0])?;
                    let key = self.lower_expr(&args[1])?;
                    let audience = self.lower_expr(&args[2])?;
                    let issuer = if args.len() >= 4 {
                        let value = self.lower_expr(&args[3])?;
                        self.b.ins().iadd_imm(value, 1)
                    } else {
                        self.b.ins().iconst(types::I64, 0)
                    };
                    let skew = if args.len() >= 5 {
                        self.lower_expr(&args[4])?
                    } else {
                        self.b.ins().iconst(types::I64, 0)
                    };
                    let host = self
                        .module
                        .declare_func_in_func(self.host.crypto.verify_jwt, self.b.func);
                    let call = self
                        .b
                        .ins()
                        .call(host, &[token, key, audience, issuer, skew]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.auth"
                    && method == "verify_paseto"
                    && (3..=7).contains(&args.len())
                {
                    let token = self.lower_expr(&args[0])?;
                    let key = self.lower_expr(&args[1])?;
                    let audience = self.lower_expr(&args[2])?;
                    let issuer = if args.len() >= 4 {
                        let value = self.lower_expr(&args[3])?;
                        self.b.ins().iadd_imm(value, 1)
                    } else {
                        self.b.ins().iconst(types::I64, 0)
                    };
                    let skew = if args.len() >= 5 {
                        self.lower_expr(&args[4])?
                    } else {
                        self.b.ins().iconst(types::I64, 0)
                    };
                    let footer = if args.len() >= 6 {
                        self.lower_expr(&args[5])?
                    } else {
                        let empty = self
                            .module
                            .declare_func_in_func(self.host.coll.list_new, self.b.func);
                        let call = self.b.ins().call(empty, &[]);
                        self.b.inst_results(call)[0]
                    };
                    let implicit = if args.len() >= 7 {
                        self.lower_expr(&args[6])?
                    } else {
                        let empty = self
                            .module
                            .declare_func_in_func(self.host.coll.list_new, self.b.func);
                        let call = self.b.ins().call(empty, &[]);
                        self.b.inst_results(call)[0]
                    };
                    let host = self
                        .module
                        .declare_func_in_func(self.host.crypto.verify_paseto, self.b.func);
                    let call = self.b.ins().call(
                        host,
                        &[token, key, audience, issuer, skew, footer, implicit],
                    );
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.vault" || module == "core.vault.expert" {
                    let tag = crate::Crypto::vault_key_tag(&expr.ty)
                        .or_else(|| {
                            args.first()
                                .and_then(|arg| crate::Crypto::vault_key_tag(&arg.ty))
                        })
                        .unwrap_or(1);
                    let tag_val = self.b.ins().iconst(types::I64, tag);
                    let (host_id, mut arg_values): (FuncId, Vec<Value>) =
                        match (module.as_str(), method.as_str(), args.as_slice()) {
                            ("core.vault", "get", [name]) => {
                                (self.host.crypto.vault_get, vec![self.lower_expr(name)?])
                            }
                            ("core.vault", "current", [name]) => (
                                self.host.crypto.vault_current,
                                vec![self.lower_expr(name)?, tag_val],
                            ),
                            ("core.vault", "versions", [name]) => (
                                self.host.crypto.vault_versions,
                                vec![self.lower_expr(name)?, tag_val],
                            ),
                            ("core.vault", "prepare_generate", [name]) => (
                                self.host.crypto.vault_prepare_generate,
                                vec![self.lower_expr(name)?, tag_val],
                            ),
                            ("core.vault", "prepare_rotate", [name]) => (
                                self.host.crypto.vault_prepare_rotate,
                                vec![self.lower_expr(name)?, tag_val],
                            ),
                            ("core.vault", "prepare_store", [name, key]) => (
                                self.host.crypto.vault_prepare_store,
                                vec![self.lower_expr(name)?, self.lower_expr(key)?, tag_val],
                            ),
                            ("core.vault", "prepare_retire", [key_ref, reason]) => (
                                self.host.crypto.vault_prepare_retire,
                                vec![
                                    self.lower_expr(key_ref)?,
                                    self.lower_expr(reason)?,
                                    tag_val,
                                ],
                            ),
                            ("core.vault", "prepare_revoke", [key_ref, reason]) => (
                                self.host.crypto.vault_prepare_revoke,
                                vec![
                                    self.lower_expr(key_ref)?,
                                    self.lower_expr(reason)?,
                                    tag_val,
                                ],
                            ),
                            ("core.vault", "authorize_write", [plan, reason]) => (
                                self.host.crypto.vault_authorize_write,
                                vec![self.lower_expr(plan)?, self.lower_expr(reason)?, tag_val],
                            ),
                            ("core.vault", "commit_generate", [write, plan]) => (
                                self.host.crypto.vault_commit_generate,
                                vec![self.lower_expr(write)?, self.lower_expr(plan)?, tag_val],
                            ),
                            ("core.vault", "commit_store", [write, plan]) => (
                                self.host.crypto.vault_commit_store,
                                vec![self.lower_expr(write)?, self.lower_expr(plan)?, tag_val],
                            ),
                            ("core.vault", "commit_rotate", [write, plan]) => (
                                self.host.crypto.vault_commit_rotate,
                                vec![self.lower_expr(write)?, self.lower_expr(plan)?, tag_val],
                            ),
                            ("core.vault", "commit_retire", [write, plan]) => (
                                self.host.crypto.vault_commit_retire,
                                vec![self.lower_expr(write)?, self.lower_expr(plan)?, tag_val],
                            ),
                            ("core.vault", "commit_revoke", [write, plan]) => (
                                self.host.crypto.vault_commit_revoke,
                                vec![self.lower_expr(write)?, self.lower_expr(plan)?, tag_val],
                            ),
                            ("core.vault", "load", [key_ref]) => (
                                self.host.crypto.vault_load,
                                vec![self.lower_expr(key_ref)?, tag_val],
                            ),
                            ("core.vault", "status", [key_ref]) => (
                                self.host.crypto.vault_status,
                                vec![self.lower_expr(key_ref)?, tag_val],
                            ),
                            ("core.vault", "export_to_recipients", [key_ref, recipients]) => (
                                self.host.crypto.vault_export_to_recipients,
                                vec![
                                    self.lower_expr(key_ref)?,
                                    self.lower_expr(recipients)?,
                                    tag_val,
                                ],
                            ),
                            ("core.vault", "export_to_passphrase", [key_ref, passphrase]) => (
                                self.host.crypto.vault_export_to_passphrase,
                                vec![
                                    self.lower_expr(key_ref)?,
                                    self.lower_expr(passphrase)?,
                                    tag_val,
                                ],
                            ),
                            ("core.vault", "prepare_import_wrapped", [name, wrapped, unlock]) => (
                                self.host.crypto.vault_prepare_import_wrapped,
                                vec![
                                    self.lower_expr(name)?,
                                    self.lower_expr(wrapped)?,
                                    self.lower_expr(unlock)?,
                                    tag_val,
                                ],
                            ),
                            ("core.vault", "authorize_wrapped_import", [plan, reason]) => (
                                self.host.crypto.vault_authorize_wrapped_import,
                                vec![self.lower_expr(plan)?, self.lower_expr(reason)?, tag_val],
                            ),
                            ("core.vault", "commit_import_wrapped", [write, plan]) => (
                                self.host.crypto.vault_commit_import_wrapped,
                                vec![self.lower_expr(write)?, self.lower_expr(plan)?, tag_val],
                            ),
                            ("core.vault.expert", "prepare_import_signing", [name, bytes]) => (
                                self.host.crypto.vault_expert_prepare_import_signing,
                                vec![self.lower_expr(name)?, self.lower_expr(bytes)?],
                            ),
                            ("core.vault.expert", "commit_import_signing", [write, plan]) => (
                                self.host.crypto.vault_expert_commit_import_signing,
                                vec![self.lower_expr(write)?, self.lower_expr(plan)?],
                            ),
                            _ => {
                                return Err(format!(
                                    "jit core call unsupported: {module}.{method}"
                                ))
                            }
                        };
                    if matches!(method.as_str(), "get") {
                        arg_values.truncate(1);
                    }
                    let host = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host, &arg_values);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.math" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "sin" if args.len() == 1 => {
                            (self.host.core.math_sin, vec![self.lower_expr(&args[0])?])
                        }
                        "cos" if args.len() == 1 => {
                            (self.host.core.math_cos, vec![self.lower_expr(&args[0])?])
                        }
                        "exp" if args.len() == 1 => {
                            (self.host.core.math_exp, vec![self.lower_expr(&args[0])?])
                        }
                        "degrees" if args.len() == 1 => {
                            (self.host.core.math_degrees, vec![self.lower_expr(&args[0])?])
                        }
                        "radians" if args.len() == 1 => {
                            (self.host.core.math_radians, vec![self.lower_expr(&args[0])?])
                        }
                        "is_finite" if args.len() == 1 => {
                            (self.host.core.math_is_finite, vec![self.lower_expr(&args[0])?])
                        }
                        "sign" if args.len() == 1 => {
                            (self.host.core.math_sign, vec![self.lower_expr(&args[0])?])
                        }
                        "atan2" if args.len() == 2 => (
                            self.host.core.math_atan2,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "hypot" if args.len() == 2 => (
                            self.host.core.math_hypot,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "lerp" if args.len() == 3 => (
                            self.host.core.math_lerp,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "checked_add" if args.len() == 2 => (
                            self.host.core.math_checked_add,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "saturating_add" if args.len() == 2 => (
                            self.host.core.math_saturating_add,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "wrapping_add" if args.len() == 2 => (
                            self.host.core.math_wrapping_add,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "int_pow" if args.len() == 2 => (
                            self.host.core.math_int_pow,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "gcd" if args.len() == 2 => (
                            self.host.core.math_gcd,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "lcm" if args.len() == 2 => (
                            self.host.core.math_lcm,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "decimal" if args.len() == 1 => (
                            self.host.num.decimal_from_str,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => return Err("jit core call unsupported".to_string()),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    let result = self.b.inst_results(call)[0];
                    if method == "decimal" {
                        self.emit_trap_check()?;
                    }
                    return Ok(result);
                }
                if module == "core.tasks" && method == "channel" && args.is_empty() {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.channel_new, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.tasks" && method == "channel" && args.len() == 1 {
                    let cap = self.lower_expr(&args[0])?;
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.channel_bounded, self.b.func);
                    let call = self.b.ins().call(host_ref, &[cap]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.tasks" && method == "after" {
                    let ms = self.lower_expr(&args[0])?;
                    let value = if args.len() >= 2 {
                        self.lower_expr(&args[1])?
                    } else {
                        self.b.ins().iconst(types::I64, 0)
                    };
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.after_value, self.b.func);
                    let call = self.b.ins().call(host_ref, &[ms, value]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.tasks" && method == "interval" && args.len() == 1 {
                    let ms = self.lower_expr(&args[0])?;
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.interval, self.b.func);
                    let call = self.b.ins().call(host_ref, &[ms]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.time" && method == "now" && args.is_empty() {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.conc.time_now, self.b.func);
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
                if module == "core.text" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "lower" if args.len() == 1 => {
                            (self.host.text.lower, vec![self.lower_expr(&args[0])?])
                        }
                        "upper" if args.len() == 1 => {
                            (self.host.text.upper, vec![self.lower_expr(&args[0])?])
                        }
                        "trim" if args.len() == 1 => {
                            (self.host.str_trim, vec![self.lower_expr(&args[0])?])
                        }
                        "scalar_count" if args.len() == 1 => {
                            (self.host.str_len, vec![self.lower_expr(&args[0])?])
                        }
                        "byte_count" if args.len() == 1 => {
                            (self.host.str_byte_len, vec![self.lower_expr(&args[0])?])
                        }
                        "graphemes" if args.len() == 1 => {
                            (self.host.text.graphemes, vec![self.lower_expr(&args[0])?])
                        }
                        "words" if args.len() == 1 => {
                            (self.host.text.words, vec![self.lower_expr(&args[0])?])
                        }
                        "sentences" if args.len() == 1 => {
                            (self.host.text.sentences, vec![self.lower_expr(&args[0])?])
                        }
                        "nfc" if args.len() == 1 => {
                            (self.host.text.nfc, vec![self.lower_expr(&args[0])?])
                        }
                        "nfkc" if args.len() == 1 => {
                            (self.host.text.nfkc, vec![self.lower_expr(&args[0])?])
                        }
                        "nfd" if args.len() == 1 => {
                            (self.host.text.nfd, vec![self.lower_expr(&args[0])?])
                        }
                        "nfkd" if args.len() == 1 => {
                            (self.host.text.nfkd, vec![self.lower_expr(&args[0])?])
                        }
                        "caseless_eq" if args.len() == 2 => (
                            self.host.text.caseless_eq,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "display_width" if args.len() == 1 => (
                            self.host.text.display_width,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "display_width" if args.len() == 2 => (
                            self.host.text.display_width_policy,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "is_alphabetic" if args.len() == 1 => (
                            self.host.text.is_alphabetic,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "is_numeric" if args.len() == 1 => (
                            self.host.text.is_numeric,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "pad_start" if args.len() == 3 => (
                            self.host.text.pad_start,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "center" if args.len() == 3 => (
                            self.host.text.center,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "starts_any" if args.len() == 2 => (
                            self.host.text.starts_any,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "char_indices" if args.len() == 1 => (
                            self.host.text.char_indices,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!("jit core.text call unsupported: {method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module.starts_with("core.sketch.") {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match (module.as_str(), method.as_str()) {
                        ("core.sketch.hll", "new") if args.is_empty() => {
                            (self.host.sketch.hll_new, Vec::new())
                        }
                        ("core.sketch.tdigest", "new") if args.is_empty() => {
                            (self.host.sketch.tdigest_new, Vec::new())
                        }
                        ("core.sketch.cms", "new") if args.is_empty() => {
                            (self.host.sketch.cms_new, Vec::new())
                        }
                        ("core.sketch.reservoir", "new") if args.len() == 1 => {
                            (self.host.sketch.reservoir_new, vec![self.lower_expr(&args[0])?])
                        }
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.args" && method == "spec" && args.is_empty() {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.args.spec, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.text.unicode" && args.len() == 1 {
                    let host_id = match method.as_str() {
                        // str_len already counts Unicode scalars via jet_rt.
                        "scalar_count" => self.host.str_len,
                        "byte_count" => self.host.str_byte_len,
                        "is_ascii" => self.host.str_is_ascii,
                        "lower" => self.host.str_to_lower,
                        "upper" => self.host.str_to_upper,
                        "scalars" => self.host.str_scalar_strings,
                        _ => {
                            return Err(format!(
                                "jit core.text.unicode call unsupported: {method}"
                            ))
                        }
                    };
                    let text = self.lower_expr(&args[0])?;
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &[text]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.fmt" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "number" | "bytes" | "duration" | "ordinal" if args.len() == 1 => {
                            let host = match method.as_str() {
                                "number" => self.host.fmt.number,
                                "bytes" => self.host.fmt.bytes,
                                "duration" => self.host.fmt.duration,
                                _ => self.host.fmt.ordinal,
                            };
                            (host, vec![self.lower_expr(&args[0])?])
                        }
                        "decimal" | "percent" if args.len() == 2 => {
                            let host = if method == "decimal" {
                                self.host.fmt.decimal
                            } else {
                                self.host.fmt.percent
                            };
                            (
                                host,
                                vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                            )
                        }
                        "plural" if args.len() == 3 => (
                            self.host.fmt.plural,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "pad_left" | "pad_right" | "pad_center" if args.len() == 3 => {
                            let host = match method.as_str() {
                                "pad_left" => self.host.fmt.pad_left,
                                "pad_right" => self.host.fmt.pad_right,
                                _ => self.host.fmt.pad_center,
                            };
                            (
                                host,
                                vec![
                                    self.lower_expr(&args[0])?,
                                    self.lower_expr(&args[1])?,
                                    self.lower_expr(&args[2])?,
                                ],
                            )
                        }
                        _ => {
                            return Err(format!("jit core call unsupported: core.fmt.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
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

                if module == "core.time.date" || module == "core.time.datetime" || module == "core.time" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match (module.as_str(), method.as_str()) {
                        ("core.time.date", "new") if args.len() == 3 => (
                            self.host.time.date_new,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?, self.lower_expr(&args[2])?],
                        ),
                        ("core.time.date", "today") if args.is_empty() => (self.host.time.date_today, Vec::new()),
                        ("core.time.date", "parse") if args.len() == 1 => (self.host.time.date_parse, vec![self.lower_expr(&args[0])?]),
                        ("core.time.datetime", "from_timestamp") if args.len() == 1 => (
                            self.host.time.datetime_from_timestamp,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        ("core.time.datetime", "now") if args.is_empty() => (self.host.time.datetime_now, Vec::new()),
                        ("core.time", "parse_rfc3339") if args.len() == 1 => (
                            self.host.time.parse_rfc3339,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        ("core.time", "from_unix_ms") if args.len() == 1 => (
                            self.host.time.from_unix_ms,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        ("core.time", "utc") if args.is_empty() => (self.host.time.utc, Vec::new()),
                        ("core.time", "period_months") if args.len() == 1 => (
                            self.host.time.period_months,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        ("core.time", "instant") if args.is_empty() => (self.host.time.instant, Vec::new()),
                        ("core.time", "zoned") if args.len() == 2 => (
                            self.host.time.zoned,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        _ => return Err(format!("jit core call unsupported: {module}.{method}")),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "jet.regex" || module == "core.regex" {
                    let widen_bool = |this: &mut Self, e: &TExpr| -> Result<Value, String> {
                        let v = this.lower_expr(e)?;
                        if this.b.func.dfg.value_type(v) == types::I8 {
                            Ok(this.b.ins().uextend(types::I64, v))
                        } else {
                            Ok(v)
                        }
                    };
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "flags" if args.len() == 3 => (
                            self.host.text.regex_flags,
                            vec![
                                widen_bool(self, &args[0])?,
                                widen_bool(self, &args[1])?,
                                widen_bool(self, &args[2])?,
                            ],
                        ),
                        "is_match" if args.len() == 2 => (
                            self.host.text.regex_is_match,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "find" if args.len() == 2 => (
                            self.host.text.regex_find,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "find_all" if args.len() == 2 => (
                            self.host.text.regex_find_all,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "match" if args.len() == 2 => (
                            self.host.text.regex_match,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "replace_all" if args.len() == 3 => (
                            self.host.text.regex_replace_all,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?, self.lower_expr(&args[2])?],
                        ),
                        "split" if args.len() == 2 => (
                            self.host.text.regex_split,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "compile" if args.len() == 1 => (
                            self.host.text.regex_compile,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "compile_with" if args.len() == 2 => (
                            self.host.text.regex_compile_with,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        _ => return Err(format!("jit core call unsupported: {module}.{method}")),
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "jet.db" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "open_memory" if args.is_empty() => {
                            (self.host.db.open_memory, Vec::new())
                        }
                        "open" if args.len() == 1 => {
                            (self.host.db.open, vec![self.lower_expr(&args[0])?])
                        }
                        "migrate" if args.len() == 3 => (
                            self.host.db.migrate,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "transaction" if args.len() == 3 => (
                            self.host.db.transaction,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "params" if args.len() == 1 => {
                            (self.host.db.params, vec![self.lower_expr(&args[0])?])
                        }
                        "row_int" if args.len() == 2 => (
                            self.host.db.row_int,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "row_text" if args.len() == 2 => (
                            self.host.db.row_text,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.url" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "parse" if args.len() == 1 => {
                            (self.host.net.url_parse, vec![self.lower_expr(&args[0])?])
                        }
                        "file" if args.len() == 1 => {
                            (self.host.net.url_file, vec![self.lower_expr(&args[0])?])
                        }
                        "data" if args.len() == 2 => (
                            self.host.net.url_data,
                            vec![self.lower_expr(&args[0])?, self.lower_expr(&args[1])?],
                        ),
                        "query" if args.len() == 1 => {
                            (self.host.net.url_query, vec![self.lower_expr(&args[0])?])
                        }
                        "percent_encode" if args.len() == 1 => (
                            self.host.net.url_percent_encode,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "percent_decode" if args.len() == 1 => (
                            self.host.net.url_percent_decode,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.mime" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "parse" if args.len() == 1 => {
                            (self.host.net.mime_parse, vec![self.lower_expr(&args[0])?])
                        }
                        "from_extension" if args.len() == 1 => (
                            self.host.net.mime_from_extension,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "extension" if args.len() == 1 => (
                            self.host.net.mime_extension,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.browser" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "profile" if args.len() == 1 => (
                            self.host.net.browser_profile,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "timeout" if args.len() == 1 => (
                            self.host.net.browser_timeout,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if module == "core.email" {
                    let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                        "address" if args.len() == 1 => (
                            self.host.net.email_address,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "attachment" if args.len() == 3 => (
                            self.host.net.email_attachment,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                            ],
                        ),
                        "message" if args.len() == 7 => (
                            self.host.net.email_message,
                            vec![
                                self.lower_expr(&args[0])?,
                                self.lower_expr(&args[1])?,
                                self.lower_expr(&args[2])?,
                                self.lower_expr(&args[3])?,
                                self.lower_expr(&args[4])?,
                                self.lower_expr(&args[5])?,
                                self.lower_expr(&args[6])?,
                            ],
                        ),
                        "serialize" if args.len() == 1 => (
                            self.host.net.email_serialize,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        "smtp" if args.len() == 1 => (
                            self.host.net.email_smtp,
                            vec![self.lower_expr(&args[0])?],
                        ),
                        _ => {
                            return Err(format!("jit core call unsupported: {module}.{method}"))
                        }
                    };
                    let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host_ref, &arg_vals);
                    return Ok(self.b.inst_results(call)[0]);
                }
                Err(format!("jit core call unsupported: {module}.{method}"))
            }
            TExprKind::CoreClosureCall { kind } => match kind {
                TCoreClosureKind::Spawn { .. } => self.lower_spawn(),
                TCoreClosureKind::Serve { .. } => {
                    Err("jit http serve closure unsupported".to_string())
                }
                TCoreClosureKind::Guard { executable, .. } => {
                    let id = super::functions_compile::lower_callable_lambda(
                        self.module,
                        self.host,
                        self.meta,
                        executable,
                        self.func_ids,
                        self.spawn_func_ids,
                        self.spawn_lambdas,
                        self.spawn_site,
                        self.runtime,
                    )?;
                    self.scope_guards.push(id);
                    Ok(self.b.ins().iconst(types::I64, 0))
                }
                TCoreClosureKind::OnCommit { executable, .. } => {
                    let id = super::functions_compile::lower_callable_lambda(
                        self.module,
                        self.host,
                        self.meta,
                        executable,
                        self.func_ids,
                        self.spawn_func_ids,
                        self.spawn_lambdas,
                        self.spawn_site,
                        self.runtime,
                    )?;
                    let Some(frame) = self.txn_stack.last_mut() else {
                        return Err("jit on_commit outside transaction".to_string());
                    };
                    frame.on_commit.push(id);
                    Ok(self.b.ins().iconst(types::I64, 0))
                }
                TCoreClosureKind::OnRollback { executable, .. } => {
                    let id = super::functions_compile::lower_callable_lambda(
                        self.module,
                        self.host,
                        self.meta,
                        executable,
                        self.func_ids,
                        self.spawn_func_ids,
                        self.spawn_lambdas,
                        self.spawn_site,
                        self.runtime,
                    )?;
                    let Some(frame) = self.txn_stack.last_mut() else {
                        return Err("jit on_rollback outside transaction".to_string());
                    };
                    frame.on_rollback.push(id);
                    Ok(self.b.ins().iconst(types::I64, 0))
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
                let mut after_flat = Vec::new();
                for (ms, value) in afters {
                    after_flat.push(self.lower_expr(ms)?);
                    after_flat.push(match value {
                        Some(v) => self.lower_expr(v)?,
                        None => self.b.ins().iconst(types::I64, 0),
                    });
                }
                let recv_list = self.lower_i64_value_list(&recv_vals)?;
                let after_list = self.lower_i64_value_list(&after_flat)?;
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
                    let is_result_option = Self::uses_result_option_abi(value);
                    let present = if is_result_option {
                        let is_ok = self
                            .module
                            .declare_func_in_func(self.host.result_is_ok, self.b.func);
                        let call = self.b.ins().call(is_ok, &[status]);
                        self.b.inst_results(call)[0]
                    } else {
                        let zero = self.b.ins().iconst(types::I64, 0);
                        self.b.ins().icmp(IntCC::SignedGreaterThan, status, zero)
                    };
                    self.b.ins().brif(present, ok_block, &[], fail_block, &[]);
                    self.b.switch_to_block(ok_block);
                    self.b.seal_block(ok_block);
                    let val = if is_result_option
                        || matches!(&value.kind, TExprKind::OverflowOpt { .. })
                    {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.result_get_i64, self.b.func);
                        let call = self.b.ins().call(host, &[status]);
                        self.b.inst_results(call)[0]
                    } else if let Type::Option(inner) = &value.ty {
                        self.unpack_option_payload(status, inner)?
                    } else if let Some(Type::Option(inner)) =
                        Self::recover_core_return_ty(value)
                    {
                        self.unpack_option_payload(status, &inner)?
                    } else {
                        let one = self.b.ins().iconst(types::I64, 1);
                        self.b.ins().isub(status, one)
                    };
                    self.b.ins().jump(merge, &[val]);
                    self.b.switch_to_block(fail_block);
                    self.b.seal_block(fail_block);
                    match fallback {
                        TOrFallback::Value(e) => {
                            let fb = self.lower_expr(e)?;
                            self.b.ins().jump(merge, &[fb]);
                        }
                        TOrFallback::Break => {
                            self.emit_loop_fallback(None, "break", false)?;
                        }
                        TOrFallback::Continue => {
                            self.emit_loop_fallback(None, "continue", true)?;
                        }
                        TOrFallback::BreakLabel(name) => {
                            self.emit_loop_fallback(Some(name), "break", false)?;
                        }
                        TOrFallback::ContinueLabel(name) => {
                            self.emit_loop_fallback(Some(name), "continue", true)?;
                        }
                        TOrFallback::Return(None) => {
                            self.emit_shield_leaves_to(0);
                            self.b.ins().return_(&[]);
                        }
                        TOrFallback::Return(Some(e)) => {
                            let val = self.lower_expr(e)?;
                            self.emit_shield_leaves_to(0);
                            self.b.ins().return_(&[val]);
                        }
                        TOrFallback::Panic { .. } => {
                            let zero = self.b.ins().iconst(types::I64, 0);
                            let host_ref = self
                                .module
                                .declare_func_in_func(self.host.trap_panic, self.b.func);
                            self.b.ins().call(host_ref, &[zero]);
                            self.emit_trap_check()?;
                            let dummy = self.b.ins().iconst(types::I64, 0);
                            self.b.ins().jump(merge, &[dummy]);
                        }
                    }
                    self.b.switch_to_block(merge);
                    self.b.seal_block(merge);
                    return Ok(self.b.block_params(merge)[0]);
                }
                // Channel receive-status encoding stays on lower_result_receive_status;
                // Result ?? uses the Result handle + result_is_ok / result_payload.
                if let Ok(status) = self.lower_result_receive_status(value) {
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
                        TOrFallback::Panic { .. } => {
                            let line = self.b.ins().iconst(types::I32, 1);
                            let host_ref = self.module.declare_func_in_func(
                                self.host.conc.panic_channel_closed,
                                self.b.func,
                            );
                            let call = self.b.ins().call(host_ref, &[line]);
                            let panic_val = self.b.inst_results(call)[0];
                            self.emit_trap_check()?;
                            self.b.ins().jump(merge, &[panic_val]);
                        }
                        TOrFallback::Break => {
                            self.emit_loop_fallback(None, "break", false)?;
                        }
                        TOrFallback::Continue => {
                            self.emit_loop_fallback(None, "continue", true)?;
                        }
                        TOrFallback::BreakLabel(name) => {
                            self.emit_loop_fallback(Some(name), "break", false)?;
                        }
                        TOrFallback::ContinueLabel(name) => {
                            self.emit_loop_fallback(Some(name), "continue", true)?;
                        }
                        TOrFallback::Value(_) | TOrFallback::Return(_) => {
                            return Err("jit or-fallback unsupported".to_string());
                        }
                    }
                    self.b.switch_to_block(merge);
                    self.b.seal_block(merge);
                    return Ok(self.b.block_params(merge)[0]);
                }
                let handle = self.lower_expr(value)?;
                let ok_ty = value
                    .ty
                    .unwrap_result()
                    .map(|(ok, _)| ok.clone())
                    .or_else(|| Self::result_ok_ty_recover(value))
                    .ok_or_else(|| "jit result ?? operand is not Result".to_string())?;
                let is_unit = matches!(&ok_ty, Type::Named(n) if n == "Unit" || n == "Void")
                    || matches!(&ok_ty, Type::Tuple(items) if items.is_empty());
                let ret_ty = if is_unit {
                    types::I8
                } else {
                    self.meta
                        .clif_ty(&ok_ty)
                        .or_else(|| clif_ty(&ok_ty))
                        .or_else(|| self.meta.clif_ty(&expr.ty))
                        .or_else(|| clif_ty(&expr.ty))
                        .ok_or_else(|| {
                            format!("jit result ?? type unsupported: ok={ok_ty:?} expr={:?}", expr.ty)
                        })?
                };
                let status_ref = self
                    .module
                    .declare_func_in_func(self.host.result_is_ok, self.b.func);
                let status_call = self.b.ins().call(status_ref, &[handle]);
                let is_ok = self.b.inst_results(status_call)[0];
                let ok_block = self.b.create_block();
                let fail_block = self.b.create_block();
                let merge = self.b.create_block();
                self.b.append_block_param(merge, ret_ty);
                self.b.ins().brif(is_ok, ok_block, &[], fail_block, &[]);
                self.b.switch_to_block(ok_block);
                self.b.seal_block(ok_block);
                let ok_val = self.result_payload(handle, &ok_ty)?;
                self.b.ins().jump(merge, &[ok_val]);
                self.b.switch_to_block(fail_block);
                self.b.seal_block(fail_block);
                match fallback {
                    TOrFallback::Value(e) => {
                        let fb = self.lower_expr(e)?;
                        self.b.ins().jump(merge, &[fb]);
                    }
                    TOrFallback::Return(None) => {
                        self.emit_shield_leaves_to(0);
                        self.b.ins().return_(&[]);
                    }
                    TOrFallback::Return(Some(e)) => {
                        let val = self.lower_expr(e)?;
                        self.emit_shield_leaves_to(0);
                        self.b.ins().return_(&[val]);
                    }
                    TOrFallback::Panic { .. } => {
                        let zero = self.b.ins().iconst(types::I64, 0);
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.trap_panic, self.b.func);
                        self.b.ins().call(host_ref, &[zero]);
                        self.emit_trap_check()?;
                        let dummy = if ret_ty == types::F64 {
                            self.b.ins().f64const(0.0)
                        } else {
                            self.b.ins().iconst(ret_ty, 0)
                        };
                        self.b.ins().jump(merge, &[dummy]);
                    }
                    TOrFallback::Break => {
                        self.emit_loop_fallback(None, "break", false)?;
                    }
                    TOrFallback::Continue => {
                        self.emit_loop_fallback(None, "continue", true)?;
                    }
                    TOrFallback::BreakLabel(name) => {
                        self.emit_loop_fallback(Some(name), "break", false)?;
                    }
                    TOrFallback::ContinueLabel(name) => {
                        self.emit_loop_fallback(Some(name), "continue", true)?;
                    }
                }
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                Ok(self.b.block_params(merge)[0])
            }
            TExprKind::ListLit(elems) => self.lower_list_lit(&expr.ty, elems),
            TExprKind::MapLit(entries) => self.lower_map_lit(entries),
            TExprKind::Index {
                base,
                index,
                is_map,
                line,
            } => {
                if *is_map {
                    let map = self.lower_expr(base)?;
                    let key = self.lower_expr(index)?;
                    let line_const = self.b.ins().iconst(types::I32, *line as i64);
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.map_get, self.b.func);
                    let call = self.b.ins().call(host_ref, &[map, key, line_const]);
                    let raw = self.b.inst_results(call)[0];
                    self.emit_trap_check()?;
                    let val = match self.meta.clif_ty(&expr.ty) {
                        Some(types::I32) => self.b.ins().ireduce(types::I32, raw),
                        Some(types::I8) => self.b.ins().ireduce(types::I8, raw),
                        Some(types::F64) => self.b.ins().bitcast(
                            types::F64,
                            Self::scalar_bitcast_memflags(),
                            raw,
                        ),
                        _ => raw,
                    };
                    return Ok(val);
                }
                let list = self.lower_expr(base)?;
                let idx = self.lower_expr(index)?;
                let line_const = self.b.ins().iconst(types::I32, *line as i64);
                let (list, idx) = if Self::is_view_mut_ty(&base.ty) {
                    let (inner, start, _) = self.unpack_view_mut(list)?;
                    (inner, self.b.ins().iadd(start, idx))
                } else {
                    (list, idx)
                };
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
                // Jet ranges are inclusive; heap list_slice is exclusive-end.
                let one = self.b.ins().iconst(types::I64, 1);
                let end_excl = self.b.ins().iadd(e, one);
                let line_const = self.b.ins().iconst(types::I32, *line as i64);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_slice, self.b.func);
                let call = self.b.ins().call(host_ref, &[list, s, end_excl, line_const]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
            }
            TExprKind::BuiltinMethod { recv, op, args } => {
                self.lower_builtin_method(recv, op, args, &expr.ty)
            }
            TExprKind::StructLit {
                fields, as_trait, ..
            } => self.lower_struct_lit(fields, as_trait.as_ref()),
            TExprKind::TupleLit { fields, .. } => self.lower_tuple_lit(fields),
            TExprKind::Field {
                recv, field, ..
            } => {
                let mut handle = self.lower_expr(recv)?;
                let record_ty = match &recv.ty {
                    Type::Apply { name, args } if name == "ViewMut" && args.len() == 1 => {
                        let (list, start, _) = self.unpack_view_mut(handle)?;
                        let line = self.b.ins().iconst(types::I32, 1);
                        let get = self
                            .module
                            .declare_func_in_func(self.host.coll.list_get, self.b.func);
                        let call = self.b.ins().call(get, &[list, start, line]);
                        handle = self.b.inst_results(call)[0];
                        self.emit_trap_check()?;
                        args[0].clone()
                    }
                    other => other.clone(),
                };
                let type_name = record_type_key(&record_ty)
                    .or_else(|| self.method_struct.clone());
                // GameFrame.input / .index — TIR may erase the frame param to Int
                // inside spawn-lambda bodies; treat input/index on Int as GameFrame.
                if matches!(field.as_str(), "input" | "index")
                    && (matches!(&record_ty, Type::Int)
                        || matches!(
                            type_name.as_deref().or_else(|| match &record_ty {
                                Type::Named(n) => Some(n.as_str()),
                                _ => None,
                            }),
                            Some("GameFrame")
                        ))
                {
                    if field == "input" {
                        return Ok(handle);
                    }
                    let host = self
                        .module
                        .declare_func_in_func(self.host.game.frame_index, self.b.func);
                    let call = self.b.ins().call(host, &[handle]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                // GameScene.assets / .input — identity projection onto the scene handle.
                if matches!(
                    type_name.as_deref().or_else(|| match &record_ty {
                        Type::Named(n) => Some(n.as_str()),
                        _ => None,
                    }),
                    Some("GameScene")
                ) && matches!(field.as_str(), "assets" | "input")
                {
                    return Ok(handle);
                }
                let type_name = type_name.ok_or_else(|| {
                    format!("jit field recv type: field `{field}` on {record_ty:?}")
                })?;
                if type_name == "HttpShutdownReport" {
                    let field_id = match field.as_str() {
                        "accepted" => 0,
                        "overloaded" => 1,
                        "completed" => 2,
                        "cancelled" => 3,
                        other => {
                            return Err(format!(
                                "jit field `{other}` on `HttpShutdownReport`"
                            ));
                        }
                    };
                    let field_v = self.b.ins().iconst(types::I64, field_id);
                    let host = self.module.declare_func_in_func(
                        self.host.net_http.http_shutdown_report_field,
                        self.b.func,
                    );
                    let call = self.b.ins().call(host, &[handle, field_v]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                // TIR may leave CORE struct fields as Int when cx.struct_fields
                // lacks the type; recover the real ABI type for get_*/print.
                let field_ty = self.meta.struct_field_ty(&type_name, field)
                    .or_else(|| core_struct_field_type(&type_name, field))
                    .unwrap_or_else(|| expr.ty.clone());
                self.lower_record_field(handle, &type_name, field, &field_ty)
            }
            TExprKind::MethodCall {
                recv,
                method,
                args,
                ..
            } => {
                if method.name == "compare"
                    && args.len() == 1
                    && matches!(&recv.ty, Type::Int)
                {
                    let left = self.lower_expr(recv)?;
                    let right = self.lower_call_arg(&args[0])?;
                    let equal = self.bool_from_icmp(IntCC::Equal, left, right);
                    let greater =
                        self.bool_from_icmp(IntCC::SignedGreaterThan, left, right);
                    let less_disc = self.b.ins().iconst(types::I64, 0);
                    let equal_disc = self.b.ins().iconst(types::I64, 1);
                    let greater_disc = self.b.ins().iconst(types::I64, 2);
                    let unequal =
                        self.b.ins().select(greater, greater_disc, less_disc);
                    return Ok(self.b.ins().select(equal, equal_disc, unequal));
                }
                if matches!(&recv.ty, Type::TraitObject(_)) {
                    return self.lower_trait_object_method(recv, method, args, &expr.ty);
                }
                if method.name == "apply"
                    && args.len() == 1
                    && record_type_key(&recv.ty).is_some_and(|base| {
                        self.meta
                            .struct_layout(&format!("{base}.Patch"))
                            .is_some()
                    })
                {
                    return self.lower_patch_apply(recv, &args[0]);
                }
                if method.name == "merge"
                    && args.len() == 1
                    && record_type_key(&recv.ty)
                        .is_some_and(|name| name.ends_with(".Patch"))
                {
                    return self.lower_patch_merge(recv, &args[0]);
                }
                let key = self.method_key(&recv.ty, method)
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
                owner,
                owner_type,
                method,
                args,
            } => {
                if method.name == "diff" && args.len() == 2 {
                    if let TStaticOwner::User(base) = owner {
                        if self
                            .meta
                            .struct_layout(&format!("{base}.Patch"))
                            .is_some()
                        {
                            return self.lower_patch_diff(base, &args[0], &args[1]);
                        }
                    }
                }
                // D-COLLBREADTH1=A: `Deque.new()` → empty VecDeque handle.
                let is_deque_new = method.name == "new"
                    && args.is_empty()
                    && matches!(
                        owner,
                        TStaticOwner::Prelude { path, .. }
                            if path == "std::collections::VecDeque"
                                || path.ends_with("::VecDeque")
                                || path.ends_with(".VecDeque")
                    );
                if is_deque_new {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.deque_new, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                let is_bag_new = method.name == "new"
                    && args.is_empty()
                    && matches!(
                        owner,
                        TStaticOwner::Prelude { path, .. }
                            if path == "std::collections::HashMap"
                                || path.ends_with("::HashMap")
                                || path.ends_with(".HashMap")
                    )
                    && matches!(&expr.ty, Type::Apply { name, .. } if name == "Bag");
                if is_bag_new {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.bag_new, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                let prelude_path = match owner {
                    TStaticOwner::Prelude { path, .. } => Some(path.as_str()),
                    _ => None,
                };
                if method.name == "new" && args.is_empty() {
                    let host_id = match prelude_path {
                        Some(path) if path.ends_with("BTreeSet") => {
                            Some(self.host.coll.sorted_set_new)
                        }
                        Some(path) if path.ends_with("BinaryHeap") => {
                            Some(self.host.coll.priority_queue_new)
                        }
                        Some(path) if path.ends_with("JetBitSet") => {
                            Some(self.host.coll.bit_set_new)
                        }
                        Some(path) if path.ends_with("JetByteBuffer") => {
                            Some(self.host.coll.byte_buffer_new)
                        }
                        _ => None,
                    };
                    if let Some(host_id) = host_id {
                        let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                        let call = self.b.ins().call(host_ref, &[]);
                        return Ok(self.b.inst_results(call)[0]);
                    }
                }
                if method.name == "new"
                    && args.len() == 1
                    && prelude_path.is_some_and(|path| path.ends_with("JetLru"))
                {
                    let capacity = self.lower_call_arg(&args[0])?;
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.lru_new, self.b.func);
                    let call = self.b.ins().call(host_ref, &[capacity]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                let is_pool_new = method.name == "new"
                    && args.is_empty()
                    && matches!(
                        owner,
                        TStaticOwner::Prelude { path, .. }
                            if path.ends_with("JetPool") || path.ends_with("::JetPool")
                    );
                if is_pool_new {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.memory.pool_new, self.b.func);
                    let call = self.b.ins().call(host_ref, &[]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                let is_shared_new = method.name == "new"
                    && args.len() == 1
                    && matches!(
                        owner,
                        TStaticOwner::Prelude { path, .. }
                            if path.ends_with("JetShared") || path.ends_with("::JetShared")
                    );
                if is_shared_new {
                    let value = self.lower_call_arg(&args[0])?;
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.memory.shared_new, self.b.func);
                    let call = self.b.ins().call(host_ref, &[value]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                // D-ENCSTREAM-SURFACE1: `EncodingLimits.safe()` — fixed defaults, no host.
                let is_encoding_limits_safe = method.name == "safe"
                    && args.is_empty()
                    && match owner {
                        TStaticOwner::User(name) => name == "EncodingLimits",
                        TStaticOwner::Prelude { path, .. } => {
                            path == "EncodingLimits"
                                || path.ends_with("::EncodingLimits")
                                || path.ends_with(".EncodingLimits")
                        }
                    }
                    && owner_type
                        .as_ref()
                        .map(|t| matches!(t, Type::Named(n) if n == "EncodingLimits"))
                        .unwrap_or(true);
                if is_encoding_limits_safe {
                    // Field order matches jet_std::EncodingLimits / sema core_types.
                    let n = self.b.ins().iconst(types::I64, 6);
                    let new_ref = self
                        .module
                        .declare_func_in_func(self.host.struct_new, self.b.func);
                    let new_call = self.b.ins().call(new_ref, &[n]);
                    let handle = self.b.inst_results(new_call)[0];
                    let set_i = self
                        .module
                        .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                    let fields = [
                        65536i64, // buffer_bytes
                        256,      // max_depth
                        16777216, // max_item_bytes
                        0,        // max_total_bytes = None
                        32,       // max_expansion_depth
                        8388608,  // max_expansion_bytes
                    ];
                    for (i, v) in fields.into_iter().enumerate() {
                        let idx = self.b.ins().iconst(types::I64, i as i64);
                        let val = self.b.ins().iconst(types::I64, v);
                        self.b.ins().call(set_i, &[handle, idx, val]);
                    }
                    return Ok(handle);
                }
                // D-EMAIL1: `email.Limits.safe()` — fixed defaults, no host.
                let is_email_limits_safe = method.name == "safe"
                    && args.is_empty()
                    && match owner {
                        TStaticOwner::User(name) => name == "Limits",
                        TStaticOwner::Prelude { path, .. } => {
                            path == "Limits"
                                || path.ends_with("::Limits")
                                || path.ends_with(".Limits")
                                || path.contains("jet_email::Limits")
                        }
                    }
                    && owner_type
                        .as_ref()
                        .map(|t| matches!(t, Type::Named(n) if n == "Limits"))
                        .unwrap_or(true);
                if is_email_limits_safe {
                    let n = self.b.ins().iconst(types::I64, 6);
                    let new_ref = self
                        .module
                        .declare_func_in_func(self.host.struct_new, self.b.func);
                    let new_call = self.b.ins().call(new_ref, &[n]);
                    let handle = self.b.inst_results(new_call)[0];
                    let set_i = self
                        .module
                        .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                    let fields = [
                        512i64,      // max_reply_line_bytes
                        100,         // max_reply_lines
                        100,         // max_capabilities
                        100,         // max_recipients
                        33_554_432,  // max_message_bytes
                        4096,        // max_auth_challenge_bytes
                    ];
                    for (i, v) in fields.into_iter().enumerate() {
                        let idx = self.b.ins().iconst(types::I64, i as i64);
                        let val = self.b.ins().iconst(types::I64, v);
                        self.b.ins().call(set_i, &[handle, idx, val]);
                    }
                    return Ok(handle);
                }
                // D-DATAFLOW1: `DataLimits.safe()` — nested EncodingLimits + max_* defaults.
                let is_data_limits_safe = method.name == "safe"
                    && args.is_empty()
                    && match owner {
                        TStaticOwner::User(name) => name == "DataLimits",
                        TStaticOwner::Prelude { path, .. } => {
                            path == "DataLimits"
                                || path.ends_with("::DataLimits")
                                || path.ends_with(".DataLimits")
                        }
                    }
                    && owner_type
                        .as_ref()
                        .map(|t| matches!(t, Type::Named(n) if n == "DataLimits"))
                        .unwrap_or(true);
                if is_data_limits_safe {
                    let n = self.b.ins().iconst(types::I64, 5);
                    let new_ref = self
                        .module
                        .declare_func_in_func(self.host.struct_new, self.b.func);
                    let new_call = self.b.ins().call(new_ref, &[n]);
                    let handle = self.b.inst_results(new_call)[0];
                    // encoding: EncodingLimits.safe() as nested struct handle
                    let enc_n = self.b.ins().iconst(types::I64, 6);
                    let enc_new = self.b.ins().call(new_ref, &[enc_n]);
                    let enc = self.b.inst_results(enc_new)[0];
                    let set_i = self
                        .module
                        .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                    let enc_fields = [
                        65536i64, 256, 16777216, 0, 32, 8388608,
                    ];
                    for (i, v) in enc_fields.into_iter().enumerate() {
                        let idx = self.b.ins().iconst(types::I64, i as i64);
                        let val = self.b.ins().iconst(types::I64, v);
                        self.b.ins().call(set_i, &[enc, idx, val]);
                    }
                    let zero = self.b.ins().iconst(types::I64, 0);
                    self.b.ins().call(set_i, &[handle, zero, enc]);
                    let data_fields = [100_000i64, 1_000_000, 1_000_000, 1_000_000];
                    for (i, v) in data_fields.into_iter().enumerate() {
                        let idx = self.b.ins().iconst(types::I64, (i + 1) as i64);
                        let val = self.b.ins().iconst(types::I64, v);
                        self.b.ins().call(set_i, &[handle, idx, val]);
                    }
                    return Ok(handle);
                }
                let key = Self::static_method_key(owner, owner_type.as_ref(), method)
                    .ok_or_else(|| format!("jit static `{}::{}`", match owner {
                        TStaticOwner::User(name) => name.as_str(),
                        TStaticOwner::Prelude { path, .. } => path.as_str(),
                    }, method.name))?;
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
            TExprKind::EnumLit {
                enum_type,
                variant,
                payload,
            } => {
                let is_datatree =
                    matches!(enum_type.as_str(), "DataTree" | "Json" | "Toml" | "Yaml" | "Csv");
                match payload {
                TEnumPayload::Unit => {
                    let disc = self
                        .meta
                        .enum_variant_index(enum_type, variant)
                        .or_else(|| match (enum_type.as_str(), variant.as_str()) {
                            // Core enum — not always in program.enum_variants.
                            ("ProcessStreamMode", "Stream") => Some(0),
                            ("ProcessStreamMode", "Inherit") => Some(1),
                            ("ProcessStreamMode", "Capture") => Some(2),
                            ("TextWidthAmbiguous", "Narrow") => Some(0),
                            ("TextWidthAmbiguous", "Wide") => Some(1),
                            ("TextWidthControls", "Zero") => Some(0),
                            ("TextWidthControls", "Reject") => Some(1),
                            ("SmtpSecurity", "StartTls") => Some(0),
                            ("SmtpSecurity", "Tls") => Some(1),
                            ("RecipientPolicy", "RequireAll") => Some(0),
                            ("RecipientPolicy", "DeliverAccepted") => Some(1),
                            ("TlsTrust", "System") => Some(0),
                            _ => None,
                        })
                        .ok_or_else(|| format!("jit enum lit `{enum_type}::{variant}`"))?;
                    if is_datatree {
                        self.pack_datatree_enum(disc, None)
                    } else {
                        Ok(self.b.ins().iconst(types::I64, disc))
                    }
                }
                TEnumPayload::Positional(values) if values.len() == 1 => {
                    let disc = self
                        .meta
                        .enum_variant_index(enum_type, variant)
                        .ok_or_else(|| format!("jit enum lit `{enum_type}::{variant}`"))?;
                    let payload_ty = values[0].value.ty.clone();
                    let payload = self.lower_expr(&values[0].value)?;
                    if is_datatree {
                        self.pack_datatree_enum(disc, Some((payload, &payload_ty)))
                    } else {
                        self.pack_enum_scalar(disc, payload, &payload_ty)
                    }
                }
                TEnumPayload::Positional(_) => Err("jit enum positional payload unsupported".to_string()),
                TEnumPayload::Named(fields) => {
                    let disc = self
                        .meta
                        .enum_variant_index(enum_type, variant)
                        .or_else(|| match (enum_type.as_str(), variant.as_str()) {
                            ("SmtpAuth", "Password") => Some(1),
                            ("SmtpAuth", "None") => Some(0),
                            ("TlsTrust", "SystemPlusCa") => Some(1),
                            _ => None,
                        })
                        .ok_or_else(|| format!("jit enum lit `{enum_type}::{variant}`"))?;
                    // Heap carrier: [disc, field0, field1, …] in source field order.
                    let n = self.b.ins().iconst(types::I64, (fields.len() + 1) as i64);
                    let new_ref = self
                        .module
                        .declare_func_in_func(self.host.struct_new, self.b.func);
                    let call = self.b.ins().call(new_ref, &[n]);
                    let handle = self.b.inst_results(call)[0];
                    let set_i = self
                        .module
                        .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                    let zero = self.b.ins().iconst(types::I64, 0);
                    let disc_v = self.b.ins().iconst(types::I64, disc);
                    self.b.ins().call(set_i, &[handle, zero, disc_v]);
                    for (i, (_name, arg)) in fields.iter().enumerate() {
                        let idx = self.b.ins().iconst(types::I64, (i + 1) as i64);
                        let payload = self.lower_expr(&arg.value)?;
                        let bits = match self.meta.clif_ty(&arg.value.ty).or_else(|| clif_ty(&arg.value.ty)) {
                            Some(ty) if ty == types::I64 => payload,
                            Some(ty) if ty == types::I8 => self.b.ins().uextend(types::I64, payload),
                            Some(ty) if ty == types::I32 => self.b.ins().sextend(types::I64, payload),
                            _ => {
                                return Err(format!(
                                    "jit enum named payload field unsupported: {:?}",
                                    arg.value.ty
                                ))
                            }
                        };
                        self.b.ins().call(set_i, &[handle, idx, bits]);
                    }
                    Ok(handle)
                }
            }
            }
            TExprKind::Present(inner) => {
                let v = self.lower_expr(inner)?;
                self.pack_option_payload(v, &inner.ty)
            }
            TExprKind::Absent => Ok(self.b.ins().iconst(types::I64, 0)),
            TExprKind::Unit => Ok(self.b.ins().iconst(types::I64, 0)),
            TExprKind::CtLit(value) => self.lower_ct_value(value),
            TExprKind::Uninit => {
                // D-UNINIT1 / GC promote: placeholder overwritten before read.
                match self.meta.clif_ty(&expr.ty).or_else(|| clif_ty(&expr.ty)) {
                    Some(ty) if ty == types::F64 => Ok(self.b.ins().f64const(0.0)),
                    Some(ty) => Ok(self.b.ins().iconst(ty, 0)),
                    None => Err(format!("jit uninit type unsupported: {:?}", expr.ty)),
                }
            }
            TExprKind::HostCall(host) => self.lower_host_call(host.as_ref(), &expr.ty),
            TExprKind::DefaultLit => Err("jit default literal unsupported".to_string()),
            TExprKind::ConstRef(name) => {
                let value = self
                    .meta
                    .constant(name)
                    .cloned()
                    .or_else(|| {
                        self.meta
                            .int_constant(name)
                            .map(jet_foundation::AST::CtValue::Int)
                    })
                    .ok_or_else(|| format!("jit const ref unknown: {name}"))?;
                self.lower_ct_value(&value)
            }
            TExprKind::DataEntriesToMap(local) => {
                let payload = self.load_local(local)?;
                let validate = self
                    .module
                    .declare_func_in_func(self.host.coll.map_validate, self.b.func);
                let call = self.b.ins().call(validate, &[payload]);
                let map = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(map)
            }
            TExprKind::PoolSlot {
                pool,
                id,
                field,
                ..
            } => {
                let elem_ty = match &pool.ty {
                    Type::Apply { args, .. } if !args.is_empty() => Some(args[0].clone()),
                    _ => None,
                };
                let pool = self.lower_expr(pool)?;
                let id = self.lower_expr(id)?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.memory.pool_get, self.b.func);
                let call = self.b.ins().call(host_ref, &[pool, id]);
                let value = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                if let Some(field) = field {
                    let elem_ty =
                        elem_ty.ok_or_else(|| "jit Pool field element type".to_string())?;
                    let type_name =
                        record_type_key(&elem_ty).ok_or("jit Pool field record type")?;
                    let field_ty = self
                        .meta
                        .struct_field_ty(&type_name, field)
                        .unwrap_or_else(|| expr.ty.clone());
                    self.lower_record_field(value, &type_name, field, &field_ty)
                } else {
                    Ok(value)
                }
            }
            TExprKind::RangeCheckedCtor { .. } => {
                Err("jit range-checked ctor unsupported".to_string())
            }
            TExprKind::DistinctCtor { arg, .. } => self.lower_expr(arg),
            TExprKind::MathBuiltin { .. } => Err("jit math builtin unsupported".to_string()),
            // D-BIGINT1 / D-DECIMAL1: precise numeric ctor/binop.
            TExprKind::PreciseBuiltin {
                type_name,
                func,
                args,
            } if type_name == "BigInt" || type_name == "Decimal" => {
                let host_fn = match (type_name.as_str(), func.as_str()) {
                    ("BigInt", "from_int") => self.host.num.bigint_from_int,
                    ("BigInt", "from_str") => self.host.num.bigint_from_str,
                    ("BigInt", "add") => self.host.num.bigint_add,
                    ("BigInt", "sub") => self.host.num.bigint_sub,
                    ("BigInt", "mul") => self.host.num.bigint_mul,
                    ("BigInt", "to_string") => self.host.num.bigint_to_string,
                    ("Decimal", "from_str") => self.host.num.decimal_from_str,
                    ("Decimal", "add") => self.host.num.decimal_add,
                    ("Decimal", "sub") => self.host.num.decimal_sub,
                    ("Decimal", "mul") => self.host.num.decimal_mul,
                    ("Decimal", "to_string") => self.host.num.decimal_to_string,
                    _ => return Err(format!("jit precise numeric builtin unsupported: {type_name}.{func}")),
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
            TExprKind::Drop(inner) => {
                self.lower_expr(inner)?;
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TExprKind::AmbientInput { prompt } => {
                let (has_prompt, prompt_v) = match prompt {
                    None => (
                        self.b.ins().iconst(types::I8, 0),
                        self.b.ins().iconst(types::I64, 0),
                    ),
                    Some(p) => (self.b.ins().iconst(types::I8, 1), self.lower_expr(p)?),
                };
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.io_input, self.b.func);
                let call = self.b.ins().call(host_ref, &[has_prompt, prompt_v]);
                Ok(self.b.inst_results(call)[0])
            }
            TExprKind::RequireStop {
                kind,
                loc,
                always_stops,
            } => {
                let fail_block = self.b.create_block();
                let cont = self.b.create_block();
                let mut eq_msg: Option<(Value, Value)> = None;
                if *always_stops {
                    self.b.ins().jump(fail_block, &[]);
                } else {
                    match kind {
                        TIR::TRequireKind::Require { cond, .. } => {
                            let ok = self.lower_expr(cond)?;
                            let zero = self.b.ins().iconst(types::I8, 0);
                            let is_true = self.b.ins().icmp(IntCC::NotEqual, ok, zero);
                            self.b.ins().brif(is_true, cont, &[], fail_block, &[]);
                        }
                        TIR::TRequireKind::RequireEq { left, right } => {
                            let l = self.lower_expr(left)?;
                            let r = self.lower_expr(right)?;
                            eq_msg = Some((l, r));
                            let eq = self.b.ins().icmp(IntCC::Equal, l, r);
                            self.b.ins().brif(eq, cont, &[], fail_block, &[]);
                        }
                        TIR::TRequireKind::Panic { .. } => {
                            self.b.ins().jump(fail_block, &[]);
                        }
                    }
                }
                self.b.switch_to_block(fail_block);
                self.b.seal_block(fail_block);
                let msg_val = match kind {
                    TIR::TRequireKind::Require { msg: Some(msg), .. }
                    | TIR::TRequireKind::Panic { msg } => self.lower_expr(msg)?,
                    TIR::TRequireKind::Require { msg: None, .. } => {
                        let h = self.runtime.heap.alloc_string("condition failed");
                        self.b.ins().iconst(types::I64, h)
                    }
                    TIR::TRequireKind::RequireEq { .. } => {
                        let _ = eq_msg;
                        let h = self
                            .runtime
                            .heap
                            .alloc_string("values are not equal");
                        self.b.ins().iconst(types::I64, h)
                    }
                };
                let begin = self
                    .module
                    .declare_func_in_func(self.host.str_begin, self.b.func);
                let call = self.b.ins().call(begin, &[]);
                let loc_buf = self.b.inst_results(call)[0];
                let mut first = true;
                for (name, place) in &loc.locals {
                    let key = Self::local_key(place);
                    let Some(var) = self.vars.get(&key).copied() else {
                        continue;
                    };
                    let ty = self.var_tys.get(&key).cloned().unwrap_or(Type::Int);
                    if !matches!(
                        ty,
                        Type::Int
                            | Type::IntN { .. }
                            | Type::Bool
                            | Type::Float
                            | Type::Float32
                    ) {
                        continue;
                    }
                    if !first {
                        let sep = self.runtime.heap.alloc_string(", ");
                        let sep_v = self.b.ins().iconst(types::I64, sep);
                        let push_s = self
                            .module
                            .declare_func_in_func(self.host.str_push_str, self.b.func);
                        self.b.ins().call(push_s, &[loc_buf, sep_v]);
                    }
                    first = false;
                    let prefix = self.runtime.heap.alloc_string(format!("{name} = "));
                    let prefix_v = self.b.ins().iconst(types::I64, prefix);
                    let push_s = self
                        .module
                        .declare_func_in_func(self.host.str_push_str, self.b.func);
                    self.b.ins().call(push_s, &[loc_buf, prefix_v]);
                    let val = self.b.use_var(var);
                    match ty {
                        Type::Float | Type::Float32 => {
                            let push = self
                                .module
                                .declare_func_in_func(self.host.str_push_f64, self.b.func);
                            self.b.ins().call(push, &[loc_buf, val]);
                        }
                        Type::Bool => {
                            let push = self
                                .module
                                .declare_func_in_func(self.host.str_push_bool, self.b.func);
                            self.b.ins().call(push, &[loc_buf, val]);
                        }
                        _ => {
                            let push = self
                                .module
                                .declare_func_in_func(self.host.str_push_i64, self.b.func);
                            self.b.ins().call(push, &[loc_buf, val]);
                        }
                    }
                }
                let locals_val = loc_buf;
                let file_h = self
                    .runtime
                    .heap
                    .alloc_string(Self::strip_rust_str_lit(&loc.file));
                let fn_h = self
                    .runtime
                    .heap
                    .alloc_string(Self::strip_rust_str_lit(&loc.fn_name));
                let src_h = self
                    .runtime
                    .heap
                    .alloc_string(Self::strip_rust_str_lit(&loc.src_line));
                let host = self
                    .module
                    .declare_func_in_func(self.host.rich_panic, self.b.func);
                let file_v = self.b.ins().iconst(types::I64, file_h);
                let line_v = self.b.ins().iconst(types::I64, i64::from(loc.line));
                let fn_v = self.b.ins().iconst(types::I64, fn_h);
                let src_v = self.b.ins().iconst(types::I64, src_h);
                let col_v = self.b.ins().iconst(types::I64, i64::from(loc.col));
                let caret_v = self.b.ins().iconst(types::I64, i64::from(loc.caret));
                self.b.ins().call(
                    host,
                    &[file_v, line_v, fn_v, src_v, col_v, caret_v, msg_val, locals_val],
                );
                self.emit_trap_check()?;
                self.b.ins().jump(cont, &[]);
                self.b.switch_to_block(cont);
                self.b.seal_block(cont);
                Ok(self.b.ins().iconst(types::I8, 0))
            }

            TExprKind::LayoutCompare { .. } => Err("jit layout compare unsupported".to_string()),
            TExprKind::LayoutLit { .. } => Err("jit layout literal unsupported".to_string()),
            TExprKind::PtrFromAddr { .. } => Err("jit pointer from addr unsupported".to_string()),
            TExprKind::RawOf(inner) => {
                if self.unsafe_depth == 0 {
                    return Err("jit raw pointer creation outside #Unsafe".to_string());
                }
                if matches!(
                    &inner.ty,
                    Type::Apply { name, args }
                        if name == jet_foundation::Syntax::TYPE_PTR && args.len() == 1
                ) {
                    return self.lower_expr(inner);
                }
                let local = Self::raw_place_local(inner);
                if let Some(local) = local {
                    let key = Self::local_key(local);
                    let var = self
                        .vars
                        .get(&key)
                        .copied()
                        .ok_or_else(|| format!("jit RawOf unknown local `{}`", local.name))?;
                    let clif = self.meta.clif_ty(&inner.ty).ok_or_else(|| {
                        format!("jit raw pointer payload unsupported: {:?}", inner.ty)
                    })?;
                    let slot = if let Some(slot) = self.raw_slots.get(&key).copied() {
                        slot
                    } else {
                        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                            StackSlotKind::ExplicitSlot,
                            u32::from(clif.bytes()),
                            0,
                        ));
                        let value = self.b.use_var(var);
                        self.b.ins().stack_store(value, slot, 0);
                        self.raw_slots.insert(key, slot);
                        slot
                    };
                    return Ok(self.b.ins().stack_addr(
                        self.module.target_config().pointer_type(),
                        slot,
                        0,
                    ));
                }
                let value = self.lower_expr(inner)?;
                let clif = self
                    .meta
                    .clif_ty(&inner.ty)
                    .ok_or_else(|| format!("jit raw pointer payload unsupported: {:?}", inner.ty))?;
                let size = u32::from(clif.bytes());
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    size,
                    0,
                ));
                self.b.ins().stack_store(value, slot, 0);
                Ok(self
                    .b
                    .ins()
                    .stack_addr(self.module.target_config().pointer_type(), slot, 0))
            }
            TExprKind::Deref(inner) => {
                if self.unsafe_depth == 0 {
                    return Err("jit raw pointer dereference outside #Unsafe".to_string());
                }
                let pointer = self.lower_expr(inner)?;
                let clif = self
                    .meta
                    .clif_ty(&expr.ty)
                    .ok_or_else(|| format!("jit raw pointer result unsupported: {:?}", expr.ty))?;
                Ok(self.b.ins().load(clif, MemFlags::trusted(), pointer, 0))
            }
            TExprKind::AllocNew { .. } => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.memory.allocator_new, self.b.func);
                let call = self.b.ins().call(host_ref, &[]);
                Ok(self.b.inst_results(call)[0])
            }
            TExprKind::JsonLit { variant, arg } => {
                let disc = self
                    .meta
                    .enum_variant_index("DataTree", variant)
                    .ok_or_else(|| format!("jit JSON lit `DataTree::{variant}`"))?;
                match arg.as_ref() {
                    None => self.pack_datatree_enum(disc, None),
                    Some(boxed) => {
                        let (expr, _) = boxed.as_ref();
                        let payload_ty = expr.ty.clone();
                        let payload = self.lower_expr(expr)?;
                        self.pack_datatree_enum(disc, Some((payload, &payload_ty)))
                    }
                }
            }
            TExprKind::DbValueLit { variant, arg } => {
                let disc = match variant.as_str() {
                    "Null" => 0i64,
                    "Int" => 1,
                    "Float" => 2,
                    "Text" => 3,
                    "Bool" => 4,
                    _ => {
                        return Err(format!("jit DbValue lit `DbValue::{variant}`"))
                    }
                };
                match arg.as_ref() {
                    None => {
                        let disc_v = self.b.ins().iconst(types::I64, disc);
                        let zero = self.b.ins().iconst(types::I64, 0);
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.db.dbvalue_pack, self.b.func);
                        let call = self.b.ins().call(host_ref, &[disc_v, zero]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    Some(boxed) => {
                        let (expr, _) = boxed.as_ref();
                        let payload_ty = expr.ty.clone();
                        let payload = self.lower_expr(expr)?;
                        let payload_bits = match self.meta.clif_ty(&payload_ty) {
                            Some(types::F64) => self.b.ins().bitcast(
                                types::I64,
                                Self::scalar_bitcast_memflags(),
                                payload,
                            ),
                            Some(types::I8) => self.b.ins().uextend(types::I64, payload),
                            Some(types::I32) => self.b.ins().sextend(types::I64, payload),
                            Some(types::I64) => payload,
                            other => {
                                return Err(format!(
                                    "jit DbValue payload type unsupported: {payload_ty:?} ({other:?})"
                                ))
                            }
                        };
                        let disc_v = self.b.ins().iconst(types::I64, disc);
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.db.dbvalue_pack, self.b.func);
                        let call = self.b.ins().call(host_ref, &[disc_v, payload_bits]);
                        Ok(self.b.inst_results(call)[0])
                    }
                }
            }
            TExprKind::ListSpread { parts } => self.lower_list_spread(&expr.ty, parts),
            TExprKind::ColumnarListLit { .. } => {
                Err("jit columnar list literal unsupported".to_string())
            }
            TExprKind::ColumnarGather { .. } => Err("jit columnar gather unsupported".to_string()),
            TExprKind::ColumnarColumnRead { .. } => {
                Err("jit columnar column read unsupported".to_string())
            }
            TExprKind::IndexHook {
                type_name,
                base,
                index,
                ..
            } => {
                let key = format!("{type_name}::get");
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing method `{key}`"))?;
                let recv = self.lower_expr(base)?;
                let index = self.lower_expr(index)?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &[recv, index]);
                let packed = self.b.inst_results(call)[0];
                let zero = self.b.ins().iconst(types::I64, 0);
                let missing = self.b.ins().icmp(IntCC::Equal, packed, zero);
                let ok = self.b.create_block();
                let fail = self.b.create_block();
                let merge = self.b.create_block();
                self.b.append_block_param(merge, types::I64);
                self.b.ins().brif(missing, fail, &[], ok, &[]);
                self.b.switch_to_block(fail);
                self.b.seal_block(fail);
                let message = self.b.ins().iconst(types::I64, 0);
                let trap = self
                    .module
                    .declare_func_in_func(self.host.trap_panic, self.b.func);
                self.b.ins().call(trap, &[message]);
                self.b.ins().jump(merge, &[zero]);
                self.b.switch_to_block(ok);
                self.b.seal_block(ok);
                let one = self.b.ins().iconst(types::I64, 1);
                let value = self.b.ins().isub(packed, one);
                self.b.ins().jump(merge, &[value]);
                self.b.switch_to_block(merge);
                self.b.seal_block(merge);
                self.emit_trap_check()?;
                Ok(self.b.block_params(merge)[0])
            }
            TExprKind::MathLaneIndex { .. } => Err("jit math lane index unsupported".to_string()),
            TExprKind::MathSwizzleRead { .. } => {
                Err("jit math swizzle read unsupported".to_string())
            }
            TExprKind::MaterializeView(inner) => self.lower_expr(inner),
            TExprKind::FnFieldCall { recv, field, args } => {
                let handle = self.lower_expr(recv)?;
                let type_name = record_type_key(&recv.ty).ok_or("jit fn-field receiver type")?;
                let fn_ty = self
                    .meta
                    .struct_field_ty(&type_name, field)
                    .ok_or_else(|| format!("jit fn-field `{field}` on `{type_name}`"))?;
                let callee = self.lower_record_field(handle, &type_name, field, &fn_ty)?;
                self.lower_fn_call(callee, &fn_ty, args)
            }
            TExprKind::Todo { line, expected_type } => {
                let msg = format!("#Todo at ?:{line} — expected {expected_type}");
                let msg_h = self.runtime.heap.alloc_string(msg);
                let msg_v = self.b.ins().iconst(types::I64, msg_h);
                let empty = self.runtime.heap.alloc_string(String::new());
                let empty_v = self.b.ins().iconst(types::I64, empty);
                let host = self
                    .module
                    .declare_func_in_func(self.host.rich_panic, self.b.func);
                let line_v = self.b.ins().iconst(types::I64, *line as i64);
                let one = self.b.ins().iconst(types::I64, 1);
                let caret = self.b.ins().iconst(types::I64, 5);
                self.b.ins().call(
                    host,
                    &[empty_v, line_v, empty_v, empty_v, one, caret, msg_v, empty_v],
                );
                self.emit_trap_check()?;
                Ok(self.b.ins().iconst(types::I64, 0))
            }
            TExprKind::DistinctRaw(inner) => self.lower_expr(inner),
            TExprKind::Ok(inner) => self.result_new(true, inner),
            TExprKind::Err(inner) => self.result_new(false, inner),
            TExprKind::Try {
                inner,
                convert,
                file,
                line,
                fn_name,
            } => self.lower_try(inner, convert, file, *line, fn_name),
            TExprKind::OptField { .. } => Err("jit optional field chain unsupported".to_string()),
            TExprKind::Lambda(lam) => {
                let id = super::functions_compile::lower_callable_lambda(
                    self.module,
                    self.host,
                    self.meta,
                    lam,
                    self.func_ids,
                    self.spawn_func_ids,
                    self.spawn_lambdas,
                    self.spawn_site,
                    self.runtime,
                )?;
                let func_ref = self.module.declare_func_in_func(id, self.b.func);
                let fn_addr = self.b.ins().func_addr(types::I64, func_ref);
                if lam.captures.is_empty() {
                    return Ok(fn_addr);
                }
                if lam.arc
                    || matches!(
                        &expr.ty,
                        Type::Named(name) if name == "HttpHandler"
                    )
                {
                    // Prefer host-side packing for the common single-capture
                    // middleware shape (`owned :: ~next`); JIT list_push was
                    // arriving empty at bind time on the serve thread.
                    if lam.captures.len() == 1 {
                        let (outer, _place, _ty) = &lam.captures[0];
                        let key = TIR::local_place(outer);
                        let var = self
                            .vars
                            .get(&key)
                            .copied()
                            .ok_or_else(|| format!("jit lambda capture unknown `{outer}`"))?;
                        let cap0 = self.b.use_var(var);
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_handler_bind1,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[fn_addr, cap0]);
                        Ok(self.b.inst_results(call)[0])
                    } else {
                        let empty = self
                            .module
                            .declare_func_in_func(self.host.coll.list_new, self.b.func);
                        let call = self.b.ins().call(empty, &[]);
                        let env = self.b.inst_results(call)[0];
                        let push = self
                            .module
                            .declare_func_in_func(self.host.coll.list_push, self.b.func);
                        for (outer, _place, _ty) in &lam.captures {
                            let key = TIR::local_place(outer);
                            let var = self
                                .vars
                                .get(&key)
                                .copied()
                                .ok_or_else(|| format!("jit lambda capture unknown `{outer}`"))?;
                            let val = self.b.use_var(var);
                            self.b.ins().call(push, &[env, val]);
                        }
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_handler_bind,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[fn_addr, env]);
                        Ok(self.b.inst_results(call)[0])
                    }
                } else {
                    Err("jit callable captures unsupported".to_string())
                }
            }
            TExprKind::HostBorrowCallback { .. } => {
                Err("jit borrowed callback adapter unsupported".to_string())
            }
            TExprKind::PatternMatches { subj, pattern } => {
                let value = self.lower_expr(subj)?;
                let enum_name = pattern
                    .enum_type
                    .as_deref()
                    .or_else(|| user_type_name(&subj.ty));
                self.lower_pattern_condition(value, &pattern.pattern, enum_name, false)
            }
            TExprKind::OptionLift2 { f, a, b } => {
                self.lower_option_lift2(f, a, b, &expr.ty)
            }
            TExprKind::ClosureMethod { recv, op, args } => {
                self.lower_closure_method(recv, op, args)
            }
            TExprKind::NumericMethod { recv, op } => self.lower_numeric_method(recv, op),
            TExprKind::DistinctConvert {
                arg,
                op,
                range,
                fallible,
                ..
            } => {
                let converted = self.lower_numeric_method(arg, op)?;
                let Some((lo, hi)) = range else {
                    return Ok(converted);
                };
                if !*fallible {
                    return Ok(converted);
                }
                let lo = self.b.ins().iconst(types::I64, *lo);
                let hi = self.b.ins().iconst(types::I64, *hi);
                let fallible = matches!(
                    op,
                    TNumericOp::TryFrom { .. }
                        | TNumericOp::FloatToInt { .. }
                        | TNumericOp::FloatNarrow { .. }
                );
                let host = if fallible {
                    self.host.distinct_range_result
                } else {
                    self.host.distinct_range
                };
                let host = self.module.declare_func_in_func(host, self.b.func);
                let call = self.b.ins().call(host, &[converted, lo, hi]);
                Ok(self.b.inst_results(call)[0])
            }
            TExprKind::UnitConvert {
                arg,
                scale,
                offset,
                rounding,
                fallible,
                ..
            } => {
                let value = self.lower_expr(arg)?;
                let ratios = [
                    scale.num.to_string(),
                    scale.den.to_string(),
                    offset.num.to_string(),
                    offset.den.to_string(),
                ]
                .map(|ratio| self.runtime.heap.alloc_string(ratio))
                .map(|id| self.b.ins().iconst(types::I64, id));
                let mut call_args = vec![value, ratios[0], ratios[1], ratios[2], ratios[3]];
                let host = if let Some((mode, digits)) = rounding {
                    call_args.push(self.b.ins().iconst(types::I64, *mode as i64));
                    call_args.push(self.lower_expr(digits)?);
                    self.host.unit_convert_rounded
                } else if *fallible {
                    self.host.unit_convert_exact
                } else {
                    self.host.unit_convert_implicit
                };
                let host = self.module.declare_func_in_func(host, self.b.func);
                let call = self.b.ins().call(host, &call_args);
                if !*fallible {
                    self.emit_trap_check()?;
                }
                Ok(self.b.inst_results(call)[0])
            }
            TExprKind::OverflowOpt {
                prefix,
                op,
                lhs,
                rhs,
            } => {
                let (signed, bits) = match &lhs.ty {
                    Type::IntN { signed, bits } => (*signed, *bits),
                    Type::Int => (true, 64),
                    _ => {
                        return Err(
                            "jit overflow opt-out needs fixed-width integers".to_string()
                        )
                    }
                };
                let op = match *op {
                    "add" => BinOp::Add,
                    "sub" => BinOp::Sub,
                    "mul" => BinOp::Mul,
                    "div" => BinOp::Div,
                    _ => return Err("jit overflow opt-out operator unsupported".to_string()),
                };
                let mode = match prefix.as_str() {
                    "wrapping" => INTN_MODE_WRAPPING,
                    "saturating" => INTN_MODE_SATURATING,
                    "checked" => INTN_MODE_CHECKED,
                    _ => return Err("jit overflow opt-out mode unsupported".to_string()),
                };
                let left = self.lower_expr(lhs)?;
                let right = self.lower_expr(rhs)?;
                let right_signed =
                    !matches!(&rhs.ty, Type::IntN { signed: false, .. });
                self.lower_intn_values(
                    op,
                    mode,
                    left,
                    right,
                    signed,
                    bits,
                    right_signed,
                )
            }
            TExprKind::FnValue { kind } => match kind {
                TFnValueKind::NamedFn {
                    name: Some(name), ..
                } => {
                    let id = self
                        .func_ids
                        .get(name)
                        .copied()
                        .ok_or_else(|| format!("jit fn value unknown function `{name}`"))?;
                    let func_ref = self.module.declare_func_in_func(id, self.b.func);
                    Ok(self.b.ins().func_addr(types::I64, func_ref))
                }
                TFnValueKind::NamedFn { name: None, .. } => {
                    Err("jit rendered fn coercion unsupported".to_string())
                }
                TFnValueKind::Call { callee, args } => {
                    let fn_ty = callee.ty.clone();
                    let value = self.lower_expr(callee)?;
                    self.lower_fn_call(value, &fn_ty, args)
                }
            },
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
            TExprKind::Close(inner) => {
                let handle = self.lower_expr(inner)?;
                let host = match &inner.ty {
                    Type::Named(n) if n == "FileWriter" => Some(self.host.io.file_writer_close),
                    Type::Named(n) if n == "FileReader" => Some(self.host.io.file_reader_close),
                    Type::Apply { name, args } if name == "Resource" && args.len() == 1 => {
                        match &args[0] {
                            Type::Named(n) if n == "FileWriter" => {
                                Some(self.host.io.file_writer_close)
                            }
                            Type::Named(n) if n == "FileReader" => {
                                Some(self.host.io.file_reader_close)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(fid) = host {
                    let host_ref = self.module.declare_func_in_func(fid, self.b.func);
                    self.b.ins().call(host_ref, &[handle]);
                }
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TExprKind::ResourceNew(inner) => self.lower_expr(inner),
            TExprKind::ResourceTake(name) => {
                let place = TIR::local_place(name);
                let var = self
                    .vars
                    .get(&place)
                    .copied()
                    .ok_or_else(|| format!("jit resource take unknown local `{name}`"))?;
                Ok(self.b.use_var(var))
            }
        }
    }

    /// `if result == { .Ok(_) -> …; .Err(_) -> … }` on Result handles.
    fn lower_option_enum_match(
        &mut self,
        scrutinee: &TExpr,
        arms: &[TIR::TMatchArm],
        else_body: Option<&[TStmt]>,
        fallthrough: bool,
    ) -> Result<(), String> {
        let Type::Option(inner) = &scrutinee.ty else {
            return Err("jit option enum match on non-Option".to_string());
        };
        let inner_ty = inner.as_ref().clone();
        let packed = self.lower_expr(scrutinee)?;
        let zero = self.b.ins().iconst(types::I64, 0);
        let is_some = self.b.ins().icmp(IntCC::NotEqual, packed, zero);
        let merge = self.b.create_block();
        let mut any_reaches_merge = false;
        let mut remaining = self.b.create_block();
        self.b.ins().jump(remaining, &[]);
        for arm in arms {
            self.b.switch_to_block(remaining);
            self.b.seal_block(remaining);
            let (want_some, binding) = match &arm.pattern.pattern {
                Pattern::Present { binding, .. } => (true, Some(binding.as_str())),
                Pattern::Absent(_) => (false, None),
                Pattern::Variant {
                    variant, bindings, ..
                } if variant == "Val" => (true, bindings.first().and_then(PatSlot::as_bind)),
                Pattern::Variant { variant, .. } if variant == "None" => (false, None),
                _ => return Err("jit option enum arm unsupported".to_string()),
            };
            let then_block = self.b.create_block();
            let next = self.b.create_block();
            let condition = if want_some {
                is_some
            } else {
                self.b.ins().icmp(IntCC::Equal, packed, zero)
            };
            self.b.ins().brif(condition, then_block, &[], next, &[]);
            self.b.switch_to_block(then_block);
            self.b.seal_block(then_block);
            let mut bound = None;
            if let Some(name) = binding.filter(|name| *name != "_") {
                let value = self.unpack_option_payload(packed, &inner_ty)?;
                let clif = self.meta.clif_ty(&inner_ty).unwrap_or(types::I64);
                let var = self.fresh_var(clif);
                self.b.def_var(var, value);
                let place = TIR::local_place(name);
                self.vars.insert(place.clone(), var);
                self.var_tys.insert(place.clone(), inner_ty.clone());
                bound = Some(place);
            }
            self.lower_stmts_scoped(&arm.body)?;
            if let Some(place) = bound {
                self.vars.remove(&place);
                self.var_tys.remove(&place);
            }
            if !self.dead {
                self.b.ins().jump(merge, &[]);
                any_reaches_merge = true;
            }
            remaining = next;
            self.dead = false;
        }
        self.b.switch_to_block(remaining);
        self.b.seal_block(remaining);
        if let Some(body) = else_body {
            self.lower_stmts_scoped(body)?;
            if !self.dead {
                self.b.ins().jump(merge, &[]);
                any_reaches_merge = true;
            }
        } else if fallthrough {
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
            self.dead = true;
        }
        Ok(())
    }

    /// `if result == { .Ok(_) -> …; .Err(_) -> … }` on Result handles.
    fn lower_result_enum_match(
        &mut self,
        scrutinee: &TExpr,
        arms: &[TIR::TMatchArm],
        else_body: Option<&[TStmt]>,
        fallthrough: bool,
    ) -> Result<(), String> {
        let Type::Result { ok, err } = &scrutinee.ty else {
            return Err("jit result enum match on non-Result".to_string());
        };
        let ok_ty = ok.as_ref().clone();
        let err_ty = err.as_ref().clone();
        let handle = self.lower_expr(scrutinee)?;
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[handle]);
        let is_ok = self.b.inst_results(status_call)[0];
        let zero_b = self.b.ins().iconst(types::I8, 0);
        let ok_cond = self.b.ins().icmp(IntCC::NotEqual, is_ok, zero_b);

        let merge = self.b.create_block();
        let mut any_reaches_merge = false;
        let mut remaining = self.b.create_block();
        self.b.ins().jump(remaining, &[]);

        for arm in arms {
            self.b.switch_to_block(remaining);
            self.b.seal_block(remaining);
            let (want_ok, binding, payload_ty) = match &arm.pattern.pattern {
                Pattern::Ok { binding, .. } => (true, binding.as_str(), ok_ty.clone()),
                Pattern::Err { binding, .. } => (false, binding.as_str(), err_ty.clone()),
                _ => return Err("jit result enum arm unsupported".to_string()),
            };
            let then_block = self.b.create_block();
            let next = self.b.create_block();
            let cond = if want_ok {
                ok_cond
            } else {
                self.b.ins().icmp(IntCC::Equal, is_ok, zero_b)
            };
            self.b.ins().brif(cond, then_block, &[], next, &[]);
            self.b.switch_to_block(then_block);
            self.b.seal_block(then_block);
            let mut bound_place = None;
            if binding != "_" {
                let payload = self.result_payload(handle, &payload_ty)?;
                let place = TIR::local_place(binding);
                let clif = clif_ty(&payload_ty).unwrap_or(types::I64);
                let var = self.fresh_var(clif);
                self.b.def_var(var, payload);
                self.vars.insert(place.clone(), var);
                self.var_tys.insert(place.clone(), payload_ty);
                bound_place = Some(place);
            }
            self.lower_stmts_scoped(&arm.body)?;
            if let Some(place) = bound_place {
                self.vars.remove(&place);
                self.var_tys.remove(&place);
            }
            if !self.dead {
                self.b.ins().jump(merge, &[]);
                any_reaches_merge = true;
            }
            remaining = next;
            self.dead = false;
        }

        self.b.switch_to_block(remaining);
        self.b.seal_block(remaining);
        if let Some(body) = else_body {
            self.lower_stmts_scoped(body)?;
            if !self.dead {
                self.b.ins().jump(merge, &[]);
                any_reaches_merge = true;
            }
        } else if fallthrough {
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
            self.dead = true;
        }
        Ok(())
    }

    fn lower_list_get_opt_status(&mut self, value: &TExpr) -> Result<Value, String> {
        if let TExprKind::BuiltinMethod {
            recv,
            op: TBuiltinOp::GetList,
            args,
        } = &value.kind
        {
            if matches!(
                &recv.ty,
                Type::List(inner) | Type::FixedList { elem: inner, .. }
                    if matches!(inner.as_ref(), Type::IntN { .. })
            ) {
                return Err("jit List<IntN>.get needs typed Option lowering".to_string());
            }
            let list = self.lower_expr(recv)?;
            let idx = self.lower_expr(&args[0])?;
            let host_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_get_opt, self.b.func);
            let call = self.b.ins().call(host_ref, &[list, idx]);
            return Ok(self.b.inst_results(call)[0]);
        }
        // Map.get(k) ?? … — same 0 / value+1 Option encoding as list_get_opt.
        if let TExprKind::BuiltinMethod {
            recv,
            op: TBuiltinOp::GetMap,
            args,
        } = &value.kind
        {
            if matches!(
                &recv.ty,
                Type::Map { value, .. } if matches!(value.as_ref(), Type::IntN { .. })
            ) {
                return Err("jit Map<_, IntN>.get needs typed Option lowering".to_string());
            }
            let map = self.lower_expr(recv)?;
            let key = self.lower_expr(&args[0])?;
            let host_ref = self
                .module
                .declare_func_in_func(self.host.coll.map_get_opt, self.b.func);
            let call = self.b.ins().call(host_ref, &[map, key]);
            return Ok(self.b.inst_results(call)[0]);
        }
        // Already-carried Option ABI; IntN uses the result arena.
        if matches!(&value.ty, Type::Option(_)) {
            return self.lower_expr(value);
        }
        // ParsedArgs queries: hosts already return packed Option (TIR may stamp Unit).
        if let TExprKind::HandleMethod { op, .. } = &value.kind {
            if matches!(
                op,
                THandleOp::ParsedArgsOption
                    | THandleOp::ParsedArgsOptionInt
                    | THandleOp::ParsedArgsOptionFloat
                    | THandleOp::ParsedArgsPositional
                    | THandleOp::ParsedArgsSubcommand
            ) {
                return self.lower_expr(value);
            }
        }
        // Recover Option when TIR erased the wrapper but Sema set is_option.
        if let Some(Type::Option(_)) = Self::recover_core_return_ty(value) {
            return self.lower_expr(value);
        }
        // `core.random.weighted_pick` / `Rng.weighted_pick` return the same
        // packed Option encoding; TIR sometimes erases the Option wrapper.
        if let TExprKind::CoreCall { module, method, .. } = &value.kind {
            if module == "core.random" && method == "weighted_pick" {
                return self.lower_expr(value);
            }
        }
        if let TExprKind::HandleMethod {
            op: THandleOp::RngPick | THandleOp::RngWeightedPick,
            ..
        } = &value.kind
        {
            return self.lower_expr(value);
        }
        // ParsedArgs option/positional queries use the same 0 / value+1 pack.
        if let TExprKind::HandleMethod { op, .. } = &value.kind {
            if matches!(
                op,
                THandleOp::ParsedArgsOption
                    | THandleOp::ParsedArgsOptionInt
                    | THandleOp::ParsedArgsOptionFloat
                    | THandleOp::ParsedArgsPositional
                    | THandleOp::ParsedArgsSubcommand
            ) {
                return self.lower_expr(value);
            }
        }
        // Local holding a packed Option from ParsedArgs* (TIR erased Option).
        if let TExprKind::Local(local) = &value.kind {
            let key = TIR::local_place(&local.name);
            if let Some(ty) = self.var_tys.get(&key) {
                if matches!(ty, Type::Option(_)) {
                    return self.lower_expr(value);
                }
            }
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
                let host_id = match (&recv.ty, &args[0].ty) {
                    (Type::Apply { name, .. }, _) if name == "PriorityQueue" => {
                        self.host.coll.priority_queue_push
                    }
                    (_, Type::Float) => self.host.coll.list_push_f64,
                    _ => self.host.coll.list_push,
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::Sort => {
                let host_id = if jit_list_string_type(&recv.ty) {
                    self.host.coll.list_sort_str
                } else {
                    self.host.coll.list_sort
                };
                let host_ref = self
                    .module
                    .declare_func_in_func(host_id, self.b.func);
                self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::LenList => {
                // CoreCall String receivers often lower `.len()` as LenList because
                // `tir_recv_jet_ty` can't see through CoreCall — treat as char len.
                if matches!(&recv.ty, Type::String) {
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.str_len, self.b.func);
                    let call = self.b.ins().call(host_ref, &[recv_val]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if Self::is_view_mut_ty(&recv.ty) {
                    let (_, start, end) = self.unpack_view_mut(recv_val)?;
                    let one = self.b.ins().iconst(types::I64, 1);
                    let span = self.b.ins().isub(end, start);
                    return Ok(self.b.ins().iadd(span, one));
                }
                let host = if matches!(&recv.ty, Type::Apply { name, .. } if name == "Set") {
                    self.host.coll.set_len
                } else if matches!(&recv.ty, Type::Apply { name, .. } if name == "Deque") {
                    self.host.coll.deque_len
                } else if matches!(&recv.ty, Type::Named(name) if name == "BitSet") {
                    self.host.coll.bit_set_len
                } else {
                    self.host.coll.list_len
                };
                let host_ref = self.module.declare_func_in_func(host, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::GetList => {
                if matches!(
                    &recv.ty,
                    Type::List(inner) | Type::FixedList { elem: inner, .. }
                        if matches!(inner.as_ref(), Type::IntN { .. })
                ) {
                    return Err("jit List<IntN>.get needs typed Option lowering".to_string());
                }
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
            TBuiltinOp::IsEmpty => {
                let host = if matches!(&recv.ty, Type::Apply { name, .. } if name == "Set") {
                    self.host.coll.set_len
                } else if matches!(&recv.ty, Type::Apply { name, .. } if name == "Deque") {
                    self.host.coll.deque_len
                } else {
                    self.host.coll.list_len
                };
                let host_ref = self.module.declare_func_in_func(host, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                let len = self.b.inst_results(call)[0];
                let zero = self.b.ins().iconst(types::I64, 0);
                Ok(self.bool_from_icmp(IntCC::Equal, len, zero))
            }
            TBuiltinOp::Pop => {
                if matches!(&recv.ty, Type::Apply { name, .. } if name == "PriorityQueue") {
                    let host = self
                        .module
                        .declare_func_in_func(self.host.coll.priority_queue_pop, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val]);
                    Ok(self.b.inst_results(call)[0])
                } else if jit_list_native_type(&recv.ty)
                    || matches!(&recv.ty, Type::List(elem) if jit_value_type(elem) || matches!(elem.as_ref(), Type::Apply { name, .. } if name == "Task"))
                {
                    let host = self
                        .module
                        .declare_func_in_func(self.host.coll.list_pop, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val]);
                    Ok(self.b.inst_results(call)[0])
                } else {
                    Err("jit builtin method unsupported".to_string())
                }
            }
            TBuiltinOp::InsertMap => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::AddNewMap => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::InsertList => {
                let idx = self.lower_expr(&args[0])?;
                let val = self.lower_expr(&args[1])?;
                let host = self.module.declare_func_in_func(self.host.coll.list_insert, self.b.func);
                self.b.ins().call(host, &[recv_val, idx, val]);
                self.emit_trap_check()?;
                Ok(self.b.ins().iconst(types::I8, 0))
            },
            TBuiltinOp::RemoveMap => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::RemoveList { .. } => {
                let idx = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_remove, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, idx]);
                let removed = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(removed)
            }
            TBuiltinOp::GetMap => {
                if matches!(
                    &recv.ty,
                    Type::Map { value, .. } if matches!(value.as_ref(), Type::IntN { .. })
                ) {
                    return Err("jit Map<_, IntN>.get needs typed Option lowering".to_string());
                }
                let key = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.map_get_opt, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, key]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::First | TBuiltinOp::Last => {
                if matches!(&recv.ty, Type::Apply { name, .. } if name == "SortedSet") {
                    let host_id = if matches!(op, TBuiltinOp::First) {
                        self.host.coll.sorted_set_first
                    } else {
                        self.host.coll.sorted_set_last
                    };
                    let host = self.module.declare_func_in_func(host_id, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val]);
                    Ok(self.b.inst_results(call)[0])
                } else {
                    Err("jit builtin method unsupported".to_string())
                }
            }
            TBuiltinOp::Contains => {
                // Set.has(x) / SortedSet.has — Int elems.
                if matches!(&recv.ty, Type::Apply { name, .. } if name == "Set") {
                    let needle = self.lower_expr(&args[0])?;
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.set_has, self.b.func);
                    let call = self.b.ins().call(host_ref, &[recv_val, needle]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                if matches!(&recv.ty, Type::List(inner) if **inner == Type::String) {
                    let needle = self.lower_expr(&args[0])?;
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.list_contains_str, self.b.func);
                    let call = self.b.ins().call(host_ref, &[recv_val, needle]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                // String.contains(needle) — other list Contains stays unsupported.
                let recv_is_str = matches!(&recv.ty, Type::String)
                    || matches!(
                        Self::recover_core_return_ty(recv),
                        Some(Type::String)
                    )
                    || matches!(
                        recv.kind,
                        TExprKind::Local(ref local)
                            if self
                                .var_tys
                                .get(&TIR::local_place(&local.name))
                                .is_some_and(|t| matches!(t, Type::String))
                    );
                if !recv_is_str {
                    return Err("jit builtin method unsupported".to_string());
                }
                let needle = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_contains, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, needle]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::IndexOf => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Reverse => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Sum { float: false } => {
                if !matches!(
                    jit_list_iter_elem_type(&recv.ty),
                    Some(Type::Int)
                ) {
                    return Err("jit builtin method unsupported".to_string());
                }
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_sum_i64, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Sum { float: true } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Product { float: false } => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.list_product_i64, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Product { float: true } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Min { float: false } => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.list_min_i64, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Max { float: false } => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.list_max_i64, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Min { float: true } | TBuiltinOp::Max { float: true } => {
                Err("jit builtin method unsupported".to_string())
            }
            TBuiltinOp::Flatten => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.list_flatten, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Intersperse => {
                let separator = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.list_intersperse, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, separator]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Unzip { .. } => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.list_unzip, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Clear => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Chars => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_chars, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Bytes => {
                if !matches!(&recv.ty, Type::String) {
                    return Err("jit builtin method unsupported".to_string());
                }
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_bytes, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Split => {
                let sep = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_split, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, sep]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Lines => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_lines, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::ParseInt | TBuiltinOp::ParseFloat => {
                let host = if matches!(op, TBuiltinOp::ParseInt) {
                    self.host.parse_i64
                } else {
                    self.host.parse_f64
                };
                let host_ref = self.module.declare_func_in_func(host, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::StartsWith | TBuiltinOp::EndsWith => {
                if !matches!(&recv.ty, Type::String) {
                    return Err("jit builtin method unsupported".to_string());
                }
                let needle = self.lower_expr(&args[0])?;
                let host_id = if matches!(op, TBuiltinOp::StartsWith) {
                    self.host.str_starts_with
                } else {
                    self.host.str_ends_with
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, needle]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Repeat => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Slice { .. } => {
                if !matches!(&recv.ty, Type::String) {
                    return Err("jit builtin method unsupported".to_string());
                }
                if args.len() != 2 {
                    return Err("jit string slice arity".to_string());
                }
                let start = self.lower_expr(&args[0])?;
                let end = self.lower_expr(&args[1])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_slice, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, start, end]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::After => {
                let sep = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_after, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, sep]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Before => {
                let sep = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.str_before, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, sep]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::TrimView => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.str_trim_view, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::AfterView | TBuiltinOp::BeforeView => {
                let separator = self.lower_expr(&args[0])?;
                let host_id = if matches!(op, TBuiltinOp::AfterView) {
                    self.host.str_after_view
                } else {
                    self.host.str_before_view
                };
                let host = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, separator]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Keys | TBuiltinOp::Values => {
                if !matches!(&recv.ty, Type::Map { .. }) {
                    return Err("jit builtin method unsupported".to_string());
                }
                let host_id = if matches!(op, TBuiltinOp::Keys) {
                    self.host.coll.map_keys
                } else {
                    self.host.coll.map_values
                };
                let host = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::ContainsKey => {
                if matches!(&recv.ty, Type::Apply { name, .. } if name == "Lru") {
                    let key = self.lower_expr(&args[0])?;
                    let host = self
                        .module
                        .declare_func_in_func(self.host.coll.lru_has, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val, key]);
                    Ok(self.b.inst_results(call)[0])
                } else {
                    Err("jit builtin method unsupported".to_string())
                }
            }
            TBuiltinOp::ToString => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::MatchGroup => {
                // Match.group(n) — same host as THandleOp::RegexMethod { method: "group" }.
                let method_id = self.runtime.heap.alloc_string("group".to_string());
                let method_val = self.b.ins().iconst(types::I64, method_id);
                let arg0 = self.lower_expr(&args[0])?;
                let zero = self.b.ins().iconst(types::I64, 0);
                let host = self
                    .module
                    .declare_func_in_func(self.host.text.regex_method, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, method_val, arg0, zero]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::Take => {
                self.lower_iter_adapter(self.host.coll.iter_take, recv_val, Some(&args[0]), None)
            }
            TBuiltinOp::Skip => {
                self.lower_iter_adapter(self.host.coll.iter_skip, recv_val, Some(&args[0]), None)
            }
            TBuiltinOp::StepBy => {
                self.lower_iter_adapter(self.host.coll.iter_step_by, recv_val, Some(&args[0]), None)
            }
            TBuiltinOp::Dedup => {
                let string_elems = matches!(
                    jit_list_iter_elem_type(&recv.ty),
                    Some(Type::String)
                );
                self.lower_iter_adapter(
                    self.host.coll.iter_dedup,
                    recv_val,
                    None,
                    Some(if string_elems { 1 } else { 0 }),
                )
            }
            TBuiltinOp::Chunks => {
                self.lower_iter_adapter(self.host.coll.iter_chunks, recv_val, Some(&args[0]), None)
            }
            TBuiltinOp::Windows => {
                self.lower_iter_adapter(self.host.coll.iter_windows, recv_val, Some(&args[0]), None)
            }
            TBuiltinOp::Indexes => {
                // AOT: `jet_iter_indexes(recv.len())` — JIT materializes list.
                let len_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_len, self.b.func);
                let len_call = self.b.ins().call(len_ref, &[recv_val]);
                let n = self.b.inst_results(len_call)[0];
                let idx_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_indexes, self.b.func);
                let idx_call = self.b.ins().call(idx_ref, &[n]);
                Ok(self.b.inst_results(idx_call)[0])
            }
            TBuiltinOp::Indexed { .. } => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::Zip { .. } => {
                let other = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.list_zip, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, other]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::OptionZip { tuple_struct, elem_ty } => {
                let other = args
                    .first()
                    .ok_or_else(|| "jit Option.zip needs one argument".to_string())?;
                self.lower_option_zip(recv, other, tuple_struct, elem_ty)
            }
            TBuiltinOp::SetFrom => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.set_from_list, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::SetInsert => {
                let v = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.set_insert, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::SetRemove => {
                let v = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.set_remove, self.b.func);
                self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::SetToList => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.set_to_list, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::SetUnion => {
                let other = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.set_union, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, other]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::SortedSetFrom => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.sorted_set_from, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::SortedSetInsert => {
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.sorted_set_insert, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::SortedSetRemove => {
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.sorted_set_remove, self.b.func);
                self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::SortedSetToList => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.sorted_set_to_list, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::SortedSetUnion => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::PriorityQueueFrom => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.priority_queue_from, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::PriorityQueuePeek => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.priority_queue_peek, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::PriorityQueueToSortedList => {
                let host = self.module.declare_func_in_func(
                    self.host.coll.priority_queue_to_sorted_list,
                    self.b.func,
                );
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::LruPut | TBuiltinOp::LruAddNew => {
                let key = self.lower_expr(&args[0])?;
                let value = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.lru_put, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, key, value]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::LruGet => {
                let key = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.lru_get, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, key]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::LruCapacity => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::LruKeys => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.lru_keys, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::BitSetAdd => {
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bit_set_add, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::BitSetRemove => {
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bit_set_remove, self.b.func);
                self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::BitSetCount => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bit_set_count, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::BitSetToList => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bit_set_to_list, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::BitSetNew => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bit_set_new, self.b.func);
                let call = self.b.ins().call(host, &[]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::ByteBufferNew => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.byte_buffer_new, self.b.func);
                let call = self.b.ins().call(host, &[]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::ByteBufferFrom => Err("jit builtin method unsupported".to_string()),
            TBuiltinOp::ByteBufferWrite { method } => {
                let value = self.lower_expr(&args[0])?;
                let method = match method.as_str() {
                    "write_u8" => 0,
                    "write_u16_le" => 1,
                    "write_u16_be" => 2,
                    "write_u32_le" => 3,
                    "write_u32_be" => 4,
                    "write_u64_le" => 5,
                    "write_u64_be" => 6,
                    "write_bytes" => 7,
                    _ => return Err("jit byte-buffer write method unsupported".to_string()),
                };
                let method = self.b.ins().iconst(types::I64, method);
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.byte_buffer_write, self.b.func);
                self.b.ins().call(host, &[recv_val, value, method]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::ByteBufferToBytes => {
                let host = self.module.declare_func_in_func(
                    self.host.coll.byte_buffer_to_bytes,
                    self.b.func,
                );
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::BagAdd => {
                self.require_raw_bag_key(&recv.ty)?;
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bag_add, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::BagRemove => {
                self.require_raw_bag_key(&recv.ty)?;
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bag_remove, self.b.func);
                self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::BagHas => {
                self.require_raw_bag_key(&recv.ty)?;
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bag_has, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::BagCount => {
                self.require_raw_bag_key(&recv.ty)?;
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bag_count, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::BagLen => {
                self.require_raw_bag_key(&recv.ty)?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.coll.bag_len, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::DequePushFront => {
                let v = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.deque_push_front, self.b.func);
                self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::DequePushBack => {
                let v = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.deque_push_back, self.b.func);
                self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            TBuiltinOp::DequePopFront => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.deque_pop_front, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::DequePopBack => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.deque_pop_back, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::DequePeekFront => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.deque_peek_front, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::DequePeekBack => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.deque_peek_back, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            TBuiltinOp::TryCollect => self.lower_try_collect(recv),
            TBuiltinOp::ViewNew { line } => {
                // Inclusive window → exclusive list_slice end. Materialized list
                // handle matches Iter/View JIT ABI (safety.rs).
                let start = args
                    .first()
                    .ok_or_else(|| "jit view needs start".to_string())?;
                let end = args
                    .get(1)
                    .ok_or_else(|| "jit view needs end".to_string())?;
                let s = self.lower_expr(start)?;
                let e = self.lower_expr(end)?;
                let one = self.b.ins().iconst(types::I64, 1);
                let end_excl = self.b.ins().iadd(e, one);
                let line_c = self.b.ins().iconst(types::I32, *line as i64);
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_slice, self.b.func);
                let call = self
                    .b
                    .ins()
                    .call(host_ref, &[recv_val, s, end_excl, line_c]);
                let result = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                Ok(result)
            }
            TBuiltinOp::ViewMutNew { .. } => {
                let start = args
                    .first()
                    .ok_or_else(|| "jit view-mut needs start".to_string())?;
                let end = args
                    .get(1)
                    .ok_or_else(|| "jit view-mut needs end".to_string())?;
                let s = self.lower_expr(start)?;
                let e = self.lower_expr(end)?;
                self.emit_view_mut_window(recv_val, s, e)
            }
            // D-ITERTOOLS1=A: JIT ABI can't carry true JetIter handles. Producers
            // (String.split, list adapters) already return list handles of the same
            // pieces AOT would yield lazily — to_list / collect is identity.
            TBuiltinOp::IterToList | TBuiltinOp::IterCollect => Ok(recv_val),
        }
    }

    fn lower_numeric_method(&mut self, recv: &TExpr, op: &TNumericOp) -> Result<Value, String> {
        let value = self.lower_expr(recv)?;
        match op {
            TNumericOp::Predicate(name) => {
                let op = match name.as_str() {
                    "is_nan" => 0,
                    "is_infinite" => 1,
                    "is_finite" => 2,
                    _ => return Err(format!("jit numeric predicate unsupported: {name}")),
                };
                let op = self.b.ins().iconst(types::I64, op);
                let host = self
                    .module
                    .declare_func_in_func(self.host.numeric_predicate, self.b.func);
                let call = self.b.ins().call(host, &[value, op]);
                Ok(self.b.inst_results(call)[0])
            }
            TNumericOp::BitCount { method: name, .. } => {
                let op = match name.as_str() {
                    "count_ones" => 0,
                    "count_zeros" => 1,
                    "leading_zeros" => 2,
                    "trailing_zeros" => 3,
                    _ => return Err(format!("jit numeric bit query unsupported: {name}")),
                };
                let op = self.b.ins().iconst(types::I64, op);
                let width = match op {
                    _ => match &recv.ty {
                        Type::IntN { bits, .. } => i64::from(*bits),
                        _ => 64,
                    },
                };
                let width = self.b.ins().iconst(types::I64, width);
                let host = self
                    .module
                    .declare_func_in_func(self.host.numeric_bit_count, self.b.func);
                let call = self.b.ins().call(host, &[value, op, width]);
                Ok(self.b.inst_results(call)[0])
            }
            TNumericOp::ToShow => {
                if let Type::IntN { signed, .. } = &recv.ty {
                    let signed = self
                        .b
                        .ins()
                        .iconst(types::I64, i64::from(*signed));
                    let host = self
                        .module
                        .declare_func_in_func(self.host.intn_to_string, self.b.func);
                    let call = self.b.ins().call(host, &[value, signed]);
                    return Ok(self.b.inst_results(call)[0]);
                }
                let begin = self
                    .module
                    .declare_func_in_func(self.host.str_begin, self.b.func);
                let call = self.b.ins().call(begin, &[]);
                let text = self.b.inst_results(call)[0];
                let push = match clif_ty(&recv.ty) {
                    Some(ty) if ty == types::I64 => self.host.str_push_i64,
                    Some(ty) if ty == types::F64 => self.host.str_push_f64,
                    _ => return Err("jit numeric display type unsupported".to_string()),
                };
                let push = self.module.declare_func_in_func(push, self.b.func);
                self.b.ins().call(push, &[text, value]);
                Ok(text)
            }
            TNumericOp::CastAs { dst_rust } => {
                let dst_float = matches!(dst_rust.as_str(), "f32" | "f64");
                if !dst_float {
                    return Ok(value);
                }
                let mut converted = if recv.ty.is_integer() {
                    if matches!(recv.ty, Type::IntN { signed: false, .. }) {
                        self.b.ins().fcvt_from_uint(types::F64, value)
                    } else {
                        self.b.ins().fcvt_from_sint(types::F64, value)
                    }
                } else {
                    value
                };
                if dst_rust == "f32" {
                    let narrowed = self.b.ins().fdemote(types::F32, converted);
                    converted = self.b.ins().fpromote(types::F64, narrowed);
                }
                Ok(converted)
            }
            TNumericOp::TryFrom { host_kind, .. } => {
                let unsigned = i64::from(matches!(recv.ty, Type::IntN { signed: false, .. }));
                let unsigned = self.b.ins().iconst(types::I64, unsigned);
                let kind = self.b.ins().iconst(types::I64, *host_kind);
                let host = self.module.declare_func_in_func(self.host.numeric_try_i64, self.b.func);
                let call = self.b.ins().call(host, &[value, unsigned, kind]);
                Ok(self.b.inst_results(call)[0])
            }
            TNumericOp::FloatToInt { host_kind, .. } => {
                let kind = self.b.ins().iconst(types::I64, *host_kind);
                let host = self.module.declare_func_in_func(self.host.numeric_float_to_int, self.b.func);
                let call = self.b.ins().call(host, &[value, kind]);
                Ok(self.b.inst_results(call)[0])
            }
            TNumericOp::FloatNarrow { .. } => {
                let host = self.module.declare_func_in_func(self.host.numeric_float_narrow, self.b.func);
                let call = self.b.ins().call(host, &[value]);
                Ok(self.b.inst_results(call)[0])
            }
            TNumericOp::Origin(origin) => {
                let _ = value; // AOT: let _ = recv
                let text = origin.as_deref().unwrap_or("untracked");
                let h = self.runtime.heap.alloc_string(text.to_string());
                Ok(self.b.ins().iconst(types::I64, h))
            },
        }
    }

    fn lower_clone(&mut self, inner: &TExpr) -> Result<Value, String> {
        if matches!(
            inner.ty,
            Type::Int
                | Type::Float
                | Type::Bool
                | Type::Char
                | Type::IntN { .. }
                | Type::Float32
        ) {
            return self.lower_expr(inner);
        }
        // Distinct numeric wrappers share the base ABI — clone is a no-op copy.
        if matches!(self.erase_distinct_ty(&inner.ty), Type::Int | Type::Float) {
            return self.lower_expr(inner);
        }
        if inner.ty == Type::String {
            let val = self.lower_expr(inner)?;
            let host_ref = self.module.declare_func_in_func(self.host.str_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        if jit_list_native_type(&inner.ty) {
            let val = self.lower_expr(inner)?;
            let host_ref = self.module.declare_func_in_func(self.host.coll.list_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        if jit_map_string_type(&inner.ty) {
            let val = self.lower_expr(inner)?;
            let host_ref = self.module.declare_func_in_func(self.host.coll.map_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        // List of records — same opaque-handle clone as List(Int/String/Float).
        if jit_list_record_type(&inner.ty) {
            let val = self.lower_expr(inner)?;
            let host_ref = self.module.declare_func_in_func(self.host.coll.list_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        // Named user enums — packed i64 ABI: clone is a bitwise copy (Link trees
        // embed nested payloads in the same word). Named structs allocate fresh.
        if matches!(&inner.ty, Type::Union(_)) {
            return self.lower_expr(inner);
        }
        if let Type::Named(name) = &inner.ty {
            if self.meta.is_enum(name) {
                return self.lower_expr(inner);
            }
            // Opaque net/http/ws runtime handles are i64 slots — clone copies the handle.
            if matches!(
                name.as_str(),
                "TcpListener"
                    | "TcpStream"
                    | "SocketAddr"
                    | "UdpSocket"
                    | "UdpPacket"
                    | "UnixListener"
                    | "UnixStream"
                    | "HttpMux"
                    | "HttpHandler"
                    | "HttpRequest"
                    | "HttpResponse"
                    | "HttpBody"
                    | "HttpHeaders"
                    | "HttpServer"
                    | "HttpShutdownReport"
                    | "WsConn"
                    | "WsMessage"
            ) {
                return self.lower_expr(inner);
            }
            return self.lower_clone_struct(inner);
        }
        if matches!(&inner.ty, Type::Apply { name, .. } if name == "Sender") {
            let val = self.lower_expr(inner)?;
            let host_ref = self
                .module
                .declare_func_in_func(self.host.conc.sender_clone, self.b.func);
            let call = self.b.ins().call(host_ref, &[val]);
            return Ok(self.b.inst_results(call)[0]);
        }
        // Shared<T> is a copyable door: cloning duplicates the handle, not T.
        if matches!(&inner.ty, Type::Shared(_)) {
            return self.lower_expr(inner);
        }
        if let Type::Apply { name, .. } = &inner.ty {
            if self.meta.is_enum(name) {
                return self.lower_expr(inner);
            }
            return self.lower_clone_struct(inner);
        }
        if jit_tuple_type(&inner.ty) {
            return self.lower_clone_tuple(inner);
        }
        // Option packed ABI is a plain i64 — clone is a bitwise copy.
        if matches!(&inner.ty, Type::Option(_)) {
            return self.lower_expr(inner);
        }
        let source = match &inner.kind {
            TExprKind::Local(local) => format!("local {}", local.name),
            TExprKind::Field { field, .. } => format!("field {field}"),
            TExprKind::MethodCall { method, .. } => format!("method {}", method.name),
            _ => "other expression".to_string(),
        };
        Err(format!("jit clone unsupported type: {:?}, {source}", inner.ty))
    }

    fn lower_clone_struct(&mut self, inner: &TExpr) -> Result<Value, String> {
        let type_name = user_type_name(&inner.ty)
            .ok_or_else(|| format!("jit clone unsupported type: {:?}", inner.ty))?;
        let n_fields = self
            .meta
            .struct_layout(type_name)
            .map(|(names, _)| names.len())
            .ok_or_else(|| format!("jit clone unsupported type: {:?}", inner.ty))?;
        let src = self.lower_expr(inner)?;
        let n = self.b.ins().iconst(types::I64, n_fields as i64);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let call = self.b.ins().call(new_ref, &[n]);
        let dst = self.b.inst_results(call)[0];
        let assign_ref = self
            .module
            .declare_func_in_func(self.host.struct_assign, self.b.func);
        self.b.ins().call(assign_ref, &[dst, src]);
        Ok(dst)
    }

    /// Pack a Present payload into the JIT Option i64 ABI.
    /// IntN uses a one-based result-arena handle; legacy payloads use `bits + 1`.
    fn pack_option_payload(&mut self, payload: Value, inner: &Type) -> Result<Value, String> {
        if matches!(inner, Type::IntN { .. }) {
            let ok = self.b.ins().iconst(types::I8, 1);
            let host = self
                .module
                .declare_func_in_func(self.host.result_new_i64, self.b.func);
            let call = self.b.ins().call(host, &[ok, payload]);
            return Ok(self.b.inst_results(call)[0]);
        }
        let bits = match clif_ty(inner) {
            Some(ty) if ty == types::F64 => self.b.ins().bitcast(
                types::I64,
                Self::scalar_bitcast_memflags(),
                payload,
            ),
            Some(ty) if ty == types::I8 => self.b.ins().uextend(types::I64, payload),
            Some(ty) if ty == types::I32 => self.b.ins().uextend(types::I64, payload),
            Some(ty) if ty == types::I64 => payload,
            _ if matches!(inner, Type::Named(_) | Type::Tuple(_) | Type::String) => payload,
            other => {
                return Err(format!("jit Option payload unsupported: {inner:?} ({other:?})"));
            }
        };
        let one = self.b.ins().iconst(types::I64, 1);
        Ok(self.b.ins().iadd(bits, one))
    }

    fn unpack_option_payload(&mut self, packed: Value, inner: &Type) -> Result<Value, String> {
        if matches!(inner, Type::IntN { .. }) {
            let host = self
                .module
                .declare_func_in_func(self.host.result_get_i64, self.b.func);
            let call = self.b.ins().call(host, &[packed]);
            return Ok(self.b.inst_results(call)[0]);
        }
        let one = self.b.ins().iconst(types::I64, 1);
        let bits = self.b.ins().isub(packed, one);
        match clif_ty(inner) {
            Some(ty) if ty == types::F64 => Ok(self.b.ins().bitcast(
                types::F64,
                Self::scalar_bitcast_memflags(),
                bits,
            )),
            Some(ty) if ty == types::I8 => Ok(self.b.ins().ireduce(types::I8, bits)),
            Some(ty) if ty == types::I32 => Ok(self.b.ins().ireduce(types::I32, bits)),
            Some(ty) if ty == types::I64 => Ok(bits),
            _ if matches!(inner, Type::Named(_) | Type::Tuple(_) | Type::String) => Ok(bits),
            other => Err(format!(
                "jit Option payload unsupported: {inner:?} ({other:?})"
            )),
        }
    }

    fn lower_option_zip(
        &mut self,
        recv: &TExpr,
        other: &TExpr,
        tuple_struct: &str,
        elem_ty: &Type,
    ) -> Result<Value, String> {
        let Type::Option(inner_a) = &recv.ty else {
            return Err("jit Option.zip receiver must be Option".to_string());
        };
        let Type::Option(inner_b) = &other.ty else {
            return Err("jit Option.zip argument must be Option".to_string());
        };
        let field_tys: Vec<Type> = match elem_ty {
            Type::Tuple(fields) if fields.len() == 2 => {
                fields.iter().map(|(_, t)| t.as_ref().clone()).collect()
            }
            _ => vec![inner_a.as_ref().clone(), inner_b.as_ref().clone()],
        };
        if field_tys.len() != 2 {
            return Err("jit Option.zip needs a 2-field pair".to_string());
        }
        let a_val = self.lower_expr(recv)?;
        let b_val = self.lower_expr(other)?;
        let none_block = self.b.create_block();
        let check_b = self.b.create_block();
        let some_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        let zero = self.b.ins().iconst(types::I64, 0);
        let a_none = self.b.ins().icmp(IntCC::Equal, a_val, zero);
        self.b.ins().brif(a_none, none_block, &[], check_b, &[]);

        self.b.switch_to_block(check_b);
        self.b.seal_block(check_b);
        let b_none = self.b.ins().icmp(IntCC::Equal, b_val, zero);
        self.b.ins().brif(b_none, none_block, &[], some_block, &[]);

        self.b.switch_to_block(some_block);
        self.b.seal_block(some_block);
        let pa = self.unpack_option_payload(a_val, inner_a)?;
        let pb = self.unpack_option_payload(b_val, inner_b)?;
        // Build the named-tuple record in declaration field order (a, b).
        let n = self.b.ins().iconst(types::I64, 2);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let call = self.b.ins().call(new_ref, &[n]);
        let handle = self.b.inst_results(call)[0];
        for (i, (payload, fty)) in [pa, pb].into_iter().zip(field_tys.iter()).enumerate() {
            let idx = self.b.ins().iconst(types::I64, i as i64);
            let host_id = match fty {
                Type::Int => self.host.struct_set_i64,
                Type::Float => self.host.struct_set_f64,
                Type::Bool => self.host.struct_set_bool,
                Type::Char => self.host.struct_set_char,
                Type::String => self.host.struct_set_str,
                other if clif_ty(other) == Some(types::I64) => self.host.struct_set_i64,
                other => {
                    return Err(format!(
                        "jit Option.zip pair field unsupported on `{tuple_struct}`: {other:?}"
                    ));
                }
            };
            let set_ref = self.module.declare_func_in_func(host_id, self.b.func);
            self.b.ins().call(set_ref, &[handle, idx, payload]);
        }
        let present = self.pack_option_payload(handle, &Type::Named(tuple_struct.to_string()))?;
        self.b.ins().jump(merge, &[present]);

        self.b.switch_to_block(none_block);
        self.b.seal_block(none_block);
        self.b.ins().jump(merge, &[zero]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_option_lift2(
        &mut self,
        f: &TExpr,
        a: &TExpr,
        b: &TExpr,
        ret_ty: &Type,
    ) -> Result<Value, String> {
        let Type::Option(inner_ret) = ret_ty else {
            return Err("jit Option.lift2 result must be Option".to_string());
        };
        let Type::Option(inner_a) = &a.ty else {
            return Err("jit Option.lift2 arg a must be Option".to_string());
        };
        let Type::Option(inner_b) = &b.ty else {
            return Err("jit Option.lift2 arg b must be Option".to_string());
        };
        let TExprKind::Lambda(lam) = &f.kind else {
            return Err("jit Option.lift2 needs a lambda".to_string());
        };
        if !lam.prep.is_empty() || lam.source_params.len() != 2 {
            return Err("jit Option.lift2 lambda shape unsupported".to_string());
        }
        let TLambdaBody::Expr(body) = &lam.executable else {
            return Err("jit Option.lift2 lambda body unsupported".to_string());
        };
        let p0 = TIR::local_place(&lam.source_params[0]);
        let p1 = TIR::local_place(&lam.source_params[1]);
        let a_val = self.lower_expr(a)?;
        let b_val = self.lower_expr(b)?;
        let none_block = self.b.create_block();
        let check_b = self.b.create_block();
        let some_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        let zero = self.b.ins().iconst(types::I64, 0);
        let a_none = self.b.ins().icmp(IntCC::Equal, a_val, zero);
        self.b.ins().brif(a_none, none_block, &[], check_b, &[]);

        self.b.switch_to_block(check_b);
        self.b.seal_block(check_b);
        let b_none = self.b.ins().icmp(IntCC::Equal, b_val, zero);
        self.b.ins().brif(b_none, none_block, &[], some_block, &[]);

        self.b.switch_to_block(some_block);
        self.b.seal_block(some_block);
        let pa = self.unpack_option_payload(a_val, inner_a)?;
        let pb = self.unpack_option_payload(b_val, inner_b)?;
        let mapped = self.with_bound_local(&p0, inner_a.as_ref().clone(), pa, |this| {
            this.with_bound_local(&p1, inner_b.as_ref().clone(), pb, |this| {
                this.lower_expr(body)
            })
        })?;
        let present = self.pack_option_payload(mapped, inner_ret)?;
        self.b.ins().jump(merge, &[present]);

        self.b.switch_to_block(none_block);
        self.b.seal_block(none_block);
        self.b.ins().jump(merge, &[zero]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_option_map(
        &mut self,
        recv: &TExpr,
        args: &[TExpr],
        inner: &Type,
    ) -> Result<Value, String> {
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        let packed = self.lower_expr(recv)?;
        let none_block = self.b.create_block();
        let some_block = self.b.create_block();
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
        let zero = self.b.ins().iconst(types::I64, 0);
        let is_none = self.b.ins().icmp(IntCC::Equal, packed, zero);
        self.b.ins().brif(is_none, none_block, &[], some_block, &[]);

        self.b.switch_to_block(some_block);
        self.b.seal_block(some_block);
        let payload = self.unpack_option_payload(packed, inner)?;
        let mapped = self.with_bound_local(&param_place, inner.clone(), payload, |this| {
            this.lower_expr(body_expr)
        })?;
        let present = self.pack_option_payload(mapped, &body_expr.ty)?;
        self.b.ins().jump(merge, &[present]);

        self.b.switch_to_block(none_block);
        self.b.seal_block(none_block);
        self.b.ins().jump(merge, &[zero]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_clone_tuple(&mut self, inner: &TExpr) -> Result<Value, String> {
        let Type::Tuple(fields) = &inner.ty else {
            return Err("jit tuple clone needs tuple type".to_string());
        };
        let src = self.lower_expr(inner)?;
        let n = self.b.ins().iconst(types::I64, fields.len() as i64);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.struct_new, self.b.func);
        let call = self.b.ins().call(new_ref, &[n]);
        let dst = self.b.inst_results(call)[0];
        for (i, (_, field_ty)) in fields.iter().enumerate() {
            let idx = self.b.ins().iconst(types::I64, i as i64);
            match field_ty.as_ref() {
                Type::Int => {
                    let get = self
                        .module
                        .declare_func_in_func(self.host.struct_get_i64, self.b.func);
                    let gcall = self.b.ins().call(get, &[src, idx]);
                    let val = self.b.inst_results(gcall)[0];
                    let set = self
                        .module
                        .declare_func_in_func(self.host.struct_set_i64, self.b.func);
                    self.b.ins().call(set, &[dst, idx, val]);
                }
                Type::Float => {
                    let get = self
                        .module
                        .declare_func_in_func(self.host.struct_get_f64, self.b.func);
                    let gcall = self.b.ins().call(get, &[src, idx]);
                    let val = self.b.inst_results(gcall)[0];
                    let set = self
                        .module
                        .declare_func_in_func(self.host.struct_set_f64, self.b.func);
                    self.b.ins().call(set, &[dst, idx, val]);
                }
                other => {
                    return Err(format!("jit tuple clone field unsupported: {other:?}"));
                }
            }
        }
        Ok(dst)
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
            let captured = TExpr {
                ty: cap.ty.clone(),
                kind: TExprKind::Local(TLocal::user(&cap.name)),
            };
            let val = if cap.clone_at_spawn {
                self.lower_clone(&captured)?
            } else {
                self.lower_expr(&captured)?
            };
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

    /// `scene.on_frame(…)` — spawn-site callback registered with the game host.
    fn lower_game_on_frame(&mut self, scene: Value) -> Result<Value, String> {
        let site = *self.spawn_site;
        *self.spawn_site += 1;
        let lam = self
            .spawn_lambdas
            .get(site)
            .ok_or_else(|| format!("jit game on_frame site {site} missing lambda"))?;
        let spawn_fn = self
            .spawn_func_ids
            .get(site)
            .copied()
            .ok_or_else(|| format!("jit game on_frame site {site} missing"))?;
        let mut cap_vals = Vec::new();
        for cap in &lam.captures {
            let captured = TExpr {
                ty: cap.ty.clone(),
                kind: TExprKind::Local(TLocal::user(&cap.name)),
            };
            let val = if cap.clone_at_spawn {
                self.lower_clone(&captured)?
            } else {
                self.lower_expr(&captured)?
            };
            cap_vals.push(val);
        }
        if cap_vals.len() > 4 {
            return Err(format!(
                "jit game on_frame capture count {} > 4",
                cap_vals.len()
            ));
        }
        let spawn_ref = self.module.declare_func_in_func(spawn_fn, self.b.func);
        let spawn_ptr = self.b.ins().func_addr(types::I64, spawn_ref);
        let n_caps = self.b.ins().iconst(types::I64, cap_vals.len() as i64);
        let zero = self.b.ins().iconst(types::I64, 0);
        let mut caps = [zero; 4];
        for (i, v) in cap_vals.into_iter().enumerate() {
            caps[i] = v;
        }
        let host = self
            .module
            .declare_func_in_func(self.host.game.on_frame, self.b.func);
        self.b.ins().call(
            host,
            &[scene, spawn_ptr, n_caps, caps[0], caps[1], caps[2], caps[3]],
        );
        Ok(self.b.ins().iconst(types::I8, 0))
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
            THandleOp::FileWriterWriteLine => {
                let line = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.io.file_writer_write_line, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, line]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::FileWriterFlush => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.io.file_writer_flush, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::JSONReaderNext => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.json_reader_next, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::JSONWriterWrite => {
                let ev = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.json_writer_write, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, ev]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::JSONWriterFlush => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.json_writer_flush, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::JSONWriterFinish => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.json_writer_finish, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::JSONLReaderNext => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.jsonl_reader_next, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::JSONLWriterWrite => {
                let v = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.jsonl_writer_write, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::JSONLWriterFlush => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.jsonl_writer_flush, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::JSONLWriterFinish => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.jsonl_writer_finish, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CSVReaderNext => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.csv_reader_next, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DataStreamNext => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.data.stream_next, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CSVWriterWrite => {
                let v = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.csv_writer_write, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CSVWriterFlush => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.csv_writer_flush, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CSVWriterFinish => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.csv_writer_finish, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::XMLReaderNext => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.xml_reader_next, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::XMLWriterWrite => {
                let v = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.xml_writer_write, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, v]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::XMLWriterFlush => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.xml_writer_flush, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::XMLWriterFinish => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.xml_writer_finish, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CBORReaderNext => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.cbor_reader_next, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CBORWriterWrite => {
                let ev = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.cbor_writer_write, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, ev]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CBORWriterFlush => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.cbor_writer_flush, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CBORWriterFinish => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.stream.cbor_writer_finish, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StdinReadLine => Err("jit handle method unsupported".to_string()),
            THandleOp::StdoutWrite => {
                let text = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stdout_write, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, text]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StdoutWriteLine => {
                let text = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stdout_write_line, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, text]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StdoutWriteBytes => {
                let bytes = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stdout_write_bytes, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, bytes]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StdoutFlush => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stdout_flush, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StdoutIsTty => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stdout_is_tty, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StderrWrite => {
                let text = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stderr_write, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, text]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StderrWriteLine => {
                let text = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stderr_write_line, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, text]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StderrWriteBytes => {
                let bytes = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stderr_write_bytes, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, bytes]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StderrFlush => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stderr_flush, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StderrIsTty => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.io.stderr_is_tty, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::StopwatchElapsedMillis => Err("jit handle method unsupported".to_string()),
            THandleOp::ClockNow => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.clock_now, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ClockTick => {
                let delta = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.clock_tick, self.b.func);
                self.b.ins().call(host, &[recv_val, delta]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::ClockAdvance => {
                let value = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.clock_advance, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, value]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ClockWait => {
                // After `??` unwrap, Duration is the raw millisecond i64 (see
                // `jet_jit_duration_from_*` / `duration_in`), not a struct handle.
                let duration_ms = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.clock_wait, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, duration_ms]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngInt => {
                let lo = self.lower_expr(&args[0])?;
                let hi = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_int, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, lo, hi]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngFloat => Err("jit handle method unsupported".to_string()),
            THandleOp::RngFloatRange => {
                let lo = self.lower_expr(&args[0])?;
                let hi = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_float_range, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, lo, hi]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngBool => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_bool, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngBoolP => {
                let p = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_bool_p, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, p]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngNormal => Err("jit handle method unsupported".to_string()),
            THandleOp::RngExponential => Err("jit handle method unsupported".to_string()),
            THandleOp::RngBytes => {
                let n = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_bytes, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, n]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngSplit => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_split, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngPick => {
                let items = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_pick, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, items]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngWeightedPick => {
                if matches!(
                    &args[0].ty,
                    Type::List(inner) | Type::FixedList { elem: inner, .. }
                        if matches!(inner.as_ref(), Type::IntN { .. })
                ) {
                    return Err(
                        "jit Rng.weighted_pick<IntN> needs typed Option lowering".to_string(),
                    );
                }
                let items = self.lower_expr(&args[0])?;
                let weights = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_weighted_pick, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, items, weights]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngSample => {
                let items = self.lower_expr(&args[0])?;
                let k = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_sample, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, items, k]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::RngShuffle => {
                let items = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.random.rng_shuffle, self.b.func);
                self.b.ins().call(host, &[recv_val, items]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
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
            THandleOp::GameSceneNew => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.scene_new, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::GameReplayRecord => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.replay_record, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::GameBackendHeadless => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.backend_headless, self.b.func);
                let call = self.b.ins().call(host, &[]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::GameSceneOnFrame => self.lower_game_on_frame(recv_val),
            THandleOp::GameSceneComponent => {
                let name = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.component, self.b.func);
                self.b.ins().call(host, &[recv_val, name]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::GameSceneQuery => {
                let names = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.query, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, names]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::GameAssetsImage => {
                let path = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.assets_image, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, path]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::GameAssetsSound => {
                let path = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.assets_sound, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, path]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::GameInputBind => {
                let action = self.lower_expr(&args[0])?;
                let key = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.input_bind, self.b.func);
                self.b.ins().call(host, &[recv_val, action, key]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::GameInputPressed => {
                let action = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.game.input_pressed, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, action]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DurationNew { unit, float } => {
                let scale = match *unit {
                    "Milliseconds" => 1,
                    "Seconds" => 1_000,
                    "Minutes" => 60_000,
                    "Hours" => 3_600_000,
                    _ => return Err("jit duration unit unsupported".to_string()),
                };
                let scale = self.b.ins().iconst(types::I64, scale);
                let host_id = if *float {
                    self.host.duration_from_float
                } else {
                    self.host.duration_from_int
                };
                let host = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, scale]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DurationIn { unit: Some(unit) } => {
                let scale = match *unit {
                    "Milliseconds" => 1,
                    "Seconds" => 1_000,
                    "Minutes" => 60_000,
                    "Hours" => 3_600_000,
                    _ => return Err("jit duration unit unsupported".to_string()),
                };
                let scale = self.b.ins().iconst(types::I64, scale);
                let host = self.module.declare_func_in_func(self.host.duration_in, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, scale]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DurationIn { unit: None } => {
                Err("jit dynamic DurationUnit falls back to AOT".to_string())
            }
            // D-BIGINT1 / D-DECIMAL1: instance methods on precise numerics.
            THandleOp::PreciseMethod { type_name, method }
                if type_name == "BigInt" || type_name == "Decimal" =>
            {
                let (host_fn, extra_args) = match (type_name.as_str(), method.as_str()) {
                    ("BigInt", "add") => (self.host.num.bigint_add, 1),
                    ("BigInt", "sub") => (self.host.num.bigint_sub, 1),
                    ("BigInt", "mul") => (self.host.num.bigint_mul, 1),
                    ("BigInt", "neg") => (self.host.num.bigint_neg, 0),
                    ("BigInt", "to_string") => (self.host.num.bigint_to_string, 0),
                    ("Decimal", "add") => (self.host.num.decimal_add, 1),
                    ("Decimal", "sub") => (self.host.num.decimal_sub, 1),
                    ("Decimal", "mul") => (self.host.num.decimal_mul, 1),
                    ("Decimal", "to_string") => (self.host.num.decimal_to_string, 0),
                    _ => {
                        return Err(format!(
                            "jit handle method unsupported: {type_name}::{method}"
                        ))
                    }
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
            THandleOp::TcpListenerAccept => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.net_http.tcp_accept, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::TcpListenerLocalAddr => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.net_http.tcp_local_addr, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::TcpStreamReadText if args.len() == 1 => {
                let limit = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.net_http.tcp_read_text, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, limit]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::TcpStreamWriteAllBytes if args.len() == 1 => {
                let data = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.net_http.tcp_write_all_bytes, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, data]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::TcpStreamReadText | THandleOp::TcpStreamWriteAllBytes => {
                Err("jit handle method unsupported".to_string())
            }
            THandleOp::TcpStreamClose => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.net_http.tcp_close, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::TcpStreamRead => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamWrite => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamPeerAddr => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamLocalAddr => Err("jit handle method unsupported".to_string()),
            THandleOp::TcpStreamReadBytes
            | THandleOp::TcpStreamWriteBytes
            | THandleOp::TcpStreamWriteText
            | THandleOp::TcpStreamShutdown
            | THandleOp::TcpStreamReady
            | THandleOp::UdpSocketReady
            | THandleOp::UdpSocketClose
            | THandleOp::UdpSocketReceiveDeadline
            | THandleOp::UdpSocketSendToDeadline
            | THandleOp::UnixListenerAcceptDeadline
            | THandleOp::UnixStreamReadDeadline
            | THandleOp::UnixStreamWriteAllDeadline
            | THandleOp::UnixStreamReady
            | THandleOp::UnixStreamClose
            | THandleOp::UnixStreamSetTimeout
            | THandleOp::TlsStreamReadDeadline
            | THandleOp::TlsStreamWriteAllDeadline
            | THandleOp::TlsStreamReady
            | THandleOp::TlsStreamClose
            | THandleOp::TlsStreamCloseWrite
            | THandleOp::TlsStreamPeerIdentity
            | THandleOp::TlsClientConfigDefault
            | THandleOp::HttpClientNew
            | THandleOp::TlsClientConfigWithAlpn
            | THandleOp::TlsRootCertificatesFromPem
            | THandleOp::TlsClientIdentityFromPem
            | THandleOp::TlsClientConfigWithTrust
            | THandleOp::TlsClientConfigWithIdentity
            | THandleOp::TlsClientConfigWithVersionBounds => {
                Err("jit handle method unsupported".to_string())
            }
            THandleOp::AllocAlloc if args.len() == 1 => {
                let value = self.lower_expr(&args[0])?;
                let bits = match self.meta.clif_ty(&args[0].ty) {
                    Some(ty) if ty == types::F64 => self.b.ins().bitcast(
                        types::I64,
                        Self::scalar_bitcast_memflags(),
                        value,
                    ),
                    Some(ty) if ty == types::I8 => self.b.ins().uextend(types::I64, value),
                    Some(ty) if ty == types::I32 => self.b.ins().uextend(types::I64, value),
                    Some(ty) if ty == types::I64 => value,
                    other => {
                        return Err(format!(
                            "jit allocator payload unsupported: {:?} ({other:?})",
                            args[0].ty
                        ))
                    }
                };
                let host = self
                    .module
                    .declare_func_in_func(self.host.memory.allocator_alloc, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, bits]);
                let bits = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                match self.meta.clif_ty(&args[0].ty) {
                    Some(ty) if ty == types::F64 => Ok(self.b.ins().bitcast(
                        types::F64,
                        Self::scalar_bitcast_memflags(),
                        bits,
                    )),
                    Some(ty) if ty == types::I8 => Ok(self.b.ins().ireduce(types::I8, bits)),
                    Some(ty) if ty == types::I32 => Ok(self.b.ins().ireduce(types::I32, bits)),
                    _ => Ok(bits),
                }
            }
            THandleOp::AllocReset if args.is_empty() => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.memory.allocator_reset, self.b.func);
                self.b.ins().call(host, &[recv_val]);
                self.emit_trap_check()?;
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::AllocAlloc | THandleOp::AllocReset => {
                Err("jit allocator method arity unsupported".to_string())
            }
            THandleOp::HttpReqField(..) => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpReqHeader => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpReqParam => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpRespField(..) => Err("jit handle method unsupported".to_string()),
            THandleOp::HttpRespHeader => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecFlag => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let host = self.module.declare_func_in_func(self.host.args.flag, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecFlagShort => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let a2 = self.lower_expr(&args[2])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.flag_short, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1, a2]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecOption => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let a2 = self.lower_expr(&args[2])?;
                let host = self.module.declare_func_in_func(self.host.args.option, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1, a2]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecOptionShort => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionDefault => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let a2 = self.lower_expr(&args[2])?;
                let a3 = self.lower_expr(&args[3])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.option_default, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1, a2, a3]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecOptionEnv => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionInt => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let a2 = self.lower_expr(&args[2])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.option_int, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1, a2]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecOptionFloat => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecOptionChoice => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let a2 = self.lower_expr(&args[2])?;
                let a3 = self.lower_expr(&args[3])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.option_choice, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1, a2, a3]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecRepeat => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let a2 = self.lower_expr(&args[2])?;
                let host = self.module.declare_func_in_func(self.host.args.repeat, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1, a2]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecRequiredOption => Err("jit handle method unsupported".to_string()),
            THandleOp::ArgsSpecPositional => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.positional, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecSubcommand => {
                let a0 = self.lower_expr(&args[0])?;
                let a1 = self.lower_expr(&args[1])?;
                let a2 = self.lower_expr(&args[2])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.subcommand, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0, a1, a2]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecVersion => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(self.host.args.version, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecCompletion => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.completion, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecHelp => {
                let host = self.module.declare_func_in_func(self.host.args.help, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ArgsSpecParse => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(self.host.args.parse, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ParsedArgsFlag => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.parsed_flag, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ParsedArgsOption => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.parsed_option, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ParsedArgsOptionInt => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.parsed_option_int, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ParsedArgsOptionFloat => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.parsed_option_float, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ParsedArgsOptions => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.parsed_options, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ParsedArgsSubcommand => {
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.parsed_subcommand, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ParsedArgsPositional => {
                let a0 = self.lower_expr(&args[0])?;
                let host = self
                    .module
                    .declare_func_in_func(self.host.args.parsed_positional, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, a0]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ProcessSpecMethod { method } => {
                let (host_fn, arity) = match method.as_str() {
                    "stdout" => (self.host.process.spec_stdout, 1),
                    "stderr" => (self.host.process.spec_stderr, 1),
                    "stdin" => (self.host.process.spec_stdin, 1),
                    "timeout" => (self.host.process.spec_timeout, 1),
                    "output_limit" => (self.host.process.spec_output_limit, 1),
                    "cwd" => (self.host.process.spec_cwd, 1),
                    "run" => (self.host.process.spec_run, 0),
                    "spawn" => (self.host.process.spec_spawn, 0),
                    _ => return Err("jit handle method unsupported".to_string()),
                };
                let mut arg_vals = vec![recv_val];
                for a in args.iter().take(arity) {
                    arg_vals.push(self.lower_expr(a)?);
                }
                let host = self.module.declare_func_in_func(host_fn, self.b.func);
                let call = self.b.ins().call(host, &arg_vals);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ProcessChildMethod { method } => match method.as_str() {
                "id" if args.is_empty() => {
                    let host = self
                        .module
                        .declare_func_in_func(self.host.process.child_id, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val]);
                    Ok(self.b.inst_results(call)[0])
                }
                "kill" | "terminate" | "interrupt" if args.is_empty() => {
                    let host = self
                        .module
                        .declare_func_in_func(self.host.process.child_kill, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val]);
                    Ok(self.b.inst_results(call)[0])
                }
                "wait" if args.is_empty() => {
                    let host = self
                        .module
                        .declare_func_in_func(self.host.process.child_wait, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val]);
                    Ok(self.b.inst_results(call)[0])
                }
                _ => Err("jit handle method unsupported".to_string()),
            },
            THandleOp::ProcessStdinWrite => Err("jit handle method unsupported".to_string()),
            THandleOp::TaskDetach => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_detach, self.b.func);
                self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::TaskPause => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_pause, self.b.func);
                self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::TaskResume => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_resume, self.b.func);
                self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::TaskTrace => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.conc.task_trace, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReflectValueTypeName => {
                let host = self.module.declare_func_in_func(self.host.reflect_type_name, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReflectValueDisplay => {
                let host = self.module.declare_func_in_func(self.host.reflect_display, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReflectValueFields => {
                let host = self.module.declare_func_in_func(self.host.reflect_fields, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReflectFieldName => {
                let host = self.module.declare_func_in_func(self.host.reflect_field_name, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReflectFieldValue => {
                let host = self.module.declare_func_in_func(self.host.reflect_field_value, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::HttpRouterRegister { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::MathMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::ReactiveGet => Err("jit handle method unsupported".to_string()),
            THandleOp::ReactiveSet => Err("jit handle method unsupported".to_string()),
            THandleOp::ReactiveEffectMethod { .. } => {
                Err("jit handle method unsupported".to_string())
            }
            THandleOp::EventMethod { method } => match method.as_str() {
                "cancel" if args.is_empty() => {
                    let host = self
                        .module
                        .declare_func_in_func(self.host.watcher.event_scope_cancel, self.b.func);
                    self.b.ins().call(host, &[recv_val]);
                    Ok(self.b.ins().iconst(types::I8, 0))
                }
                "is_active" if args.is_empty() => {
                    let host = self.module.declare_func_in_func(
                        self.host.watcher.subscription_is_active,
                        self.b.func,
                    );
                    let call = self.b.ins().call(host, &[recv_val]);
                    Ok(self.b.inst_results(call)[0])
                }
                _ => Err(format!("jit event method unsupported: {method}")),
            },
            THandleOp::WatchMethod {
                method,
                callback_index,
            } => self.lower_watch_method(recv, recv_val, method, *callback_index, args),
            THandleOp::MeasurementMethod { method } => match method.as_str() {
                "value" | "uncertainty" if args.is_empty() => {
                    let field = self
                        .b
                        .ins()
                        .iconst(types::I64, i64::from(method == "uncertainty"));
                    let host = self
                        .module
                        .declare_func_in_func(self.host.measurement_get, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val, field]);
                    Ok(self.b.inst_results(call)[0])
                }
                "add" | "sub" | "mul" | "div" if args.len() == 1 => {
                    let right = self.lower_expr(&args[0])?;
                    let op = match method.as_str() {
                        "add" => 0,
                        "sub" => 1,
                        "mul" => 2,
                        "div" => 3,
                        _ => unreachable!(),
                    };
                    let op = self.b.ins().iconst(types::I64, op);
                    let host = self
                        .module
                        .declare_func_in_func(self.host.measurement_arithmetic, self.b.func);
                    let call = self.b.ins().call(host, &[recv_val, right, op]);
                    Ok(self.b.inst_results(call)[0])
                }
                _ => Err("jit handle method unsupported".to_string()),
            },
            THandleOp::LayoutMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::LoadableMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::ExpiringMethod { method } => {
                let clock = args
                    .first()
                    .ok_or_else(|| format!("jit ExpiringValue.{method} needs a clock"))?;
                let clock = self.lower_expr(clock)?;
                match method.as_str() {
                    "get" => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.memory.expiring_get, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, clock]);
                        let status = self.b.inst_results(call)[0];
                        Ok(self.result_from_packed_i64(status))
                    }
                    "is_valid" => {
                        let host = self.module.declare_func_in_func(
                            self.host.memory.expiring_is_valid,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, clock]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    _ => Err(format!("jit ExpiringValue method unsupported: {method}")),
                }
            }
            THandleOp::SketchMethod { sketch, method } => {
                let kind = match sketch.as_str() {
                    "HyperLogLog" => 0i64,
                    "TDigest" => 1,
                    "CountMinSketch" => 2,
                    "ReservoirSampler" => 3,
                    _ => return Err(format!("jit sketch unsupported: {sketch}")),
                };
                match method.as_str() {
                    "add" if sketch == "TDigest" && args.len() == 1 => {
                        let v = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.sketch.add_f64, self.b.func);
                        self.b.ins().call(host, &[recv_val, v]);
                        Ok(self.b.ins().iconst(types::I8, 0))
                    }
                    "add" if args.len() == 1 => {
                        let s = self.lower_expr(&args[0])?;
                        let kind_v = self.b.ins().iconst(types::I64, kind);
                        let host = self
                            .module
                            .declare_func_in_func(self.host.sketch.add_str, self.b.func);
                        self.b.ins().call(host, &[recv_val, kind_v, s]);
                        Ok(self.b.ins().iconst(types::I8, 0))
                    }
                    "count" if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.sketch.count0, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    "count" if args.len() == 1 => {
                        let key = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.sketch.count1, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, key]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    "quantile" if args.len() == 1 => {
                        let q = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.sketch.quantile, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, q]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    "sample" if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.sketch.sample, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    _ => Err(format!("jit sketch method unsupported: {sketch}.{method}")),
                }
            }
            THandleOp::CivilTimeMethod { method, .. } => {
                let recv_v = self.lower_expr(recv)?;
                let method_id = self.runtime.heap.alloc_string(method.clone());
                let method_val = self.b.ins().iconst(types::I64, method_id);
                let a0 = if let Some(a) = args.first() {
                    self.lower_expr(a)?
                } else {
                    self.b.ins().iconst(types::I64, 0)
                };
                let a1 = if let Some(a) = args.get(1) {
                    self.lower_expr(a)?
                } else {
                    self.b.ins().iconst(types::I64, 0)
                };
                let host = self
                    .module
                    .declare_func_in_func(self.host.time.civil_method, self.b.func);
                let call = self.b.ins().call(host, &[recv_v, method_val, a0, a1]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::UrlMimeMethod { method, .. } => {
                let (host_id, arg_vals): (FuncId, Vec<Value>) = match method.as_str() {
                    "to_string" if args.is_empty() => {
                        (self.host.net.url_to_string, vec![recv_val])
                    }
                    "host" if args.is_empty() => (self.host.net.url_host, vec![recv_val]),
                    "path" if args.is_empty() => (self.host.net.url_path, vec![recv_val]),
                    "query_pairs" if args.is_empty() => {
                        (self.host.net.url_query_pairs, vec![recv_val])
                    }
                    "path_segments" if args.is_empty() => {
                        (self.host.net.url_path_segments, vec![recv_val])
                    }
                    "fragment" if args.is_empty() => {
                        (self.host.net.url_fragment, vec![recv_val])
                    }
                    "join" if args.len() == 1 => (
                        self.host.net.url_join,
                        vec![recv_val, self.lower_expr(&args[0])?],
                    ),
                    "essence" if args.is_empty() => {
                        (self.host.net.mime_essence, vec![recv_val])
                    }
                    "param" if args.len() == 1 => (
                        self.host.net.mime_param,
                        vec![recv_val, self.lower_expr(&args[0])?],
                    ),
                    _ => {
                        return Err(format!("jit UrlMime method unsupported: {method}"))
                    }
                };
                let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
                let call = self.b.ins().call(host_ref, &arg_vals);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::EmailMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::RegexMethod { method, .. } => {
                let recv_v = self.lower_expr(recv)?;
                let method_name = if method == "replace_all_with" {
                    // Constant-string lambdas fold to replace_all (canonical
                    // algorithm); non-constant callbacks stay unsupported.
                    "replace_all"
                } else {
                    method.as_str()
                };
                let method_id = self.runtime.heap.alloc_string(method_name.to_string());
                let method_val = self.b.ins().iconst(types::I64, method_id);
                let a0 = if let Some(a) = args.first() {
                    self.lower_expr(a)?
                } else {
                    self.b.ins().iconst(types::I64, 0)
                };
                let a1 = if let Some(a) = args.get(1) {
                    if method == "replace_all_with" {
                        if let Some(text) = Self::constant_string_lambda(a) {
                            let sid = self.runtime.heap.alloc_string(text);
                            self.b.ins().iconst(types::I64, sid)
                        } else {
                            return Err(
                                "jit regex replace_all_with non-constant callback unsupported"
                                    .to_string(),
                            );
                        }
                    } else {
                        self.lower_expr(a)?
                    }
                } else {
                    self.b.ins().iconst(types::I64, 0)
                };
                let host = self
                    .module
                    .declare_func_in_func(self.host.text.regex_method, self.b.func);
                let call = self.b.ins().call(host, &[recv_v, method_val, a0, a1]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::HttpClientMethod { kind, method } => {
                match (kind.as_str(), method.as_str()) {
                    ("HttpResponse", "status") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_resp_status, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpResponse", "body") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_resp_body, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpResponse", "header") if args.len() == 1 => {
                        let name = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_resp_header, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, name]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpResponse", "cookies") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_resp_cookies, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpBody", "text") if args.len() == 1 => {
                        let limit = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_body_text, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, limit]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "form") if args.len() == 2 => {
                        let a0 = self.lower_expr(&args[0])?;
                        let a1 = self.lower_expr(&args[1])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_client_request_form,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, a0, a1]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "cookie") if args.len() == 2 => {
                        let a0 = self.lower_expr(&args[0])?;
                        let a1 = self.lower_expr(&args[1])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_client_request_cookie,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, a0, a1]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "header") if args.len() == 2 => {
                        let a0 = self.lower_expr(&args[0])?;
                        let a1 = self.lower_expr(&args[1])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_client_request_header,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, a0, a1]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "redirects") if args.len() == 1 => {
                        let a0 = self.lower_expr(&args[0])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_client_request_redirects,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, a0]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "connect_timeout") if args.len() == 1 => {
                        let a0 = self.lower_expr(&args[0])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_client_request_connect_timeout,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, a0]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "read_timeout") if args.len() == 1 => {
                        let a0 = self.lower_expr(&args[0])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_client_request_read_timeout,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, a0]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "send") if args.is_empty() => {
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_client_request_send,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    _ => Err(format!(
                        "jit handle method unsupported: {kind}::{method}"
                    )),
                }
            }
            THandleOp::HttpServerMethod { kind, method } => {
                match (kind.as_str(), method.as_str()) {
                    ("HttpMux", m)
                        if matches!(
                            m,
                            "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
                        ) && args.len() == 2 =>
                    {
                        let pattern = self.lower_expr(&args[0])?;
                        let handler = self.lower_expr(&args[1])?;
                        let method_s = self.runtime.heap.alloc_string(m.to_uppercase());
                        let method_v = self.b.ins().iconst(types::I64, method_s);
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_mux_add, self.b.func);
                        let call =
                            self.b
                                .ins()
                                .call(host, &[recv_val, method_v, pattern, handler]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpMux", "middleware") if args.len() == 1 => {
                        let mw = self.lower_expr(&args[0])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_mux_middleware,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, mw]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpHandler", "handle") if args.len() == 1 => {
                        let req = self.lower_expr(&args[0])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_handler_handle,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, req]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "body") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_req_body, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "method") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_req_method, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "path") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_req_path, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "param") if args.len() == 1 => {
                        let name = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_req_param, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, name]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "header") if args.len() == 1 => {
                        let name = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_req_header, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, name]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "trailers") if args.is_empty() => {
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_req_trailers,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "body_len") if args.is_empty() => {
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_req_body_len,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpRequest", "under_limit") if args.len() == 1 => {
                        let max = self.lower_expr(&args[0])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_req_under_limit,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, max]);
                        let v = self.b.inst_results(call)[0];
                        Ok(self.b.ins().ireduce(types::I8, v))
                    }
                    ("HttpResponse", "trailers") if args.len() == 1 => {
                        let trailers = self.lower_expr(&args[0])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_resp_trailers,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, trailers]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpBody", "text") if args.len() == 1 => {
                        let limit = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_body_text, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, limit]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpResponse", "status") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_resp_status, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpResponse", "body") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_resp_body, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpServer", "local_addr") if args.is_empty() => {
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_server_local_addr,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpServer", "serve") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.http_server_serve, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("HttpServer", "shutdown") if args.len() == 1 => {
                        // Duration handle -> ms via DurationIn not available here; pass
                        // duration record bits as ms when it's a Duration milliseconds handle.
                        let grace = self.lower_expr(&args[0])?;
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.http_server_shutdown,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val, grace]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("WsConn", "send_text") if args.len() == 1 => {
                        let text = self.lower_expr(&args[0])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.ws_send_text, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, text]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("WsConn", "recv") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.ws_recv, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("WsConn", "close") if args.len() == 2 => {
                        let code = self.lower_expr(&args[0])?;
                        let reason = self.lower_expr(&args[1])?;
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.ws_close, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val, code, reason]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    ("WsMessage", "is_text") if args.is_empty() => {
                        let host = self.module.declare_func_in_func(
                            self.host.net_http.ws_message_is_text,
                            self.b.func,
                        );
                        let call = self.b.ins().call(host, &[recv_val]);
                        let v = self.b.inst_results(call)[0];
                        Ok(self.b.ins().ireduce(types::I8, v))
                    }
                    ("WsMessage", "text") if args.is_empty() => {
                        let host = self
                            .module
                            .declare_func_in_func(self.host.net_http.ws_message_text, self.b.func);
                        let call = self.b.ins().call(host, &[recv_val]);
                        Ok(self.b.inst_results(call)[0])
                    }
                    _ => Err(format!(
                        "jit handle method unsupported: {kind}::{method}"
                    )),
                }
            }
            THandleOp::HttpReqTrailers => {
                let host = self.module.declare_func_in_func(
                    self.host.net_http.http_req_trailers,
                    self.b.func,
                );
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::HttpRespTrailers => {
                let trailers = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(
                    self.host.net_http.http_resp_trailers,
                    self.b.func,
                );
                let call = self.b.ins().call(host, &[recv_val, trailers]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DataTreeField | THandleOp::JsonField => {
                let name = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_field, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, name]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DataTreeAt | THandleOp::JsonAt => {
                let idx = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_at, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, idx]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DataTreeInt | THandleOp::JsonInt => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_int, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DataTreeText | THandleOp::JsonText => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_text, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DataTreeBool | THandleOp::JsonBool => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_bool, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DataTreeFloat | THandleOp::JsonFloat => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.encoding.datatree_float, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                // Result stores f64 bits; convert via result_get_f64 at use sites.
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PathFrom => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.path_from, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PathJoin => {
                let part = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.path_join_handle, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, part]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PathParent => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.path_parent, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PathExtension => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.path_extension, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PathStem => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.path_stem, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PathToString => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.path_to_string, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PathWalk => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.path_walk, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PathWriteAtomic => {
                let bytes = self.lower_expr(&args[0])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.core.path_write_atomic, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, bytes]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::UiBackendMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::DevServerMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::WebAppMethod { .. } => Err("jit handle method unsupported".to_string()),
            THandleOp::DbQuery => {
                let sql = self.lower_expr(&args[0])?;
                let params = self.lower_expr(&args[1])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.query, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, sql, params]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbQueryOne => {
                let sql = self.lower_expr(&args[0])?;
                let params = self.lower_expr(&args[1])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.query_one, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, sql, params]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbExecute => {
                let sql = self.lower_expr(&args[0])?;
                let params = self.lower_expr(&args[1])?;
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.execute, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val, sql, params]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbBegin => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.begin, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbCommit => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.commit, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbRollback => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.rollback, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbClose => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.close, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbValueInt => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.dbvalue_int, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbValueFloat => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.dbvalue_float, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbValueText => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.dbvalue_text, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbValueBool => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.dbvalue_bool, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DbValueIsNull => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.db.dbvalue_is_null, self.b.func);
                let call = self.b.ins().call(host_ref, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::PluginCall => Err("jit handle method unsupported".to_string()),
            THandleOp::PluginCallInt => Err("jit handle method unsupported".to_string()),
            THandleOp::ReaderOver => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_over, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderReadU8 => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_read_u8, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderReadU16Le => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_read_u16_le, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderReadU16Be => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_read_u16_be, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderReadU32Le => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_read_u32_le, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderReadU32Be => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_read_u32_be, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderReadU64Le => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_read_u64_le, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderReadU64Be => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_read_u64_be, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderTake => {
                let n = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(self.host.parse.reader_take, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, n]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderRemaining => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_remaining, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderAtEnd => {
                let host = self.module.declare_func_in_func(self.host.parse.reader_at_end, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CursorOver => {
                let host = self.module.declare_func_in_func(self.host.parse.cursor_over, self.b.func);
                let call = self.b.ins().call(host, &[recv_val]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CursorTakeUntil => {
                let d = self.lower_expr(&args[0])?;
                let host = self.module.declare_func_in_func(self.host.parse.cursor_take_until, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, d]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::CursorSkipWs => {
                let host = self.module.declare_func_in_func(self.host.parse.cursor_skip_ws, self.b.func);
                self.b.ins().call(host, &[recv_val]);
                Ok(self.b.ins().iconst(types::I8, 0))
            }
            THandleOp::CursorTakePattern { parts, .. } => {
                let pid = crate::Parse::install_str_pattern(parts.clone());
                let pid_v = self.b.ins().iconst(types::I64, pid);
                let host = self.module.declare_func_in_func(self.host.parse.cursor_take_pattern, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, pid_v]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::ReaderTakePattern { parts, .. } => {
                let pid = crate::Parse::install_bin_pattern(parts.clone());
                let pid_v = self.b.ins().iconst(types::I64, pid);
                let host = self.module.declare_func_in_func(self.host.parse.reader_take_pattern, self.b.func);
                let call = self.b.ins().call(host, &[recv_val, pid_v]);
                Ok(self.b.inst_results(call)[0])
            }
            THandleOp::DataTreeDecode(target) => {
                let tree = self.lower_expr(recv)?;
                self.lower_datatree_decode(tree, target)
            }
            THandleOp::SerdeEncode => self.lower_serde_encode(recv),
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
    fn lower_compare_chain(
        &mut self,
        operands: &[TExpr],
        ops: &[BinOp],
        hooks: &[bool],
    ) -> Result<Value, String> {
        if operands.len() != ops.len() + 1 || hooks.len() != ops.len() {
            return Err("jit compare chain arity mismatch".to_string());
        }
        let vals: Result<Vec<_>, _> = operands.iter().map(|e| self.lower_expr(e)).collect();
        let vals = vals?;
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I8);
        for (i, op) in ops.iter().enumerate() {
            let cmp = if hooks[i] {
                let key = self.method_key(&operands[i].ty, &TMethodRef::inherent("compare"))
                    .ok_or_else(|| format!("jit compare hook on {:?}", operands[i].ty))?;
                let func_id = self
                    .func_ids
                    .get(&key)
                    .copied()
                    .ok_or_else(|| format!("jit missing method `{key}`"))?;
                let func_ref = self.module.declare_func_in_func(func_id, self.b.func);
                let call = self.b.ins().call(func_ref, &[vals[i], vals[i + 1]]);
                let ordering = self.b.inst_results(call)[0];
                self.emit_trap_check()?;
                let (condition, discriminant) = match op {
                    BinOp::Lt => (IntCC::Equal, 0),
                    BinOp::Le => (IntCC::NotEqual, 2),
                    BinOp::Gt => (IntCC::Equal, 2),
                    BinOp::Ge => (IntCC::NotEqual, 0),
                    _ => return Err("jit compare chain operator unsupported".to_string()),
                };
                let discriminant = self.b.ins().iconst(types::I64, discriminant);
                self.bool_from_icmp(condition, ordering, discriminant)
            } else {
                let lhs_ty = self.expr_arith_type(&operands[i]);
                let rhs_ty = self.expr_arith_type(&operands[i + 1]);
                if lhs_ty != rhs_ty {
                    return Err("jit compare chain mixed operand types".to_string());
                }
                match (&lhs_ty, op) {
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
                    (Type::IntN { signed, .. }, op) => {
                        let cc = match (signed, op) {
                            (true, BinOp::Lt) => IntCC::SignedLessThan,
                            (true, BinOp::Gt) => IntCC::SignedGreaterThan,
                            (true, BinOp::Le) => IntCC::SignedLessThanOrEqual,
                            (true, BinOp::Ge) => IntCC::SignedGreaterThanOrEqual,
                            (false, BinOp::Lt) => IntCC::UnsignedLessThan,
                            (false, BinOp::Gt) => IntCC::UnsignedGreaterThan,
                            (false, BinOp::Le) => IntCC::UnsignedLessThanOrEqual,
                            (false, BinOp::Ge) => IntCC::UnsignedGreaterThanOrEqual,
                            _ => return Err("jit compare chain operator unsupported".to_string()),
                        };
                        self.bool_from_icmp(cc, vals[i], vals[i + 1])
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
                }
            };
            if i + 1 == ops.len() {
                self.b.ins().jump(merge, &[cmp]);
            } else {
                let next = self.b.create_block();
                let zero = self.b.ins().iconst(types::I8, 0);
                let failed = self.b.ins().icmp(IntCC::Equal, cmp, zero);
                self.b.ins().brif(failed, merge, &[zero], next, &[]);
                self.b.switch_to_block(next);
                self.b.seal_block(next);
            }
        }
        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_incdec(
        &mut self,
        op: IncDecOp,
        place: &TPlace,
        postfix: bool,
        ty: &Type,
    ) -> Result<Value, String> {
        if !matches!(ty, Type::Int) {
            return Err("jit increment/decrement unsupported type".to_string());
        }
        let local = place
            .as_local()
            .ok_or("jit increment/decrement non-local place")?;
        let key = Self::local_key(local);
        let var = self
            .vars
            .get(&key)
            .copied()
            .ok_or_else(|| format!("jit increment/decrement unknown local `{}`", local.name))?;
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
        matches!(expr.kind, TExprKind::Field { .. }).then(|| expr.ty.clone())
    }

    fn expr_arith_type(&self, expr: &TExpr) -> Type {
        if let Some(t) = self.expr_field_type(expr) {
            return self.erase_distinct_ty(&t);
        }
        // TIR `handle_method_return_ty` falls through to Unit for several Rng
        // draws; recover the real scalar from the op so compares type-check.
        if let Some(ty) = self.expr_arith_type_from_op(expr) {
            return ty;
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
        self.erase_distinct_ty(&expr.ty)
    }

    fn expr_arith_type_from_op(&self, expr: &TExpr) -> Option<Type> {
        if let Some(ty) = Self::recover_core_return_ty(expr) {
            return Some(ty);
        }
        match &expr.kind {
            TExprKind::CoreCall { module, method, .. } if module == "core.random" => {
                match method.as_str() {
                    "float_range" | "normal" | "exponential" => Some(Type::Float),
                    "bool" => Some(Type::Bool),
                    _ => None,
                }
            }
            TExprKind::HandleMethod { op, .. } => match op {
                THandleOp::RngFloat
                | THandleOp::RngFloatRange
                | THandleOp::RngNormal
                | THandleOp::RngExponential => Some(Type::Float),
                THandleOp::RngInt
                | THandleOp::ClockNow
                | THandleOp::ClockTick
                | THandleOp::ClockAdvance
                | THandleOp::ClockWait => Some(Type::Int),
                THandleOp::RngBool | THandleOp::RngBoolP => Some(Type::Bool),
                _ => None,
            },
            _ => None,
        }
    }

    /// Print dispatch type — recover Bool/Float/Int when TIR left `Unit` on
    /// compare results or Rng draws (same hole as `expr_arith_type`).
    fn print_result_ty(&self, expr: &TExpr) -> Type {
        if matches!(
            &expr.ty,
            Type::Apply { name, args }
                if name == "View"
                    && args.len() == 1
                    && (matches!(&args[0], Type::String)
                        || matches!(&args[0], Type::Named(name) if name == "str"))
        ) {
            return Type::String;
        }
        if let TExprKind::Local(local) = &expr.kind {
            let key = TIR::local_place(&local.name);
            if let Some(ty) = self.var_tys.get(&key) {
                if local.deref {
                    if let Type::Apply { name, args } = ty {
                        if name == "ViewMut" && args.len() == 1 {
                            return self.erase_distinct_ty(&args[0]);
                        }
                    }
                }
                if !matches!(ty, Type::Named(n) if n == "Unit" || n == "Void") {
                    return self.erase_distinct_ty(ty);
                }
            }
        }
        if let TExprKind::Binary { op, lhs, rhs, .. } = &expr.kind {
            if matches!(
                op,
                BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Gt
                    | BinOp::Le
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or
            ) {
                return Type::Bool;
            }
            let lt = self.expr_arith_type(lhs);
            let rt = self.expr_arith_type(rhs);
            if lt == Type::Float || rt == Type::Float {
                return Type::Float;
            }
            if lt == Type::Int && rt == Type::Int {
                return Type::Int;
            }
        }
        if let TExprKind::OrFallback { value, fallback, .. } = &expr.kind {
            if let Type::Option(inner) = &value.ty {
                return self.erase_distinct_ty(inner);
            }
            if let TOrFallback::Value(fb) = fallback {
                let fb_ty = self.print_result_ty(fb);
                if !matches!(&fb_ty, Type::Named(n) if n == "Unit" || n == "Void") {
                    return fb_ty;
                }
            }
            if let Some(ok) = Self::result_ok_ty_recover(value) {
                return ok;
            }
            // weighted_pick Option<String> when TIR erased the Option wrapper.
            if let TExprKind::CoreCall { module, method, .. } = &value.kind {
                if module == "core.random" && method == "weighted_pick" {
                    return Type::String;
                }
            }
            if matches!(
                &value.kind,
                TExprKind::HandleMethod {
                    op: THandleOp::RngWeightedPick,
                    ..
                }
            ) {
                return Type::String;
            }
        }
        if let Some(ty) = self.expr_arith_type_from_op(expr) {
            return ty;
        }
        // CORE struct fields (ProcessResult.output, …) — TIR may say Int.
        if let TExprKind::Field { recv, field, .. } = &expr.kind {
            if let Some(name) = record_type_key(&recv.ty) {
                if matches!(
                    self.meta.struct_field_ty(&name, field),
                    Some(Type::Apply { name, args })
                        if name == "View"
                            && args.len() == 1
                            && (matches!(&args[0], Type::String)
                                || matches!(&args[0], Type::Named(name) if name == "str"))
                ) {
                    return Type::String;
                }
                if let Some(ty) = core_struct_field_type(&name, field) {
                    return self.erase_distinct_ty(&ty);
                }
            }
        }
        // String transforms keep Type::String even when the receiver's field
        // type was erased to Int above.
        if let TExprKind::BuiltinMethod { op, .. } = &expr.kind {
            if matches!(
                op,
                TBuiltinOp::Trim
                    | TBuiltinOp::TrimView
                    | TBuiltinOp::AfterView
                    | TBuiltinOp::BeforeView
                    | TBuiltinOp::ToUpper
                    | TBuiltinOp::ToLower
            ) {
                return Type::String;
            }
        }
        self.erase_distinct_ty(&expr.ty)
    }

    /// `TExprKind::Binary` (`op != And/Or`, those short-circuit separately in
    /// `lower_short_circuit`). Keyed on `(Type, BinOp)` — ancillary types, not
    /// a `TIR` enum — so the trailing `_` is a real unsupported-combination
    /// gap (e.g. no bitwise-on-float), not a hidden `TIR` variant.
    fn lower_intn_values(
        &mut self,
        op: BinOp,
        mode: i64,
        left: Value,
        right: Value,
        signed: bool,
        bits: u8,
        right_signed: bool,
    ) -> Result<Value, String> {
        let op = match op {
            BinOp::Add => INTN_OP_ADD,
            BinOp::Sub => INTN_OP_SUB,
            BinOp::Mul => INTN_OP_MUL,
            BinOp::Div => INTN_OP_DIV,
            BinOp::Rem => INTN_OP_REM,
            BinOp::BitAnd => INTN_OP_BIT_AND,
            BinOp::BitOr => INTN_OP_BIT_OR,
            BinOp::BitXor => INTN_OP_BIT_XOR,
            BinOp::Shl => INTN_OP_SHL,
            BinOp::Shr => INTN_OP_SHR,
            _ => return Err("jit fixed-width integer operation unsupported".to_string()),
        };
        let args = [
            left,
            right,
            self.b.ins().iconst(types::I64, op),
            self.b.ins().iconst(types::I64, mode),
            self.b.ins().iconst(types::I64, i64::from(signed)),
            self.b.ins().iconst(types::I64, i64::from(bits)),
            self.b
                .ins()
                .iconst(types::I64, i64::from(right_signed)),
        ];
        let host = self
            .module
            .declare_func_in_func(self.host.intn_binop, self.b.func);
        let call = self.b.ins().call(host, &args);
        let result = self.b.inst_results(call)[0];
        if mode != INTN_MODE_CHECKED {
            self.emit_trap_check()?;
        }
        Ok(result)
    }

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
        let lhs_ty = self.expr_arith_type(lhs);
        let _rhs_ty = self.expr_arith_type(rhs);
        if let Type::IntN { signed, bits } = lhs_ty {
            let right_signed = !matches!(&rhs.ty, Type::IntN { signed: false, .. });
            let comparison = match op {
                BinOp::Eq => Some(IntCC::Equal),
                BinOp::Ne => Some(IntCC::NotEqual),
                BinOp::Lt if signed => Some(IntCC::SignedLessThan),
                BinOp::Gt if signed => Some(IntCC::SignedGreaterThan),
                BinOp::Le if signed => Some(IntCC::SignedLessThanOrEqual),
                BinOp::Ge if signed => Some(IntCC::SignedGreaterThanOrEqual),
                BinOp::Lt => Some(IntCC::UnsignedLessThan),
                BinOp::Gt => Some(IntCC::UnsignedGreaterThan),
                BinOp::Le => Some(IntCC::UnsignedLessThanOrEqual),
                BinOp::Ge => Some(IntCC::UnsignedGreaterThanOrEqual),
                _ => None,
            };
            if let Some(cc) = comparison {
                return Ok(self.bool_from_icmp(cc, l, r));
            }
            return self.lower_intn_values(
                op,
                INTN_MODE_TRAP,
                l,
                r,
                signed,
                bits,
                right_signed,
            );
        }
        if overflow {
            let host_id = match op {
                BinOp::Add => self.host.add_i64,
                BinOp::Sub => self.host.sub_i64,
                BinOp::Mul => self.host.mul_i64,
                BinOp::Div => self.host.div_i64,
                BinOp::Rem => self.host.rem_i64,
                _ => return Err("jit overflow op unsupported".to_string()),
            };
            let line_const = self.b.ins().iconst(types::I32, line as i64);
            let host_ref = self.module.declare_func_in_func(host_id, self.b.func);
            let call = self.b.ins().call(host_ref, &[l, r, line_const]);
            let result = self.b.inst_results(call)[0];
            self.emit_trap_check()?;
            return Ok(result);
        }
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
            (Type::Named(name), BinOp::Eq | BinOp::Ne) if name == "BigInt" => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.num.bigint_eq, self.b.func);
                let call = self.b.ins().call(host_ref, &[l, r]);
                let eq = self.b.inst_results(call)[0];
                if matches!(op, BinOp::Eq) {
                    eq
                } else {
                    let one = self.b.ins().iconst(types::I8, 1);
                    self.b.ins().isub(one, eq)
                }
            }
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
            (Type::List(_) | Type::FixedList { .. }, BinOp::Eq) => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_eq, self.b.func);
                let call = self.b.ins().call(host_ref, &[l, r]);
                self.b.inst_results(call)[0]
            }
            (Type::List(_) | Type::FixedList { .. }, BinOp::Ne) => {
                let host_ref = self
                    .module
                    .declare_func_in_func(self.host.coll.list_eq, self.b.func);
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
        if let Type::IntN { signed, .. } = &inner.ty {
            let value = self.lower_expr(inner)?;
            let signed = self
                .b
                .ins()
                .iconst(types::I64, i64::from(*signed));
            let show = self
                .module
                .declare_func_in_func(self.host.intn_to_string, self.b.func);
            let call = self.b.ins().call(show, &[value, signed]);
            let text = self.b.inst_results(call)[0];
            let print = self
                .module
                .declare_func_in_func(self.host.print_str, self.b.func);
            self.b.ins().call(print, &[text]);
            return Ok(());
        }
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
                let print_ty = self.print_result_ty(inner);
                // Some method chains type `list.join(sep)` as Unit in TIR even though
                // the lowered value is a String handle (seen on Url.path_segments().join).
                if matches!(&print_ty, Type::Named(n) if n == "Unit" || n == "Void") {
                    if matches!(
                        &inner.kind,
                        TExprKind::BuiltinMethod {
                            op: TBuiltinOp::JoinSep,
                            ..
                        }
                    ) {
                        let print = self
                            .module
                            .declare_func_in_func(self.host.print_str, self.b.func);
                        self.b.ins().call(print, &[val]);
                        return Ok(());
                    }
                    return Ok(());
                }
                // List / materialized Iter — same jet_show `[a, b, c]` AOT uses.
                if let Some(elem) = jit_list_iter_elem_type(&print_ty) {
                    let kind = match elem {
                        Type::String => 1,
                        Type::IntN { signed: true, .. } => 2,
                        Type::IntN { signed: false, .. } => 3,
                        _ => 0,
                    };
                    let flag = self
                        .b
                        .ins()
                        .iconst(types::I64, kind);
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.print_list, self.b.func);
                    self.b.ins().call(host_ref, &[val, flag]);
                    return Ok(());
                }
                // `T?` — IntN uses the result arena; legacy payloads use `value + 1`.
                if let Type::Option(inner_ty) = &print_ty {
                    let mut kind = match inner_ty.as_ref() {
                        Type::Int => 0i64,
                        Type::String => 1,
                        Type::Float => 2,
                        Type::IntN { signed: true, .. } => 3,
                        Type::IntN { signed: false, .. } => 4,
                        _ => return Err("jit print type unsupported".to_string()),
                    };
                    if Self::uses_result_option_abi(inner) {
                        kind += 10;
                    }
                    let flag = self.b.ins().iconst(types::I64, kind);
                    let host_ref = self
                        .module
                        .declare_func_in_func(self.host.coll.print_opt, self.b.func);
                    self.b.ins().call(host_ref, &[val, flag]);
                    return Ok(());
                }
                if matches!(
                    &print_ty,
                    Type::Apply { name, args }
                        if name == "Measurement" && args.as_slice() == [Type::Float]
                ) {
                    let show = self
                        .module
                        .declare_func_in_func(self.host.measurement_show, self.b.func);
                    let call = self.b.ins().call(show, &[val]);
                    let text = self.b.inst_results(call)[0];
                    let print = self
                        .module
                        .declare_func_in_func(self.host.print_str, self.b.func);
                    self.b.ins().call(print, &[text]);
                    return Ok(());
                }
                // Packed enums — JetShow `user_Variant(…)` matching AOT `{:?}`.
                if let Type::Named(name) = &print_ty {
                    if self.meta.is_enum(name) && self.meta.enum_packed_showable(name) {
                        // Leak the type name once: heap string handles die on
                        // resident reset between compile and run.
                        let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
                        let ptr = self.b.ins().iconst(types::I64, leaked.as_ptr() as i64);
                        let len = self.b.ins().iconst(types::I64, leaked.len() as i64);
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.coll.print_enum, self.b.func);
                        self.b.ins().call(host_ref, &[val, ptr, len]);
                        return Ok(());
                    }
                }
                let host_id = match &print_ty {
                    Type::Int | Type::IntN { .. } => self.host.print_i64,
                    Type::String => self.host.print_str,
                    Type::Float => self.host.print_f64,
                    Type::Bool => self.host.print_bool,
                    Type::Char => self.host.print_char,
                    Type::Named(n) if n == "DataTree" || n == "Json" => {
                        // JetShow DataTree/Json via shared encoding host (same as AOT).
                        let host_ref = self
                            .module
                            .declare_func_in_func(self.host.encoding.json_to_string, self.b.func);
                        let call = self.b.ins().call(host_ref, &[val]);
                        let s = self.b.inst_results(call)[0];
                        let print_ref = self
                            .module
                            .declare_func_in_func(self.host.print_str, self.b.func);
                        self.b.ins().call(print_ref, &[s]);
                        return Ok(());
                    }
                    _ => {
                        return Err(format!("jit print type unsupported: {print_ty:?}"));
                    }
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

    fn lower_iter_adapter(
        &mut self,
        host: FuncId,
        recv_val: Value,
        n_arg: Option<&TExpr>,
        const_flag: Option<i64>,
    ) -> Result<Value, String> {
        let second = if let Some(n) = n_arg {
            self.lower_expr(n)?
        } else {
            self.b.ins().iconst(types::I64, const_flag.unwrap_or(0))
        };
        let host_ref = self.module.declare_func_in_func(host, self.b.func);
        let call = self.b.ins().call(host_ref, &[recv_val, second]);
        Ok(self.b.inst_results(call)[0])
    }

    /// Native Iter/list closure adapters — lambda bodies inlined in Cranelift.
    fn lower_closure_method(
        &mut self,
        recv: &TExpr,
        op: &TClosureOp,
        args: &[TExpr],
    ) -> Result<Value, String> {
        match op {
            TClosureOp::Map | TClosureOp::MapMut | TClosureOp::OptionMap | TClosureOp::ViewMap => {
                if let Type::Option(inner) = &recv.ty {
                    return self.lower_option_map(recv, args, inner);
                }
                self.lower_iter_map_filter(recv, args, false)
            }
            // D-PARCAPTURE1: order-preserving parallel adapters — serial Cranelift
            // inlining matches AOT results for the covered examples.
            TClosureOp::ParaMap => self.lower_iter_map_filter(recv, args, false),
            TClosureOp::Filter => self.lower_iter_map_filter(recv, args, true),
            TClosureOp::ParaFilter => self.lower_iter_map_filter(recv, args, true),
            TClosureOp::ParaPartition { .. } => self.lower_para_partition(recv, args),
            TClosureOp::ParaFold => self.lower_para_fold(recv, args),
            TClosureOp::Each | TClosureOp::EachMut | TClosureOp::EachRef => {
                self.lower_iter_each(recv, args)
            }
            TClosureOp::FilterMap => self.lower_iter_filter_map(recv, args),
            TClosureOp::SortBy => self.lower_iter_sort_by(recv, args),
            TClosureOp::TakeWhile => self.lower_iter_take_skip_while(recv, args, false),
            TClosureOp::SkipWhile => self.lower_iter_take_skip_while(recv, args, true),
            TClosureOp::Fold | TClosureOp::Reduce | TClosureOp::ViewFold => {
                self.lower_iter_fold(recv, args)
            }
            TClosureOp::Position => self.lower_iter_position(recv, args),
            TClosureOp::MinBy => self.lower_iter_min_max_by(recv, args, false),
            TClosureOp::MaxBy => self.lower_iter_min_max_by(recv, args, true),
            TClosureOp::FlatMap => self.lower_iter_flat_map(recv, args),
            TClosureOp::CountBy => self.lower_iter_count_by(recv, args),
            _ => Err("jit closure method unsupported".to_string()),
        }
    }

    fn lower_iter_count_by(
        &mut self,
        recv: &TExpr,
        args: &[TExpr],
    ) -> Result<Value, String> {
        let elem_ty = jit_closure_elem_type(&recv.ty)
            .ok_or_else(|| "jit count_by receiver unsupported".to_string())?;
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        if !matches!((&elem_ty, &body_expr.ty), (Type::String, Type::String)) {
            return Err("jit count_by types unsupported".to_string());
        }
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);
        let map_new = self
            .module
            .declare_func_in_func(self.host.coll.map_new, self.b.func);
        let map_call = self.b.ins().call(map_new, &[]);
        let map = self.b.inst_results(map_call)[0];
        let map_var = self.fresh_var(types::I64);
        self.b.def_var(map_var, map);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let key = self.with_bound_local(&param_place, elem_ty, elem, |this| {
            this.lower_expr(body_expr)
        })?;
        let increment = self
            .module
            .declare_func_in_func(self.host.coll.map_increment, self.b.func);
        let map = self.b.use_var(map_var);
        self.b.ins().call(increment, &[map, key]);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(map_var))
    }

    fn closure_unary_lambda<'a>(
        &self,
        args: &'a [TExpr],
    ) -> Result<(String, &'a TExpr), String> {
        let lam_expr = args
            .first()
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        let TExprKind::Lambda(lam) = &lam_expr.kind else {
            return Err("jit closure method unsupported".to_string());
        };
        if !lam.prep.is_empty() || lam.source_params.len() != 1 {
            return Err("jit closure method unsupported".to_string());
        }
        let TLambdaBody::Expr(body) = &lam.executable else {
            return Err("jit closure method unsupported".to_string());
        };
        Ok((TIR::local_place(&lam.source_params[0]), body))
    }

    fn with_bound_local<R>(
        &mut self,
        place: &str,
        ty: Type,
        value: Value,
        body: impl FnOnce(&mut Self) -> Result<R, String>,
    ) -> Result<R, String> {
        let old_var = self.vars.remove(place);
        let old_ty = self.var_tys.remove(place);
        let clif = match &ty {
            Type::Float => types::F64,
            Type::Bool => types::I8,
            Type::Char => types::I32,
            _ => types::I64,
        };
        let var = self.fresh_var(clif);
        self.b.def_var(var, value);
        self.vars.insert(place.to_string(), var);
        self.var_tys.insert(place.to_string(), ty);
        let result = body(self)?;
        self.vars.remove(place);
        self.var_tys.remove(place);
        if let Some(v) = old_var {
            self.vars.insert(place.to_string(), v);
        }
        if let Some(t) = old_ty {
            self.var_tys.insert(place.to_string(), t);
        }
        Ok(result)
    }

    fn lower_iter_map_filter(
        &mut self,
        recv: &TExpr,
        args: &[TExpr],
        is_filter: bool,
    ) -> Result<Value, String> {
        let elem_ty = jit_closure_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(
            &elem_ty,
            Type::Int | Type::String | Type::Named(_)
        ) {
            return Err("jit closure method unsupported".to_string());
        }
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);

        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_val = self.b.inst_results(out_call)[0];
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, out_val);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;

        let pred_or_mapped =
            self.with_bound_local(&param_place, elem_ty.clone(), elem, |this| {
                this.lower_expr(body_expr)
            })?;

        if is_filter {
            let keep_block = self.b.create_block();
            let zero_b = self.b.ins().iconst(types::I8, 0);
            let keep = self.b.ins().icmp(IntCC::NotEqual, pred_or_mapped, zero_b);
            self.b.ins().brif(keep, keep_block, &[], step, &[]);
            self.b.switch_to_block(keep_block);
            self.b.seal_block(keep_block);
            let out = self.b.use_var(out_var);
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[out, elem]);
            self.b.ins().jump(step, &[]);
        } else {
            let out = self.b.use_var(out_var);
            let mapped_ty = self.erase_distinct_ty(&body_expr.ty);
            let (push_id, push_val) = if matches!(mapped_ty, Type::Float | Type::Float32) {
                (self.host.coll.list_push_f64, pred_or_mapped)
            } else {
                (self.host.coll.list_push, pred_or_mapped)
            };
            let push_ref = self.module.declare_func_in_func(push_id, self.b.func);
            self.b.ins().call(push_ref, &[out, push_val]);
            self.b.ins().jump(step, &[]);
        }

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(out_var))
    }

    fn lower_iter_each(&mut self, recv: &TExpr, args: &[TExpr]) -> Result<Value, String> {
        let elem_ty = jit_closure_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(
            &elem_ty,
            Type::Int | Type::String | Type::Named(_)
        ) {
            return Err("jit closure method unsupported".to_string());
        }
        let lam_expr = args
            .first()
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        let TExprKind::Lambda(lam) = &lam_expr.kind else {
            return Err("jit closure method unsupported".to_string());
        };
        if !lam.prep.is_empty() || lam.source_params.len() != 1 {
            return Err("jit closure method unsupported".to_string());
        }
        let param_place = TIR::local_place(&lam.source_params[0]);
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        self.with_bound_local(&param_place, elem_ty, elem, |this| {
            match &lam.executable {
                TLambdaBody::Expr(body) => this.lower_expr(body).map(|_| ()),
                TLambdaBody::Block(stmts) => this.lower_stmts(stmts),
            }
        })?;
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.ins().iconst(types::I8, 0))
    }

    /// `if k == .Char(c)` / `.F(n)` / `.Ctrl(c)` on user enums.
    fn lower_enum_if_let(
        &mut self,
        pattern: &TPattern,
        subj: &TExpr,
        then_body: &[TStmt],
        else_body: Option<&[TStmt]>,
    ) -> Result<(), String> {
        let Pattern::Variant {
            variant, bindings, ..
        } = &pattern.pattern
        else {
            return Err("jit if-let pattern unsupported".to_string());
        };
        let enum_name = pattern
            .enum_type
            .as_deref()
            .or_else(|| user_type_name(&subj.ty))
            .ok_or("jit enum if-let missing type")?;
        let f64_heap = self
            .meta
            .enum_variant_payload_types(enum_name, variant)
            .and_then(|tys| tys.first().cloned())
            .is_some_and(|ty| matches!(ty, Type::Float | Type::Float32));
        let subject = self.lower_expr(subj)?;
        let then_block = self.b.create_block();
        let else_block = self.b.create_block();
        let merge_block = self.b.create_block();
        let eq = self.lower_pattern_condition(
            subject,
            &pattern.pattern,
            Some(enum_name),
            f64_heap,
        )?;
        self.b
            .ins()
            .brif(eq, then_block, &[], else_block, &[]);

        self.b.switch_to_block(then_block);
        self.b.seal_block(then_block);
        let bound = if let Some(name) = bindings.first().and_then(PatSlot::as_bind) {
            let payload_ty = self
                .meta
                .enum_variant_payload_types(enum_name, variant)
                .and_then(|tys| tys.first())
                .cloned()
                .unwrap_or(Type::Int);
            let payload = if f64_heap {
                self.unpack_enum_heap_payload(subject, &payload_ty)?
            } else {
                self.unpack_enum_scalar(subject, &payload_ty)?
            };
            let payload_clif = self.meta.clif_ty(&payload_ty).unwrap_or(types::I64);
            let var = self.fresh_var(payload_clif);
            self.b.def_var(var, payload);
            let key = TIR::local_place(name);
            let old_var = self.vars.insert(key.clone(), var);
            let old_ty = self.var_tys.insert(key.clone(), payload_ty);
            Some((key, old_var, old_ty))
        } else {
            None
        };
        self.lower_stmts_scoped(then_body)?;
        if let Some((key, old_var, old_ty)) = bound {
            match old_var {
                Some(var) => {
                    self.vars.insert(key.clone(), var);
                }
                None => {
                    self.vars.remove(&key);
                }
            }
            match old_ty {
                Some(ty) => {
                    self.var_tys.insert(key, ty);
                }
                None => {
                    self.var_tys.remove(&key);
                }
            }
        }
        let then_dead = self.dead;
        if !then_dead {
            self.b.ins().jump(merge_block, &[]);
        }

        self.b.switch_to_block(else_block);
        self.b.seal_block(else_block);
        self.dead = false;
        if let Some(body) = else_body {
            self.lower_stmts_scoped(body)?;
        }
        let else_dead = self.dead;
        if !else_dead {
            self.b.ins().jump(merge_block, &[]);
        }

        if then_dead && else_dead {
            self.dead = true;
        } else {
            self.b.switch_to_block(merge_block);
            self.b.seal_block(merge_block);
            self.dead = false;
        }
        Ok(())
    }

    /// `if x == .Ok(v)` / `.Err(e)` on Result, or `if opt == Val(v)` on Option.
    fn lower_result_if_let(
        &mut self,
        pattern: &TPattern,
        subj: &TExpr,
        then_body: &[TStmt],
        else_body: Option<&[TStmt]>,
    ) -> Result<(), String> {
        if matches!(&pattern.pattern, Pattern::Present { .. }) {
            return self.lower_option_present_if_let(pattern, subj, then_body, else_body);
        }
        let (want_ok, binding, payload_ty) = match &pattern.pattern {
            Pattern::Ok { binding, .. } => {
                let Type::Result { ok, .. } = &subj.ty else {
                    return Err("jit if-let Ok on non-Result".to_string());
                };
                (true, binding.clone(), ok.as_ref().clone())
            }
            Pattern::Err { binding, .. } => {
                let Type::Result { err, .. } = &subj.ty else {
                    return Err("jit if-let Err on non-Result".to_string());
                };
                (false, binding.clone(), err.as_ref().clone())
            }
            _ => return Err("jit if-let pattern unsupported".to_string()),
        };
        let handle = self.lower_expr(subj)?;
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[handle]);
        let is_ok = self.b.inst_results(status_call)[0];
        let zero_b = self.b.ins().iconst(types::I8, 0);
        let cond = if want_ok {
            self.b.ins().icmp(IntCC::NotEqual, is_ok, zero_b)
        } else {
            self.b.ins().icmp(IntCC::Equal, is_ok, zero_b)
        };

        let then_block = self.b.create_block();
        let else_block = self.b.create_block();
        let merge_block = self.b.create_block();
        self.b
            .ins()
            .brif(cond, then_block, &[], else_block, &[]);

        self.b.switch_to_block(then_block);
        self.b.seal_block(then_block);
        let payload = self.result_payload(handle, &payload_ty)?;
        let place = TIR::local_place(&binding);
        let clif = self.b.func.dfg.value_type(payload);
        let old_var = self.vars.remove(&place);
        let old_ty = self.var_tys.remove(&place);
        let var = self.fresh_var(clif);
        self.b.def_var(var, payload);
        self.vars.insert(place.clone(), var);
        self.var_tys.insert(place.clone(), payload_ty);
        self.lower_stmts_scoped(then_body)?;
        self.vars.remove(&place);
        self.var_tys.remove(&place);
        if let Some(v) = old_var {
            self.vars.insert(place.clone(), v);
        }
        if let Some(t) = old_ty {
            self.var_tys.insert(place, t);
        }
        let then_reaches = !self.dead;
        if then_reaches {
            self.b.ins().jump(merge_block, &[]);
        }

        self.b.switch_to_block(else_block);
        self.b.seal_block(else_block);
        self.dead = false;
        if let Some(body) = else_body {
            self.lower_stmts(body)?;
        }
        let else_reaches = !self.dead;
        if else_reaches {
            self.b.ins().jump(merge_block, &[]);
        }

        if then_reaches || else_reaches {
            self.b.switch_to_block(merge_block);
            self.b.seal_block(merge_block);
            self.dead = false;
        } else {
            self.dead = true;
        }
        Ok(())
    }

    /// `if opt == Val(x)` — Option ABI is 0 = None, value+1 = Some.
    fn lower_option_present_if_let(
        &mut self,
        pattern: &TPattern,
        subj: &TExpr,
        then_body: &[TStmt],
        else_body: Option<&[TStmt]>,
    ) -> Result<(), String> {
        let Pattern::Present { binding, .. } = &pattern.pattern else {
            return Err("jit option if-let needs Present".to_string());
        };
        let Type::Option(inner) = &subj.ty else {
            return Err("jit if-let Val on non-Option".to_string());
        };
        let inner_ty = inner.as_ref().clone();
        let packed = self.lower_expr(subj)?;
        let zero = self.b.ins().iconst(types::I64, 0);
        let is_some = self.b.ins().icmp(IntCC::NotEqual, packed, zero);

        let then_block = self.b.create_block();
        let else_block = self.b.create_block();
        let merge_block = self.b.create_block();
        self.b
            .ins()
            .brif(is_some, then_block, &[], else_block, &[]);

        self.b.switch_to_block(then_block);
        self.b.seal_block(then_block);
        let payload = self.unpack_option_payload(packed, &inner_ty)?;
        let place = TIR::local_place(binding);
        let clif = clif_ty(&inner_ty).unwrap_or(types::I64);
        let old_var = self.vars.remove(&place);
        let old_ty = self.var_tys.remove(&place);
        let var = self.fresh_var(clif);
        self.b.def_var(var, payload);
        self.vars.insert(place.clone(), var);
        self.var_tys.insert(place.clone(), inner_ty);
        self.lower_stmts_scoped(then_body)?;
        self.vars.remove(&place);
        self.var_tys.remove(&place);
        if let Some(v) = old_var {
            self.vars.insert(place.clone(), v);
        }
        if let Some(t) = old_ty {
            self.var_tys.insert(place, t);
        }
        let then_reaches = !self.dead;
        if then_reaches {
            self.b.ins().jump(merge_block, &[]);
        }

        self.b.switch_to_block(else_block);
        self.b.seal_block(else_block);
        self.dead = false;
        if let Some(body) = else_body {
            self.lower_stmts(body)?;
        }
        let else_reaches = !self.dead;
        if else_reaches {
            self.b.ins().jump(merge_block, &[]);
        }

        if then_reaches || else_reaches {
            self.b.switch_to_block(merge_block);
            self.b.seal_block(merge_block);
            self.dead = false;
        } else {
            self.dead = true;
        }
        Ok(())
    }

    /// `if data == .Object(entries)` / `.Int(n)` on DataTree heap records.
    fn lower_datatree_if_let(
        &mut self,
        pattern: &TPattern,
        subj: &TExpr,
        then_body: &[TStmt],
        else_body: Option<&[TStmt]>,
    ) -> Result<(), String> {
        let Pattern::Variant {
            variant, bindings, ..
        } = &pattern.pattern
        else {
            return Err("jit DataTree if-let needs a variant pattern".to_string());
        };
        let want_disc = Self::datatree_variant_disc(variant)
            .ok_or_else(|| format!("jit DataTree variant `{variant}`"))?;
        let payload_ty = self
            .meta
            .enum_variant_payload_types("DataTree", variant)
            .and_then(|tys| tys.first())
            .cloned();

        let handle = self.lower_expr(subj)?;
        let zero = self.b.ins().iconst(types::I64, 0);
        let get_i = self
            .module
            .declare_func_in_func(self.host.struct_get_i64, self.b.func);
        let disc_call = self.b.ins().call(get_i, &[handle, zero]);
        let actual_disc = self.b.inst_results(disc_call)[0];
        let want = self.b.ins().iconst(types::I64, want_disc);
        let cond = self.bool_from_icmp(IntCC::Equal, actual_disc, want);

        let then_block = self.b.create_block();
        let else_block = self.b.create_block();
        let merge_block = self.b.create_block();
        self.b
            .ins()
            .brif(cond, then_block, &[], else_block, &[]);

        self.b.switch_to_block(then_block);
        self.b.seal_block(then_block);

        let mut bound_place: Option<String> = None;
        let mut old_var = None;
        let mut old_ty = None;
        match &pattern.position {
            TPatternPosition::DataEntries { temp } => {
                // Object → bind temp to map payload; body prefix converts via DataEntriesToMap.
                let payload = self.unpack_enum_heap_payload(
                    handle,
                    &payload_ty.clone().unwrap_or(Type::Map {
                        key: Box::new(Type::String),
                        key_span: None,
                        value: Box::new(Type::Named("DataTree".into())),
                    }),
                )?;
                let place = temp.clone();
                old_var = self.vars.remove(&place);
                old_ty = self.var_tys.remove(&place);
                let var = self.fresh_var(types::I64);
                self.b.def_var(var, payload);
                self.vars.insert(place.clone(), var);
                self.var_tys.insert(
                    place.clone(),
                    payload_ty.clone().unwrap_or(Type::Map {
                        key: Box::new(Type::String),
                        key_span: None,
                        value: Box::new(Type::Named("DataTree".into())),
                    }),
                );
                bound_place = Some(place);
            }
            _ => {
                if let Some(PatSlot::Bind { name, .. }) = bindings.first() {
                    if name != "_" {
                        let pty = payload_ty.clone().unwrap_or(Type::Int);
                        let payload = self.unpack_enum_heap_payload(handle, &pty)?;
                        let place = TIR::local_place(name);
                        old_var = self.vars.remove(&place);
                        old_ty = self.var_tys.remove(&place);
                        let clif = self.meta.clif_ty(&pty).unwrap_or(types::I64);
                        let var = self.fresh_var(clif);
                        self.b.def_var(var, payload);
                        self.vars.insert(place.clone(), var);
                        self.var_tys.insert(place.clone(), pty);
                        bound_place = Some(place);
                    }
                }
            }
        }

        self.lower_stmts_scoped(then_body)?;
        if let Some(place) = bound_place {
            self.vars.remove(&place);
            self.var_tys.remove(&place);
            if let Some(v) = old_var {
                self.vars.insert(place.clone(), v);
            }
            if let Some(t) = old_ty {
                self.var_tys.insert(place, t);
            }
        }
        let then_reaches_merge = !self.dead;
        if then_reaches_merge {
            self.b.ins().jump(merge_block, &[]);
        }

        self.b.switch_to_block(else_block);
        self.b.seal_block(else_block);
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
            self.dead = true;
        }
        Ok(())
    }

    fn lower_iter_filter_map(
        &mut self,
        recv: &TExpr,
        args: &[TExpr],
    ) -> Result<Value, String> {
        let elem_ty = jit_list_iter_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(elem_ty, Type::String) {
            return Err("jit closure method unsupported".to_string());
        }
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        let Type::Result { ok, .. } = &body_expr.ty else {
            return Err("jit filter_map body must return Result".to_string());
        };
        let ok_ty = ok.as_ref().clone();
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);

        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_val = self.b.inst_results(out_call)[0];
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, out_val);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;

        let res = self.with_bound_local(&param_place, elem_ty, elem, |this| {
            this.lower_expr(body_expr)
        })?;
        let keep = self.b.create_block();
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[res]);
        let is_ok = self.b.inst_results(status_call)[0];
        let zero_b = self.b.ins().iconst(types::I8, 0);
        let ok = self.b.ins().icmp(IntCC::NotEqual, is_ok, zero_b);
        self.b.ins().brif(ok, keep, &[], step, &[]);

        self.b.switch_to_block(keep);
        self.b.seal_block(keep);
        let payload = self.result_payload(res, &ok_ty)?;
        let out = self.b.use_var(out_var);
        let push_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);
        self.b.ins().call(push_ref, &[out, payload]);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(out_var))
    }

    fn lower_iter_sort_by(
        &mut self,
        recv: &TExpr,
        args: &[TExpr],
    ) -> Result<Value, String> {
        let elem_ty = jit_closure_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(elem_ty, Type::String | Type::Named(_)) {
            return Err("jit closure method unsupported".to_string());
        }
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        if !matches!(body_expr.ty, Type::Int) {
            return Err("jit sort_by key must be Int".to_string());
        }
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);

        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let keys_call = self.b.ins().call(new_ref, &[]);
        let keys_init = self.b.inst_results(keys_call)[0];
        let keys_var = self.fresh_var(types::I64);
        self.b.def_var(keys_var, keys_init);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let key = self.with_bound_local(&param_place, elem_ty, elem, |this| {
            this.lower_expr(body_expr)
        })?;
        let keys = self.b.use_var(keys_var);
        let push_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);
        self.b.ins().call(push_ref, &[keys, key]);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        let coll = self.b.use_var(coll_var);
        let keys = self.b.use_var(keys_var);
        let sort_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_sort_by_i64_keys, self.b.func);
        self.b.ins().call(sort_ref, &[coll, keys]);
        Ok(self.b.ins().iconst(types::I8, 0))
    }

    fn lower_try_collect(&mut self, recv: &TExpr) -> Result<Value, String> {
        let (ok_ty, err_ty) = jit_result_list_elem(&recv.ty)
            .ok_or_else(|| "jit try_collect unsupported".to_string())?;
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);

        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_init = self.b.inst_results(out_call)[0];
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, out_init);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
        let exit_ok = self.b.create_block();
        let exit_err = self.b.create_block();
        self.b.append_block_param(exit_err, types::I64);
        let merge = self.b.create_block();
        self.b.append_block_param(merge, types::I64);
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit_ok, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let res = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let status_ref = self
            .module
            .declare_func_in_func(self.host.result_is_ok, self.b.func);
        let status_call = self.b.ins().call(status_ref, &[res]);
        let is_ok = self.b.inst_results(status_call)[0];
        let zero_b = self.b.ins().iconst(types::I8, 0);
        let ok = self.b.ins().icmp(IntCC::NotEqual, is_ok, zero_b);
        let push_block = self.b.create_block();
        self.b.ins().brif(ok, push_block, &[], exit_err, &[res]);

        self.b.switch_to_block(push_block);
        self.b.seal_block(push_block);
        let payload = self.result_payload(res, &ok_ty)?;
        let out = self.b.use_var(out_var);
        let push_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);
        self.b.ins().call(push_ref, &[out, payload]);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit_ok);
        self.b.seal_block(header);
        self.b.seal_block(exit_ok);
        let out = self.b.use_var(out_var);
        // Ok(list) — list handle is i64 payload.
        let tag = self.b.ins().iconst(types::I8, 1);
        let host_ref = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let ok_call = self.b.ins().call(host_ref, &[tag, out]);
        let ok_handle = self.b.inst_results(ok_call)[0];
        self.b.ins().jump(merge, &[ok_handle]);

        self.b.switch_to_block(exit_err);
        self.b.seal_block(exit_err);
        let err_res = self.b.block_params(exit_err)[0];
        let err_payload = self.result_payload(err_res, &err_ty)?;
        let tag = self.b.ins().iconst(types::I8, 0);
        let host_ref = self
            .module
            .declare_func_in_func(self.host.result_new_i64, self.b.func);
        let err_call = self.b.ins().call(host_ref, &[tag, err_payload]);
        let err_handle = self.b.inst_results(err_call)[0];
        self.b.ins().jump(merge, &[err_handle]);

        self.b.switch_to_block(merge);
        self.b.seal_block(merge);
        Ok(self.b.block_params(merge)[0])
    }

    fn lower_iter_take_skip_while(
        &mut self,
        recv: &TExpr,
        args: &[TExpr],
        is_skip: bool,
    ) -> Result<Value, String> {
        let elem_ty = jit_list_iter_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(elem_ty, Type::Int) {
            return Err("jit closure method unsupported".to_string());
        }
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);

        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_init = self.b.inst_results(out_call)[0];
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, out_init);

        let flag_var = self.fresh_var(types::I8);
        let one_b = self.b.ins().iconst(types::I8, 1);
        self.b.def_var(flag_var, one_b);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let pred = self.with_bound_local(&param_place, Type::Int, elem, |this| this.lower_expr(body_expr))?;
        let zero_b = self.b.ins().iconst(types::I8, 0);
        let pred_true = self.b.ins().icmp(IntCC::NotEqual, pred, zero_b);

        if !is_skip {
            let keep = self.b.create_block();
            self.b.ins().brif(pred_true, keep, &[], exit, &[]);
            self.b.switch_to_block(keep);
            self.b.seal_block(keep);
            let out = self.b.use_var(out_var);
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[out, elem]);
            self.b.ins().jump(step, &[]);
        } else {
            let still_skip = self.b.create_block();
            let take_rest = self.b.create_block();
            let flag = self.b.use_var(flag_var);
            let skipping = self.b.ins().icmp(IntCC::NotEqual, flag, zero_b);
            self.b.ins().brif(skipping, still_skip, &[], take_rest, &[]);

            self.b.switch_to_block(still_skip);
            self.b.seal_block(still_skip);
            let stop_skip = self.b.create_block();
            self.b.ins().brif(pred_true, step, &[], stop_skip, &[]);
            self.b.switch_to_block(stop_skip);
            self.b.seal_block(stop_skip);
            let zero_flag = self.b.ins().iconst(types::I8, 0);
            self.b.def_var(flag_var, zero_flag);
            let out = self.b.use_var(out_var);
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[out, elem]);
            self.b.ins().jump(step, &[]);

            self.b.switch_to_block(take_rest);
            self.b.seal_block(take_rest);
            let out = self.b.use_var(out_var);
            let push_ref = self
                .module
                .declare_func_in_func(self.host.coll.list_push, self.b.func);
            self.b.ins().call(push_ref, &[out, elem]);
            self.b.ins().jump(step, &[]);
        }

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(out_var))
    }

    fn lower_iter_fold(&mut self, recv: &TExpr, args: &[TExpr]) -> Result<Value, String> {
        let elem_ty = jit_closure_elem_type(&recv.ty)
            .or_else(|| jit_list_iter_elem_type(&recv.ty))
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(elem_ty, Type::Int | Type::Named(_)) || args.len() < 2 {
            return Err("jit closure method unsupported".to_string());
        }
        let seed = self.lower_expr(&args[0])?;
        let TExprKind::Lambda(lam) = &args[1].kind else {
            return Err("jit closure method unsupported".to_string());
        };
        if !lam.prep.is_empty() || lam.source_params.len() != 2 {
            return Err("jit closure method unsupported".to_string());
        }
        let TLambdaBody::Expr(body_expr) = &lam.executable else {
            return Err("jit closure method unsupported".to_string());
        };
        let acc_place = TIR::local_place(&lam.source_params[0]);
        let elem_place = TIR::local_place(&lam.source_params[1]);
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);
        let acc_var = self.fresh_var(types::I64);
        self.b.def_var(acc_var, seed);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let acc = self.b.use_var(acc_var);

        let next_acc = self.with_bound_local(&acc_place, Type::Int, acc, |this| {
            this.with_bound_local(&elem_place, elem_ty.clone(), elem, |inner| {
                inner.lower_expr(body_expr)
            })
        })?;
        self.b.def_var(acc_var, next_acc);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(acc_var))
    }

    /// Serial para_partition → `(false_, true_)` record (order-preserving).
    fn lower_para_partition(&mut self, recv: &TExpr, args: &[TExpr]) -> Result<Value, String> {
        let elem_ty = jit_closure_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(elem_ty, Type::Int | Type::String | Type::Named(_)) {
            return Err("jit closure method unsupported".to_string());
        }
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);

        let new_list = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let falses_call = self.b.ins().call(new_list, &[]);
        let falses = self.b.inst_results(falses_call)[0];
        let falses_var = self.fresh_var(types::I64);
        self.b.def_var(falses_var, falses);
        let trues_call = self.b.ins().call(new_list, &[]);
        let trues = self.b.inst_results(trues_call)[0];
        let trues_var = self.fresh_var(types::I64);
        self.b.def_var(trues_var, trues);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let pred = self.with_bound_local(&param_place, elem_ty.clone(), elem, |this| {
            this.lower_expr(body_expr)
        })?;
        let zero_b = self.b.ins().iconst(types::I8, 0);
        let pred_true = self.b.ins().icmp(IntCC::NotEqual, pred, zero_b);
        let to_true = self.b.create_block();
        let to_false = self.b.create_block();
        self.b.ins().brif(pred_true, to_true, &[], to_false, &[]);

        self.b.switch_to_block(to_true);
        self.b.seal_block(to_true);
        let push = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);
        let tlist = self.b.use_var(trues_var);
        self.b.ins().call(push, &[tlist, elem]);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(to_false);
        self.b.seal_block(to_false);
        let flist = self.b.use_var(falses_var);
        self.b.ins().call(push, &[flist, elem]);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        let falses = self.b.use_var(falses_var);
        let trues = self.b.use_var(trues_var);
        let handle = self.new_record(2);
        self.set_record_slot(handle, 0, falses, &Type::Int)?;
        self.set_record_slot(handle, 1, trues, &Type::Int)?;
        Ok(handle)
    }

    /// Serial para_fold: call seed once, fold with step (single chunk; merge unused).
    fn lower_para_fold(&mut self, recv: &TExpr, args: &[TExpr]) -> Result<Value, String> {
        let elem_ty = jit_closure_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(elem_ty, Type::Int | Type::Named(_)) || args.len() != 3 {
            return Err("jit closure method unsupported".to_string());
        }
        let TExprKind::Lambda(seed_lam) = &args[0].kind else {
            return Err("jit closure method unsupported".to_string());
        };
        if !seed_lam.prep.is_empty() || !seed_lam.source_params.is_empty() {
            return Err("jit closure method unsupported".to_string());
        }
        let TLambdaBody::Expr(seed_body) = &seed_lam.executable else {
            return Err("jit closure method unsupported".to_string());
        };
        let seed = self.lower_expr(seed_body)?;

        let TExprKind::Lambda(step_lam) = &args[1].kind else {
            return Err("jit closure method unsupported".to_string());
        };
        if !step_lam.prep.is_empty() || step_lam.source_params.len() != 2 {
            return Err("jit closure method unsupported".to_string());
        }
        let TLambdaBody::Expr(step_body) = &step_lam.executable else {
            return Err("jit closure method unsupported".to_string());
        };
        let acc_place = TIR::local_place(&step_lam.source_params[0]);
        let elem_place = TIR::local_place(&step_lam.source_params[1]);

        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);
        let acc_var = self.fresh_var(types::I64);
        self.b.def_var(acc_var, seed);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let acc = self.b.use_var(acc_var);
        let next_acc = self.with_bound_local(&acc_place, Type::Int, acc, |this| {
            this.with_bound_local(&elem_place, elem_ty.clone(), elem, |inner| {
                inner.lower_expr(step_body)
            })
        })?;
        self.b.def_var(acc_var, next_acc);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(acc_var))
    }

    fn lower_iter_position(&mut self, recv: &TExpr, args: &[TExpr]) -> Result<Value, String> {
        let elem_ty = jit_list_iter_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(elem_ty, Type::Int) {
            return Err("jit closure method unsupported".to_string());
        }
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);
        let result_var = self.fresh_var(types::I64);
        let none = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(result_var, none);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let pred = self.with_bound_local(&param_place, Type::Int, elem, |this| this.lower_expr(body_expr))?;
        let found = self.b.create_block();
        let zero_b = self.b.ins().iconst(types::I8, 0);
        let pred_true = self.b.ins().icmp(IntCC::NotEqual, pred, zero_b);
        self.b.ins().brif(pred_true, found, &[], step, &[]);

        self.b.switch_to_block(found);
        self.b.seal_block(found);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let packed = self.b.ins().iadd(idx, one);
        self.b.def_var(result_var, packed);
        self.b.ins().jump(exit, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(result_var))
    }

    fn lower_iter_min_max_by(
        &mut self,
        recv: &TExpr,
        args: &[TExpr],
        is_max: bool,
    ) -> Result<Value, String> {
        let elem_ty = jit_list_iter_elem_type(&recv.ty)
            .ok_or_else(|| "jit closure method unsupported".to_string())?;
        if !matches!(elem_ty, Type::String) {
            return Err("jit closure method unsupported".to_string());
        }
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);

        let result_var = self.fresh_var(types::I64);
        let none = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(result_var, none);
        let best_key_var = self.fresh_var(types::I64);
        let zero_key = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(best_key_var, zero_key);
        let has_var = self.fresh_var(types::I8);
        let zero_b0 = self.b.ins().iconst(types::I8, 0);
        self.b.def_var(has_var, zero_b0);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let elem = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let elem_var = self.fresh_var(types::I64);
        self.b.def_var(elem_var, elem);
        let key = self.with_bound_local(&param_place, elem_ty.clone(), elem, |this| this.lower_expr(body_expr))?;
        let key_var = self.fresh_var(types::I64);
        self.b.def_var(key_var, key);

        let first = self.b.create_block();
        let compare = self.b.create_block();
        let zero_b = self.b.ins().iconst(types::I8, 0);
        let has_flag = self.b.use_var(has_var);
        let has = self.b.ins().icmp(IntCC::NotEqual, has_flag, zero_b);
        self.b.ins().brif(has, compare, &[], first, &[]);

        self.b.switch_to_block(first);
        self.b.seal_block(first);
        let one = self.b.ins().iconst(types::I64, 1);
        let elem_now = self.b.use_var(elem_var);
        let packed = self.b.ins().iadd(elem_now, one);
        self.b.def_var(result_var, packed);
        let key_now = self.b.use_var(key_var);
        self.b.def_var(best_key_var, key_now);
        let one_b = self.b.ins().iconst(types::I8, 1);
        self.b.def_var(has_var, one_b);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(compare);
        self.b.seal_block(compare);
        let key_now = self.b.use_var(key_var);
        let best_key = self.b.use_var(best_key_var);
        // Match Rust/AOT: min_by_key keeps first on ties; max_by_key keeps last.
        let better = if is_max {
            let gt = self
                .b
                .ins()
                .icmp(IntCC::SignedGreaterThan, key_now, best_key);
            let eq = self.b.ins().icmp(IntCC::Equal, key_now, best_key);
            self.b.ins().bor(gt, eq)
        } else {
            self.b
                .ins()
                .icmp(IntCC::SignedLessThan, key_now, best_key)
        };
        let take = self.b.create_block();
        self.b.ins().brif(better, take, &[], step, &[]);
        self.b.switch_to_block(take);
        self.b.seal_block(take);
        let one = self.b.ins().iconst(types::I64, 1);
        let elem_now = self.b.use_var(elem_var);
        let packed = self.b.ins().iadd(elem_now, one);
        self.b.def_var(result_var, packed);
        let key_now = self.b.use_var(key_var);
        self.b.def_var(best_key_var, key_now);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(result_var))
    }

    fn lower_iter_flat_map(&mut self, recv: &TExpr, args: &[TExpr]) -> Result<Value, String> {
        if !jit_list_of_int_list_type(&recv.ty) {
            return Err("jit closure method unsupported".to_string());
        }
        let (param_place, body_expr) = self.closure_unary_lambda(args)?;
        let recv_val = self.lower_expr(recv)?;
        let coll_var = self.fresh_var(types::I64);
        self.b.def_var(coll_var, recv_val);
        let new_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_new, self.b.func);
        let out_call = self.b.ins().call(new_ref, &[]);
        let out_init = self.b.inst_results(out_call)[0];
        let out_var = self.fresh_var(types::I64);
        self.b.def_var(out_var, out_init);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let step = self.b.create_block();
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
        let done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, idx, len);
        self.b.ins().brif(done, exit, &[], body, &[]);

        self.b.switch_to_block(body);
        self.b.seal_block(body);
        let get_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_get, self.b.func);
        let line = self.b.ins().iconst(types::I32, 0);
        let get_call = self.b.ins().call(get_ref, &[coll, idx, line]);
        let inner_list = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let list_ty = Type::List(Box::new(Type::Int));
        let mapped = self.with_bound_local(&param_place, list_ty, inner_list, |this| this.lower_expr(body_expr))?;

        let inner_header = self.b.create_block();
        let inner_body = self.b.create_block();
        let inner_step = self.b.create_block();
        let inner_exit = self.b.create_block();
        let j_var = self.fresh_var(types::I64);
        let mapped_var = self.fresh_var(types::I64);
        self.b.def_var(mapped_var, mapped);
        let jz = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(j_var, jz);
        self.b.ins().jump(inner_header, &[]);

        self.b.switch_to_block(inner_header);
        let j = self.b.use_var(j_var);
        let mapped_list = self.b.use_var(mapped_var);
        let inner_len_call = self.b.ins().call(len_ref, &[mapped_list]);
        let inner_len = self.b.inst_results(inner_len_call)[0];
        let inner_done = self
            .b
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, j, inner_len);
        self.b
            .ins()
            .brif(inner_done, inner_exit, &[], inner_body, &[]);

        self.b.switch_to_block(inner_body);
        self.b.seal_block(inner_body);
        let get_call = self.b.ins().call(get_ref, &[mapped_list, j, line]);
        let v = self.b.inst_results(get_call)[0];
        self.emit_trap_check()?;
        let out = self.b.use_var(out_var);
        let push_ref = self
            .module
            .declare_func_in_func(self.host.coll.list_push, self.b.func);
        self.b.ins().call(push_ref, &[out, v]);
        self.b.ins().jump(inner_step, &[]);

        self.b.switch_to_block(inner_step);
        self.b.seal_block(inner_step);
        let j = self.b.use_var(j_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let jn = self.b.ins().iadd(j, one);
        self.b.def_var(j_var, jn);
        self.b.ins().jump(inner_header, &[]);

        self.b.switch_to_block(inner_exit);
        self.b.seal_block(inner_header);
        self.b.seal_block(inner_exit);
        self.b.ins().jump(step, &[]);

        self.b.switch_to_block(step);
        self.b.seal_block(step);
        let idx = self.b.use_var(idx_var);
        let one = self.b.ins().iconst(types::I64, 1);
        let next = self.b.ins().iadd(idx, one);
        self.b.def_var(idx_var, next);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.b.seal_block(header);
        self.b.seal_block(exit);
        Ok(self.b.use_var(out_var))
    }
}

fn core_struct_field_index(type_name: &str, field: &str) -> Option<usize> {
    let fields: &[&str] = match type_name {
        "DirEntry" => &["name", "path", "is_dir"],
        "Stat" => &[
            "size",
            "modified_ms",
            "created_ms",
            "readonly",
            "is_file",
            "is_dir",
            "is_symlink",
            "kind",
        ],
        "WalkEntry" => &["path", "relative", "is_dir", "depth"],
        "TempDir" | "TempFile" | "FileLock" => &["path"],
        "LogField" => &["key", "value", "kind", "redacted"],
        "LogSpan" => &["id", "name"],
        // Mirrors jet_std::ProcessResult field order (Open.rs).
        "ProcessResult" => &["code", "output", "errors", "success", "signal", "timed_out"],
        // D-ENCSTREAM-SURFACE1 / jet_std::EncodingLimits.
        "EncodingLimits" => &[
            "buffer_bytes",
            "max_depth",
            "max_item_bytes",
            "max_total_bytes",
            "max_expansion_depth",
            "max_expansion_bytes",
        ],
        "DataLimits" => &[
            "encoding",
            "max_groups",
            "max_sort_rows",
            "max_join_rows",
            "max_output_rows",
        ],
        "DataStatus" => &[
            "step",
            "path",
            "copy",
            "ownership",
            "trust",
            "fallback",
            "replacement",
        ],
        "DataGroup" => &["key", "count", "sum", "mean"],
        "DataError" => &[
            "kind",
            "operation",
            "row",
            "column",
            "index",
            "reason",
            "cause",
        ],
        "DataSummary" => &[
            "count",
            "sum",
            "mean",
            "min",
            "max",
            "median",
            "variance",
            "stddev",
        ],
        "DataTable" | "Table" | "LazyFrame" => &["rows", "missing", "plan"],
        "Series" | "DataSeries" => &["values", "missing"],
        "DataColumn" => &["name", "type_name"],
        "DataJoin" | "Join" => &["left", "right"],
        "DataPivotCell" => &["row_key", "column_key", "count", "sum", "mean"],
        "EncodingCause" => &["kind", "os_code", "message"],
        "EncodingError" => &[
            "format",
            "kind",
            "byte_offset",
            "line",
            "column",
            "path",
            "reason",
            "cause",
        ],
        "FieldError" | "DecodeError" => &["path", "reason"],
        "MigrationStatus" => &["migrated", "from", "steps"],
        "DecodeResult" => &["value", "migration"],
        "TextWidth" => &["ambiguous", "controls"],
        "Claims" => &["subject", "audience", "issuer", "expires_at", "issued_at"],
        "Rotation" => &["previous", "current"],
        _ => return None,
    };
    fields.iter().position(|f| *f == field)
}

fn structured_record_field_place(place: &TPlace) -> Option<(&TLocal, &str)> {
    let TPlace::Expr(expr) = place else {
        return None;
    };
    let TExprKind::Field {
        recv,
        field,
        boxed: false,
    } = &expr.kind
    else {
        return None;
    };
    let TExprKind::Local(local) = &recv.kind else {
        return None;
    };
    Some((local, field.as_str()))
}
