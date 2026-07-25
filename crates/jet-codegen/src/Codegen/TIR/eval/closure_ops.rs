//! Closure-method evaluation for the canonical TIR evaluator (#778 deopt).
use std::collections::HashMap;

use crate::Codegen::TIR::{TClosureOp, TExpr, TExprKind, TLambda, TLambdaBody};
use crate::Comptime::Builtins::{as_bool, cmp};
use crate::Comptime::CtValue;
use crate::Diagnostics::Diagnostic;

use super::{materialize_view_mut_window, unsupported, EvalCtx, Flow};

impl EvalCtx<'_> {
    pub(super) fn eval_closure_method(
        &mut self,
        recv: &TExpr,
        op: &TClosureOp,
        args: &[TExpr],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let mut recv_v = self.eval_expr(recv, scope)?;
        // ViewMut place-window → inclusive List for read-only map/fold.
        if let CtValue::Struct {
            type_name,
            fields,
        } = &recv_v
        {
            if type_name == "__JetViewMut"
                && matches!(op, TClosureOp::ViewMap | TClosureOp::ViewFold)
            {
                recv_v = materialize_view_mut_window(fields, scope, self.span())?;
            }
        }
        let mut call1 = |this: &mut Self, item: CtValue| -> Result<CtValue, Diagnostic> {
            let f = args
                .first()
                .ok_or_else(|| unsupported("closure method arg", this.span()))?;
            this.apply_callable(f, vec![item], scope)
        };
        match op {
            TClosureOp::Map | TClosureOp::MapMut | TClosureOp::ViewMap => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("map receiver", self.span()));
                };
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(call1(self, item)?);
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::Filter => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("filter receiver", self.span()));
                };
                let mut out = Vec::new();
                for item in items {
                    let keep = call1(self, item.clone())?;
                    if as_bool(&keep, self.span())? {
                        out.push(item);
                    }
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::Each | TClosureOp::EachMut | TClosureOp::EachRef => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("each receiver", self.span()));
                };
                for item in items {
                    let _ = call1(self, item)?;
                }
                Ok(CtValue::Unit)
            }
            TClosureOp::Find => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("find receiver", self.span()));
                };
                for item in items {
                    let keep = call1(self, item.clone())?;
                    if as_bool(&keep, self.span())? {
                        return Ok(CtValue::Some(Box::new(item)));
                    }
                }
                Ok(CtValue::None(crate::AST::Type::Named("Any".into())))
            }
            TClosureOp::Any | TClosureOp::BagAny => {
                let items = match recv_v {
                    CtValue::List(items) => items,
                    CtValue::Struct { type_name, fields }
                        if type_name == "Bag" || type_name.ends_with("Bag") =>
                    {
                        fields
                            .into_iter()
                            .find_map(|(name, value)| match (name.as_str(), value) {
                                ("items", CtValue::List(items)) => Some(items),
                                _ => None,
                            })
                            .unwrap_or_default()
                    }
                    _ => {
                        return Err(unsupported("any receiver", self.span()));
                    }
                };
                for item in items {
                    if as_bool(&call1(self, item)?, self.span())? {
                        return Ok(CtValue::Bool(true));
                    }
                }
                Ok(CtValue::Bool(false))
            }
            TClosureOp::All => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("all receiver", self.span()));
                };
                for item in items {
                    if !as_bool(&call1(self, item)?, self.span())? {
                        return Ok(CtValue::Bool(false));
                    }
                }
                Ok(CtValue::Bool(true))
            }
            TClosureOp::Reduce | TClosureOp::Fold | TClosureOp::ViewFold => {
                if args.len() < 2 {
                    return Err(unsupported("reduce arity", self.span()));
                }
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("reduce receiver", self.span()));
                };
                let mut acc = self.eval_expr(&args[0], scope)?;
                let f = &args[1];
                for item in items {
                    acc = self.apply_callable(f, vec![acc, item], scope)?;
                }
                Ok(acc)
            }
            TClosureOp::SortBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("sort_by receiver", self.span()));
                };
                let mut keyed = Vec::with_capacity(items.len());
                for item in items {
                    let k = call1(self, item.clone())?;
                    keyed.push((k, item));
                }
                let span = self.span();
                let mut sort_err = None;
                keyed.sort_by(|a, b| match cmp(a.0.clone(), b.0.clone(), span) {
                    Ok(o) => o,
                    Err(e) => {
                        sort_err.get_or_insert(e);
                        std::cmp::Ordering::Equal
                    }
                });
                if let Some(e) = sort_err {
                    return Err(e);
                }
                let sorted = CtValue::List(keyed.into_iter().map(|(_, v)| v).collect());
                self.write_back_place(recv, sorted, scope)?;
                Ok(CtValue::Unit)
            }
            TClosureOp::TakeWhile => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("take_while receiver", self.span()));
                };
                let mut out = Vec::new();
                for item in items {
                    if !as_bool(&call1(self, item.clone())?, self.span())? {
                        break;
                    }
                    out.push(item);
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::SkipWhile => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("skip_while receiver", self.span()));
                };
                let mut skipping = true;
                let mut out = Vec::new();
                for item in items {
                    if skipping {
                        if as_bool(&call1(self, item.clone())?, self.span())? {
                            continue;
                        }
                        skipping = false;
                    }
                    out.push(item);
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::FlatMap => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("flat_map receiver", self.span()));
                };
                let mut out = Vec::new();
                for item in items {
                    match call1(self, item)? {
                        CtValue::List(inner) => out.extend(inner),
                        other => out.push(other),
                    }
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::FilterMap => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("filter_map receiver", self.span()));
                };
                let mut out = Vec::new();
                for item in items {
                    match call1(self, item)? {
                        CtValue::Some(v) | CtValue::ResOk(v) => out.push(*v),
                        CtValue::None(_) | CtValue::ResErr(_) => {}
                        other => out.push(other),
                    }
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::Position => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("position receiver", self.span()));
                };
                for (i, item) in items.into_iter().enumerate() {
                    if as_bool(&call1(self, item)?, self.span())? {
                        return Ok(CtValue::Some(Box::new(CtValue::Int(i as i64))));
                    }
                }
                Ok(CtValue::None(crate::AST::Type::Int))
            }
            TClosureOp::OptionMap => match recv_v {
                CtValue::Some(inner) => Ok(CtValue::Some(Box::new(call1(self, *inner)?))),
                CtValue::None(t) => Ok(CtValue::None(t)),
                _ => Err(unsupported("option map receiver", self.span())),
            },
            TClosureOp::ParaMap => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("para_map receiver", self.span()));
                };
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(call1(self, item)?);
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::ParaFilter => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("para_filter receiver", self.span()));
                };
                let mut out = Vec::new();
                for item in items {
                    let keep = call1(self, item.clone())?;
                    if as_bool(&keep, self.span())? {
                        out.push(item);
                    }
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::ParaFold => {
                if args.len() < 2 {
                    return Err(unsupported("para_fold arity", self.span()));
                }
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("para_fold receiver", self.span()));
                };
                let mut acc = self.eval_expr(&args[0], scope)?;
                let f = &args[1];
                for item in items {
                    acc = self.apply_callable(f, vec![acc, item], scope)?;
                }
                Ok(acc)
            }
            // D-ITERTOOLS1=A: key-selecting reducers (JIT may deopt here; #729 owns native).
            op @ (TClosureOp::MinBy | TClosureOp::MaxBy) => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("min_by/max_by receiver", self.span()));
                };
                let Some(mut best) = items.first().cloned() else {
                    return Ok(CtValue::None(crate::AST::Type::Named("Any".into())));
                };
                let maximum = matches!(op, TClosureOp::MaxBy);
                let mut best_key = call1(self, best.clone())?;
                for candidate in items.into_iter().skip(1) {
                    let candidate_key = call1(self, candidate.clone())?;
                    let order = cmp(best_key.clone(), candidate_key.clone(), self.span())?;
                    if (maximum && order != std::cmp::Ordering::Greater)
                        || (!maximum && order == std::cmp::Ordering::Greater)
                    {
                        best = candidate;
                        best_key = candidate_key;
                    }
                }
                Ok(CtValue::Some(Box::new(best)))
            }
            TClosureOp::CountBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("count_by receiver", self.span()));
                };
                let mut out = std::collections::BTreeMap::new();
                for item in items {
                    let key_v = call1(self, item)?;
                    let key = crate::AST::CtKey::from_value(key_v)
                        .ok_or_else(|| unsupported("this map key type", self.span()))?;
                    match out.entry(key).or_insert(CtValue::Int(0)) {
                        CtValue::Int(n) => *n += 1,
                        _ => unreachable!(),
                    }
                }
                Ok(CtValue::Map(out))
            }
            TClosureOp::EachMap => {
                let CtValue::Map(entries) = recv_v else {
                    return Err(unsupported("map each receiver", self.span()));
                };
                let f = args
                    .first()
                    .ok_or_else(|| unsupported("map each arg", self.span()))?;
                for (key, value) in entries {
                    let _ = self.apply_callable(
                        f,
                        vec![key.to_value(), value],
                        scope,
                    )?;
                }
                Ok(CtValue::Unit)
            }
            TClosureOp::Partition { .. } | TClosureOp::ParaPartition { .. } => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("partition receiver", self.span()));
                };
                let mut trues = Vec::new();
                let mut falses = Vec::new();
                for item in items {
                    if as_bool(&call1(self, item.clone())?, self.span())? {
                        trues.push(item);
                    } else {
                        falses.push(item);
                    }
                }
                // AOT emits a 2-field tuple/struct with `false_` then `true_`
                // (see INLINE_HOF_EXPECTED: split.false_ / split.true_).
                Ok(CtValue::Struct {
                    type_name: "Partition".to_string(),
                    fields: vec![
                        ("false_".to_string(), CtValue::List(falses)),
                        ("true_".to_string(), CtValue::List(trues)),
                    ],
                })
            }
            TClosureOp::Scan => {
                if args.len() < 2 {
                    return Err(unsupported("scan arity", self.span()));
                }
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("scan receiver", self.span()));
                };
                let mut acc = self.eval_expr(&args[0], scope)?;
                let f = &args[1];
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    acc = self.apply_callable(f, vec![acc, item], scope)?;
                    out.push(acc.clone());
                }
                Ok(CtValue::List(out))
            }
            TClosureOp::GroupBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("group_by receiver", self.span()));
                };
                let mut out: std::collections::BTreeMap<crate::AST::CtKey, Vec<CtValue>> =
                    std::collections::BTreeMap::new();
                for item in items {
                    let key_v = call1(self, item.clone())?;
                    let key = crate::AST::CtKey::from_value(key_v)
                        .ok_or_else(|| unsupported("this group_by key type", self.span()))?;
                    out.entry(key).or_default().push(item);
                }
                Ok(CtValue::Map(
                    out.into_iter()
                        .map(|(k, vs)| (k, CtValue::List(vs)))
                        .collect(),
                ))
            }
        }
    }

    pub(super) fn apply_callable(
        &mut self,
        f: &TExpr,
        argv: Vec<CtValue>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        match &f.kind {
            TExprKind::Lambda(lam) => self.eval_tlambda(lam, argv, scope),
            TExprKind::Local(local) => {
                if let Some(CtValue::Closure(_)) = scope.get(&local.name) {
                    return Err(unsupported(
                        "stored CtValue::Closure invoke (use inline lambda)",
                        self.span(),
                    ));
                }
                if let Some(func) = self.funcs.get(&local.name).copied() {
                    let mut child = HashMap::new();
                    return self.run_func(func, argv, &mut child);
                }
                let v = self.eval_expr(f, scope)?;
                Err(unsupported(
                    &format!("callable value `{v:?}`"),
                    self.span(),
                ))
            }
            _ => {
                if let TExprKind::Call { name, .. } = &f.kind {
                    if let Some(func) = self.funcs.get(name).copied() {
                        let mut child = HashMap::new();
                        return self.run_func(func, argv, &mut child);
                    }
                }
                Err(unsupported("callable form", self.span()))
            }
        }
    }

    fn eval_tlambda(
        &mut self,
        lam: &TLambda,
        argv: Vec<CtValue>,
        outer: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let mut child = outer.clone();
        let param_names: std::collections::HashSet<String> =
            lam.source_params.iter().cloned().collect();
        for (i, name) in lam.source_params.iter().enumerate() {
            child.insert(
                name.clone(),
                argv.get(i).cloned().unwrap_or(CtValue::Unit),
            );
        }
        let result = match &lam.executable {
            TLambdaBody::Expr(e) => self.eval_expr(e, &mut child)?,
            TLambdaBody::Block(stmts) => match self.exec_stmts(stmts, &mut child)? {
                Flow::Return(v) => v,
                Flow::Normal => CtValue::Unit,
                other => {
                    return Err(unsupported(
                        &format!("control flow {other:?} escaping lambda"),
                        self.span(),
                    ));
                }
            },
        };
        // FnMut capture write-back: mutations to outer locals (Set.add, Map.add, …)
        // must be visible after the lambda returns (#777 pure-parity HOF).
        for (k, v) in child {
            if param_names.contains(&k) {
                continue;
            }
            if outer.contains_key(&k) {
                outer.insert(k, v);
            }
        }
        Ok(result)
    }
}
