//! Closure-method evaluation for the canonical TIR evaluator (#778 deopt).
use std::collections::HashMap;

use crate::Codegen::TIR::{TClosureOp, TExpr, TExprKind, TLambda, TLambdaBody};
use crate::Comptime::Builtins::{as_bool, cmp};
use crate::Comptime::{CtReport, CtValue};
use crate::Diagnostics::{Diagnostic, Span};

use super::{
    materialize_view_mut_window, progress_elapsed, progress_emit, progress_iter_parts,
    progress_iter_value, progress_no_color, progress_now, unsupported, EvalCtx, Flow,
};

fn ordering_cmp(value: &CtValue, span: Span) -> Result<std::cmp::Ordering, Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported("sort_by comparator must return Ordering", span));
    };
    if type_name != crate::Syntax::TYPE_ORDERING || !args.is_empty() {
        return Err(unsupported("sort_by comparator must return Ordering", span));
    }
    match variant.as_str() {
        "Less" => Ok(std::cmp::Ordering::Less),
        "Equal" => Ok(std::cmp::Ordering::Equal),
        "Greater" => Ok(std::cmp::Ordering::Greater),
        _ => Err(unsupported("sort_by comparator must return Ordering", span)),
    }
}

fn progress_parts(
    value: &CtValue,
) -> Option<(Vec<CtValue>, String, String, f64, Vec<usize>, usize, usize, bool)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "__JetProgressIter" {
        return None;
    }
    let items = fields.iter().find_map(|(name, value)| {
        (name == "items").then(|| match value {
            CtValue::List(items) => Some(items.clone()),
            _ => None,
        })
    })??;
    let description = fields.iter().find_map(|(name, value)| {
        (name == "description").then(|| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
    })??;
    let format = fields.iter().find_map(|(name, value)| {
        (name == "format").then(|| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
    })??;
    let started_at = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "started_at").then(|| match value {
                CtValue::Float(value) => Some(value.as_f64()),
                CtValue::Int(value) => Some(*value as f64),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or_else(progress_now);
    let pulls = fields
        .iter()
        .find(|(name, _)| name == "pulls")
        .and_then(|(_, value)| match value {
            CtValue::List(values) => values
                .iter()
                .map(|value| match value {
                    CtValue::Int(value) => Some((*value).max(0) as usize),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>(),
            _ => None,
        })
        .unwrap_or_else(|| vec![1; items.len()]);
    let tail = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "tail").then(|| match value {
                CtValue::Int(value) => Some((*value).max(0) as usize),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(0);
    let total = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "total").then(|| match value {
                CtValue::Int(value) => Some((*value).max(0) as usize),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(items.len());
    let known_total = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "known_total").then(|| match value {
                CtValue::Bool(value) => Some(*value),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(true);
    Some((
        items,
        description,
        format,
        started_at,
        pulls,
        tail,
        total,
        known_total,
    ))
}

fn progress_value(
    items: Vec<CtValue>,
    description: String,
    format: String,
    started_at: f64,
    pulls: Vec<usize>,
    tail: usize,
    total: usize,
    known_total: bool,
) -> CtValue {
    CtValue::Struct {
        type_name: "__JetProgressIter".to_string(),
        fields: vec![
            ("items".to_string(), CtValue::List(items)),
            ("description".to_string(), CtValue::Str(description)),
            ("format".to_string(), CtValue::Str(format)),
            ("started_at".to_string(), CtValue::Float(crate::AST::CtFloat::f64(started_at))),
            (
                "pulls".to_string(),
                CtValue::List(pulls.into_iter().map(|n| CtValue::Int(n as i64)).collect()),
            ),
            ("tail".to_string(), CtValue::Int(tail as i64)),
            ("total".to_string(), CtValue::Int(total as i64)),
            ("known_total".to_string(), CtValue::Bool(known_total)),
        ],
    }
}

mod progress_semantics {
    include!("../../../Prelude/Core/Progress.rs");
}

fn emit_progress_raw(
    sink: Option<&std::sync::Arc<std::sync::Mutex<crate::Comptime::DevSink>>>,
    description: &str,
    format: &str,
    started_at: f64,
    total: usize,
    known_total: bool,
    count: &mut usize,
    raw_pulls: usize,
) {
    for _ in 0..raw_pulls {
        *count = (*count).saturating_add(1);
        let text = progress_semantics::jet_progress_render(
            description,
            format,
            *count,
            known_total.then_some(total),
            progress_elapsed(started_at),
            progress_no_color(),
        );
        progress_emit(sink, &text);
    }
}

fn progress_passthrough(
    progress: &Option<(Vec<CtValue>, String, String, f64, Vec<usize>, usize, usize, bool)>,
    output_len: usize,
) -> (Vec<usize>, usize) {
    let Some((_, _, _, _, pulls, tail, _, _)) = progress else {
        return (Vec::new(), 0);
    };
    (pulls.iter().copied().take(output_len).collect(), *tail)
}

fn emit_progress_next(
    progress: &Option<(Vec<CtValue>, String, String, f64, Vec<usize>, usize, usize, bool)>,
    sink: Option<&std::sync::Arc<std::sync::Mutex<crate::Comptime::DevSink>>>,
    cursor: &mut usize,
    count: &mut usize,
) {
    let Some((_, description, format, started_at, pulls, _, total, known_total)) = progress else {
        return;
    };
    let raw = pulls.get(*cursor).copied().unwrap_or(1);
    *cursor = (*cursor).saturating_add(1);
    emit_progress_raw(
        sink,
        description,
        format,
        *started_at,
        *total,
        *known_total,
        count,
        raw,
    );
}

fn emit_progress_finish(
    progress: &Option<(Vec<CtValue>, String, String, f64, Vec<usize>, usize, usize, bool)>,
    sink: Option<&std::sync::Arc<std::sync::Mutex<crate::Comptime::DevSink>>>,
    cursor: usize,
    count: &mut usize,
) {
    let Some((_, description, format, started_at, pulls, tail, total, known_total)) = progress else {
        return;
    };
    let raw = pulls[cursor..].iter().sum::<usize>() + *tail;
    emit_progress_raw(
        sink,
        description,
        format,
        *started_at,
        *total,
        *known_total,
        count,
        raw,
    );
    if *count != 0 {
        let tty = super::term_semantics::jet_term_stdout_is_terminal();
        let frame = super::term_semantics::jet_term_progress_finish(tty);
        if tty {
            use std::io::Write;
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(frame.as_bytes());
            let _ = out.flush();
        } else if let Some(sink) = sink {
            let mut sink = sink.lock().expect("evaluator sink poisoned");
            sink.stdout.push_str(frame);
        }
    }
}

impl<'a> EvalCtx<'a> {
    pub(super) fn eval_closure_method(
        &mut self,
        recv: &'a TExpr,
        op: &TClosureOp,
        args: &'a [TExpr],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if matches!(op, TClosureOp::EditDisjoint) {
            let base_name = match &recv.kind {
                TExprKind::Local(local) => local.name.clone(),
                TExprKind::Borrow { place, .. } => match &place.kind {
                    TExprKind::Local(local) => local.name.clone(),
                    _ => return Err(unsupported("edit_disjoint base", self.span())),
                },
                _ => return Err(unsupported("edit_disjoint base", self.span())),
            };
            let CtValue::List(items) = scope
                .get(&base_name)
                .cloned()
                .ok_or_else(|| unsupported("edit_disjoint unbound base", self.span()))?
            else {
                return Err(unsupported("edit_disjoint list base", self.span()));
            };
            let CtValue::List(targets) = self.eval_expr_child(&args[0], scope)? else {
                return Err(unsupported("edit_disjoint indexes", self.span()));
            };
            let mut indexes = Vec::with_capacity(targets.len());
            for target in targets {
                indexes.push(crate::Comptime::Builtins::as_int(&target, self.span())?);
            }
            if indexes.len() != 2 {
                return Ok(CtValue::failed(Box::new(CtValue::Str(
                    "edit_disjoint needs exactly two indexes".to_string(),
                ))));
            }
            let ordered = match super::disjoint_semantics::indexes(items.len(), &indexes) {
                Ok(ordered) => ordered,
                Err(error) => return Ok(CtValue::failed(Box::new(CtValue::Str(error)))),
            };
            let mut views = ordered
                .into_iter()
                .map(|(start, end, position)| {
                    (
                        position,
                        CtValue::Struct {
                            type_name: "__JetViewMut".into(),
                            fields: vec![
                                ("base".into(), CtValue::Str(base_name.clone())),
                                ("start".into(), CtValue::Int(start as i64)),
                                ("end".into(), CtValue::Int(end as i64 - 1)),
                            ],
                        },
                    )
                })
                .collect::<Vec<_>>();
            views.sort_by_key(|(position, _)| *position);
            let argv = views.into_iter().map(|(_, view)| view).collect();
            let callable = self.eval_expr(&args[1], scope)?;
            let _ = self.call_callable_in_scope(&callable, argv, scope)?;
            return Ok(CtValue::Present(Box::new(CtValue::Unit)));
        }
        let mut recv_v = self.eval_expr_child(recv, scope)?;
        let progress = progress_parts(&recv_v);
        let iter = progress_iter_parts(&recv_v);
        if let Some((items, _, _, _, _, _, _, _)) = &progress {
            recv_v = CtValue::List(items.clone());
        } else if let Some((items, _)) = &iter {
            recv_v = CtValue::List(items.clone());
        }
        let wrap_list = |items: Vec<CtValue>, pulls: Vec<usize>, tail: usize| match &progress {
            Some((_, description, format, started_at, _, _, total, known_total)) => {
                progress_value(
                    items,
                    description.clone(),
                    format.clone(),
                    *started_at,
                    pulls,
                    tail,
                    *total,
                    *known_total,
                )
            }
            None => match &iter {
                Some(_) => progress_iter_value(items, false),
                None => CtValue::List(items),
            },
        };
        let lazy = matches!(
            op,
            TClosureOp::Map
                | TClosureOp::MapMut
                | TClosureOp::ViewMap
                | TClosureOp::Filter
                | TClosureOp::TakeWhile
                | TClosureOp::SkipWhile
                | TClosureOp::FlatMap
                | TClosureOp::FilterMap
                | TClosureOp::ParaMap
                | TClosureOp::ParaFilter
                | TClosureOp::Scan
        );
        let short_circuit = matches!(
            op,
            TClosureOp::Find
                | TClosureOp::Any
                | TClosureOp::BagAny
                | TClosureOp::All
                | TClosureOp::Position
        );
        if !lazy && !short_circuit {
            if let Some((_, description, format, started_at, pulls, tail, total, known_total)) = &progress {
                if matches!(&recv_v, CtValue::List(_)) {
                    let raw_pulls = pulls.iter().sum::<usize>() + *tail;
                    let mut count = 0;
                    emit_progress_raw(
                        self.sink.as_ref(),
                        description,
                        format,
                        *started_at,
                        *total,
                        *known_total,
                        &mut count,
                        raw_pulls,
                    );
                }
            }
        }
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
        // Evaluate callback expression once per collection operation. A direct
        // lambda expression creates one runtime callable and its canonical
        // `call_callable` path writes FnMut captures back into that storage.
        // Re-evaluating the lambda for every item would throw that storage away.
        let callback_index = match op {
            TClosureOp::Reduce
            | TClosureOp::Fold
            | TClosureOp::ViewFold
            | TClosureOp::Scan
            | TClosureOp::ParaFold
            | TClosureOp::MapFold => 1,
            _ => 0,
        };
        let callback = args
            .get(callback_index)
            .ok_or_else(|| unsupported("closure method arg", self.span()))?;
        let callback_value = self.eval_expr(callback, scope)?;
        let scope_ptr = scope as *mut HashMap<String, CtValue>;
        let calln = |this: &mut Self, argv: Vec<CtValue>| {
            // SAFETY: the collection operation invokes this closure
            // synchronously; no other scope access overlaps the call.
            unsafe { this.call_callable_in_scope(&callback_value, argv, &mut *scope_ptr) }
        };
        let mut progress_cursor = 0usize;
        let mut progress_count = 0usize;
        match op {
            TClosureOp::EditDisjoint => unreachable!(),
            TClosureOp::Map | TClosureOp::MapMut | TClosureOp::ViewMap => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("map receiver", self.span()));
                };
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(calln(self, vec![item])?);
                }
                let (pulls, tail) = progress_passthrough(&progress, out.len());
                Ok(wrap_list(out, pulls, tail))
            }
            TClosureOp::Filter => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("filter receiver", self.span()));
                };
                let (source_pulls, old_tail) = progress
                    .as_ref()
                    .map(|(_, _, _, _, pulls, tail, _, _)| (pulls.clone(), *tail))
                    .unwrap_or_default();
                let mut out = Vec::new();
                let mut out_pulls = Vec::new();
                let mut pending = 0usize;
                for (index, item) in items.into_iter().enumerate() {
                    pending += source_pulls.get(index).copied().unwrap_or(1);
                    let keep = calln(self, vec![item.clone()])?;
                    if as_bool(&keep, self.span())? {
                        out.push(item);
                        out_pulls.push(pending);
                        pending = 0;
                    }
                }
                Ok(wrap_list(out, out_pulls, pending + old_tail))
            }
            TClosureOp::Each | TClosureOp::EachMut | TClosureOp::EachRef => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("each receiver", self.span()));
                };
                for item in items {
                    let _ = calln(self, vec![item])?;
                }
                Ok(CtValue::Unit)
            }
            TClosureOp::Find => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("find receiver", self.span()));
                };
                for item in items {
                    emit_progress_next(
                        &progress,
                        self.sink.as_ref(),
                        &mut progress_cursor,
                        &mut progress_count,
                    );
                    let keep = calln(self, vec![item.clone()])?;
                    if as_bool(&keep, self.span())? {
                        return Ok(CtValue::Present(Box::new(item)));
                    }
                }
                emit_progress_finish(
                    &progress,
                    self.sink.as_ref(),
                    progress_cursor,
                    &mut progress_count,
                );
                Ok(CtValue::absent(crate::AST::Type::Named("Any".into())))
            }
            TClosureOp::Any | TClosureOp::BagAny => {
                let items = match recv_v {
                    CtValue::List(items) => items,
                    CtValue::Struct { type_name, fields }
                        if type_name == crate::Syntax::TYPE_TALLY || type_name.ends_with(crate::Syntax::TYPE_TALLY) =>
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
                    emit_progress_next(
                        &progress,
                        self.sink.as_ref(),
                        &mut progress_cursor,
                        &mut progress_count,
                    );
                    if as_bool(&calln(self, vec![item])?, self.span())? {
                        return Ok(CtValue::Bool(true));
                    }
                }
                emit_progress_finish(
                    &progress,
                    self.sink.as_ref(),
                    progress_cursor,
                    &mut progress_count,
                );
                Ok(CtValue::Bool(false))
            }
            TClosureOp::All => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("all receiver", self.span()));
                };
                for item in items {
                    emit_progress_next(
                        &progress,
                        self.sink.as_ref(),
                        &mut progress_cursor,
                        &mut progress_count,
                    );
                    if !as_bool(&calln(self, vec![item])?, self.span())? {
                        return Ok(CtValue::Bool(false));
                    }
                }
                emit_progress_finish(
                    &progress,
                    self.sink.as_ref(),
                    progress_cursor,
                    &mut progress_count,
                );
                Ok(CtValue::Bool(true))
            }
            TClosureOp::Reduce | TClosureOp::Fold | TClosureOp::ViewFold => {
                if args.len() < 2 {
                    return Err(unsupported("reduce arity", self.span()));
                }
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("reduce receiver", self.span()));
                };
                let mut acc = self.eval_expr_child(&args[0], scope)?;
                for item in items {
                    acc = calln(self, vec![acc, item])?;
                }
                Ok(acc)
            }
            TClosureOp::SortBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("sort_by receiver", self.span()));
                };
                let mut keyed = Vec::with_capacity(items.len());
                for item in items {
                    let k = calln(self, vec![item.clone()])?;
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
            TClosureOp::SortByCompare => {
                let CtValue::List(mut items) = recv_v else {
                    return Err(unsupported("sort_by receiver", self.span()));
                };
                let span = self.span();
                let mut sort_err = None;
                items.sort_by(|left, right| {
                    match calln(self, vec![left.clone(), right.clone()])
                        .and_then(|value| ordering_cmp(&value, span))
                    {
                        Ok(order) => order,
                        Err(error) => {
                            sort_err.get_or_insert(error);
                            std::cmp::Ordering::Equal
                        }
                    }
                });
                if let Some(error) = sort_err {
                    return Err(error);
                }
                self.write_back_place(recv, CtValue::List(items), scope)?;
                Ok(CtValue::Unit)
            }
            TClosureOp::TakeWhile => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("take_while receiver", self.span()));
                };
                let mut out = Vec::new();
                let (source_pulls, old_tail) = progress
                    .as_ref()
                    .map(|(_, _, _, _, pulls, tail, _, _)| (pulls.clone(), *tail))
                    .unwrap_or_default();
                let mut out_pulls = Vec::new();
                let mut pending = 0usize;
                let mut stopped = false;
                for (index, item) in items.into_iter().enumerate() {
                    pending += source_pulls.get(index).copied().unwrap_or(1);
                    if !as_bool(&calln(self, vec![item.clone()])?, self.span())? {
                        stopped = true;
                        break;
                    }
                    out.push(item);
                    out_pulls.push(pending);
                    pending = 0;
                }
                let tail = if stopped { pending } else { pending + old_tail };
                Ok(wrap_list(out, out_pulls, tail))
            }
            TClosureOp::SkipWhile => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("skip_while receiver", self.span()));
                };
                let mut skipping = true;
                let mut out = Vec::new();
                let (source_pulls, old_tail) = progress
                    .as_ref()
                    .map(|(_, _, _, _, pulls, tail, _, _)| (pulls.clone(), *tail))
                    .unwrap_or_default();
                let mut out_pulls = Vec::new();
                let mut pending = 0usize;
                for (index, item) in items.into_iter().enumerate() {
                    pending += source_pulls.get(index).copied().unwrap_or(1);
                    if skipping {
                        if as_bool(&calln(self, vec![item.clone()])?, self.span())? {
                            continue;
                        }
                        skipping = false;
                    }
                    out.push(item);
                    out_pulls.push(pending);
                    pending = 0;
                }
                Ok(wrap_list(out, out_pulls, pending + old_tail))
            }
            TClosureOp::FlatMap => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("flat_map receiver", self.span()));
                };
                let mut out = Vec::new();
                let (source_pulls, old_tail) = progress
                    .as_ref()
                    .map(|(_, _, _, _, pulls, tail, _, _)| (pulls.clone(), *tail))
                    .unwrap_or_default();
                let mut out_pulls = Vec::new();
                let mut pending = 0usize;
                for (index, item) in items.into_iter().enumerate() {
                    let source_pull = source_pulls.get(index).copied().unwrap_or(1);
                    match calln(self, vec![item])? {
                        CtValue::List(inner) => {
                            if inner.is_empty() {
                                pending += source_pull;
                            } else {
                                let inner_len = inner.len();
                                out_pulls.push(pending + source_pull);
                                pending = 0;
                                out.extend(inner);
                                out_pulls.extend(std::iter::repeat(0).take(inner_len - 1));
                            }
                        }
                        other => {
                            out_pulls.push(pending + source_pull);
                            pending = 0;
                            out.push(other);
                        }
                    }
                }
                Ok(wrap_list(out, out_pulls, pending + old_tail))
            }
            TClosureOp::FilterMap => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("filter_map receiver", self.span()));
                };
                let (source_pulls, old_tail) = progress
                    .as_ref()
                    .map(|(_, _, _, _, pulls, tail, _, _)| (pulls.clone(), *tail))
                    .unwrap_or_default();
                let mut out = Vec::new();
                let mut out_pulls = Vec::new();
                let mut pending = 0usize;
                for (index, item) in items.into_iter().enumerate() {
                    pending += source_pulls.get(index).copied().unwrap_or(1);
                    match calln(self, vec![item])? {
                        CtValue::Present(v) => {
                            out.push(*v);
                            out_pulls.push(pending);
                            pending = 0;
                        }
                        CtValue::Failed(CtReport::Clean(_)) | CtValue::Failed(CtReport::Told(_)) => {}
                        other => {
                            out.push(other);
                            out_pulls.push(pending);
                            pending = 0;
                        }
                    }
                }
                Ok(wrap_list(out, out_pulls, pending + old_tail))
            }
            TClosureOp::Position => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("position receiver", self.span()));
                };
                for (i, item) in items.into_iter().enumerate() {
                    emit_progress_next(
                        &progress,
                        self.sink.as_ref(),
                        &mut progress_cursor,
                        &mut progress_count,
                    );
                    if as_bool(&calln(self, vec![item])?, self.span())? {
                        return Ok(CtValue::Present(Box::new(CtValue::Int(i as i64))));
                    }
                }
                emit_progress_finish(
                    &progress,
                    self.sink.as_ref(),
                    progress_cursor,
                    &mut progress_count,
                );
                Ok(CtValue::absent(crate::AST::Type::Int))
            }
            TClosureOp::OptionMap => match recv_v {
                CtValue::Present(inner) => Ok(CtValue::Present(Box::new(calln(self, vec![*inner])?))),
                CtValue::Failed(CtReport::Clean(t)) => Ok(CtValue::Failed(CtReport::Clean(t))),
                _ => Err(unsupported("option map receiver", self.span())),
            },
            TClosureOp::ParaMap => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("para_map receiver", self.span()));
                };
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(calln(self, vec![item])?);
                }
                let (pulls, tail) = progress_passthrough(&progress, out.len());
                Ok(wrap_list(out, pulls, tail))
            }
            TClosureOp::ParaFilter => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("para_filter receiver", self.span()));
                };
                let (source_pulls, old_tail) = progress
                    .as_ref()
                    .map(|(_, _, _, _, pulls, tail, _, _)| (pulls.clone(), *tail))
                    .unwrap_or_default();
                let mut out = Vec::new();
                let mut out_pulls = Vec::new();
                let mut pending = 0usize;
                for (index, item) in items.into_iter().enumerate() {
                    pending += source_pulls.get(index).copied().unwrap_or(1);
                    let keep = calln(self, vec![item.clone()])?;
                    if as_bool(&keep, self.span())? {
                        out.push(item);
                        out_pulls.push(pending);
                        pending = 0;
                    }
                }
                Ok(wrap_list(out, out_pulls, pending + old_tail))
            }
            TClosureOp::ParaFold => {
                if args.len() < 2 {
                    return Err(unsupported("para_fold arity", self.span()));
                }
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("para_fold receiver", self.span()));
                };
                let mut acc = self.eval_expr_child(&args[0], scope)?;
                for item in items {
                    acc = calln(self, vec![acc, item])?;
                }
                Ok(acc)
            }
            // D-ITERTOOLS1=A: key-selecting reducers (JIT may deopt here; #729 owns native).
            op @ (TClosureOp::MinBy | TClosureOp::MaxBy) => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("min_by/max_by receiver", self.span()));
                };
                let Some(mut best) = items.first().cloned() else {
                    return Ok(CtValue::absent(crate::AST::Type::Named("Any".into())));
                };
                let maximum = matches!(op, TClosureOp::MaxBy);
                let mut best_key = calln(self, vec![best.clone()])?;
                for candidate in items.into_iter().skip(1) {
                    let candidate_key = calln(self, vec![candidate.clone()])?;
                    let order = cmp(best_key.clone(), candidate_key.clone(), self.span())?;
                    if (maximum && order != std::cmp::Ordering::Greater)
                        || (!maximum && order == std::cmp::Ordering::Greater)
                    {
                        best = candidate;
                        best_key = candidate_key;
                    }
                }
                Ok(CtValue::Present(Box::new(best)))
            }
            TClosureOp::CountBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("count_by receiver", self.span()));
                };
                let mut out = std::collections::BTreeMap::new();
                for item in items {
                    let key_v = calln(self, vec![item])?;
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
                for (key, value) in entries {
                    let _ = calln(self, vec![key.to_value(), value])?;
                }
                Ok(CtValue::Unit)
            }
            TClosureOp::MapAny => {
                let CtValue::Map(entries) = recv_v else {
                    return Err(unsupported("map any receiver", self.span()));
                };
                for (key, value) in entries {
                    if as_bool(&calln(self, vec![key.to_value(), value])?, self.span())? {
                        return Ok(CtValue::Bool(true));
                    }
                }
                Ok(CtValue::Bool(false))
            }
            TClosureOp::MapAll => {
                let CtValue::Map(entries) = recv_v else {
                    return Err(unsupported("map all receiver", self.span()));
                };
                for (key, value) in entries {
                    if !as_bool(&calln(self, vec![key.to_value(), value])?, self.span())? {
                        return Ok(CtValue::Bool(false));
                    }
                }
                Ok(CtValue::Bool(true))
            }
            TClosureOp::MapFilter => {
                let CtValue::Map(entries) = recv_v else {
                    return Err(unsupported("map filter receiver", self.span()));
                };
                let mut out = std::collections::BTreeMap::new();
                for (key, value) in entries {
                    if as_bool(&calln(self, vec![key.to_value(), value.clone()])?, self.span())? {
                        out.insert(key, value);
                    }
                }
                Ok(CtValue::Map(out))
            }
            TClosureOp::MapMap => {
                let CtValue::Map(entries) = recv_v else {
                    return Err(unsupported("map map receiver", self.span()));
                };
                let mut out = std::collections::BTreeMap::new();
                for (key, value) in entries {
                    let mapped = calln(self, vec![key.to_value(), value])?;
                    out.insert(key, mapped);
                }
                Ok(CtValue::Map(out))
            }
            TClosureOp::MapFold => {
                if args.len() < 2 {
                    return Err(unsupported("map fold arity", self.span()));
                }
                let CtValue::Map(entries) = recv_v else {
                    return Err(unsupported("map fold receiver", self.span()));
                };
                let mut acc = self.eval_expr_child(&args[0], scope)?;
                for (key, value) in entries {
                    acc = calln(self, vec![acc, key.to_value(), value])?;
                }
                Ok(acc)
            }
            TClosureOp::MapFlatMap => {
                let CtValue::Map(entries) = recv_v else {
                    return Err(unsupported("map flat_map receiver", self.span()));
                };
                let mut out = std::collections::BTreeMap::new();
                for (key, value) in entries {
                    let part = calln(self, vec![key.to_value(), value])?;
                    let CtValue::Map(part) = part else {
                        return Err(unsupported("map flat_map must return map", self.span()));
                    };
                    for (k, v) in part { out.insert(k, v); }
                }
                Ok(CtValue::Map(out))
            }
            TClosureOp::ListBinarySearchBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("binary_search_by receiver", self.span()));
                };
                for (i, item) in items.into_iter().enumerate() {
                    let ord = calln(self, vec![item])?;
                    if matches!(ord, CtValue::Int(0)) {
                        return Ok(CtValue::Present(Box::new(CtValue::Int(i as i64))));
                    }
                }
                Ok(CtValue::absent(crate::AST::Type::Int))
            }
            TClosureOp::ListMinMaxBy { .. } => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("min_max_by receiver", self.span()));
                };
                if items.is_empty() {
                    return Ok(CtValue::absent(crate::AST::Type::Int));
                }
                let mut min_item = items[0].clone();
                let mut max_item = items[0].clone();
                let mut min_key = calln(self, vec![min_item.clone()])?.jet_show();
                let mut max_key = min_key.clone();
                for item in items.into_iter().skip(1) {
                    let key = calln(self, vec![item.clone()])?.jet_show();
                    if key < min_key { min_key = key.clone(); min_item = item.clone(); }
                    if key > max_key { max_key = key; max_item = item; }
                }
                Ok(CtValue::Present(Box::new(CtValue::Struct {
                    type_name: String::new(),
                    fields: vec![("min".into(), min_item), ("max".into(), max_item)],
                })))
            }
            TClosureOp::Partition { .. } | TClosureOp::ParaPartition { .. } => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("partition receiver", self.span()));
                };
                let mut trues = Vec::new();
                let mut falses = Vec::new();
                for item in items {
                    if as_bool(&calln(self, vec![item.clone()])?, self.span())? {
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
                let mut acc = self.eval_expr_child(&args[0], scope)?;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    acc = calln(self, vec![acc, item])?;
                    out.push(acc.clone());
                }
                let (pulls, tail) = progress_passthrough(&progress, out.len());
                Ok(wrap_list(out, pulls, tail))
            }
            TClosureOp::GroupBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("group_by receiver", self.span()));
                };
                let mut out: std::collections::BTreeMap<crate::AST::CtKey, Vec<CtValue>> =
                    std::collections::BTreeMap::new();
                for item in items {
                    let key_v = calln(self, vec![item.clone()])?;
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
            // #1479: mirrors `jet_iter_dedup_by` (Prelude/Core/Collections.rs)
            // — keeps the first item of each consecutive run sharing a key.
            TClosureOp::DedupBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("dedup_by receiver", self.span()));
                };
                let mut out = Vec::new();
                let mut prev_key: Option<CtValue> = None;
                for item in items {
                    let key = calln(self, vec![item.clone()])?;
                    if prev_key.as_ref() == Some(&key) {
                        continue;
                    }
                    prev_key = Some(key);
                    out.push(item);
                }
                let (pulls, tail) = progress_passthrough(&progress, out.len());
                Ok(wrap_list(out, pulls, tail))
            }
            // #1479: mirrors `jet_iter_is_sorted_by` — non-decreasing key
            // order across consecutive elements.
            TClosureOp::IsSortedBy => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("is_sorted_by receiver", self.span()));
                };
                let mut prev_key: Option<CtValue> = None;
                for item in items {
                    let key = calln(self, vec![item.clone()])?;
                    if let Some(prev) = prev_key {
                        if cmp(prev, key.clone(), self.span())? == std::cmp::Ordering::Greater {
                            return Ok(CtValue::Bool(false));
                        }
                    }
                    prev_key = Some(key);
                }
                Ok(CtValue::Bool(true))
            }
            // #1479: mirrors `jet_iter_chunk_while` — groups runs where
            // `f(prev, next)` holds between the chunk's last element and the
            // next candidate.
            TClosureOp::ChunkWhile => {
                let CtValue::List(items) = recv_v else {
                    return Err(unsupported("chunk_while receiver", self.span()));
                };
                let mut chunks: Vec<Vec<CtValue>> = Vec::new();
                for item in items {
                    let start_new = match chunks.last() {
                        Some(chunk) => {
                            let last = chunk.last().cloned().unwrap();
                            !as_bool(
                                &calln(self, vec![last, item.clone()])?,
                                self.span(),
                            )?
                        }
                        None => true,
                    };
                    if start_new {
                        chunks.push(vec![item]);
                    } else {
                        chunks.last_mut().unwrap().push(item);
                    }
                }
                let out: Vec<CtValue> = chunks.into_iter().map(CtValue::List).collect();
                let (pulls, tail) = progress_passthrough(&progress, out.len());
                Ok(wrap_list(out, pulls, tail))
            }
        }
    }

    pub(super) fn apply_callable(
        &mut self,
        callable: &CtValue,
        argv: Vec<CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        // `callable` is the stable `__JetTirCallable` slot returned by
        // `eval_expr`. Do not recover a lambda from the source expression here:
        // `call_callable` snapshots and writes back the same slot's captures,
        // which is the FnMut persistence contract.
        self.call_callable(callable, argv)
    }

    /// Evaluate a callable expression once, then keep calling its canonical
    /// runtime storage. `call_callable` owns slot capture write-back and this
    /// method publishes the same update to the active lexical scope; the cache
    /// keeps a direct lambda from minting a fresh storage slot per item.
    pub(super) fn apply_callable_once(
        &mut self,
        f: &'a TExpr,
        callable: &mut Option<CtValue>,
        argv: Vec<CtValue>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if callable.is_none() {
            *callable = Some(self.eval_expr_child(f, scope)?);
        }
        let result = self.apply_callable(
            callable
                .as_ref()
                .expect("cached callable initialized before call"),
            argv,
        );
        self.sync_callable_captures(
            callable
                .as_ref()
                .expect("cached callable initialized before capture sync"),
            scope,
        );
        result
    }

    pub(super) fn eval_tlambda(
        &mut self,
        lam: &'a TLambda,
        argv: Vec<CtValue>,
        outer: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let mut child = outer.clone();
        for (source, runtime, _) in &lam.captures {
            if runtime != source {
                if let Some(value) = outer.get(source).cloned() {
                    child.insert(runtime.clone(), value);
                }
            }
        }
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
            TLambdaBody::SharedBlock(stmts) => match self.exec_stmts(&stmts[..], &mut child)? {
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
        for (source, runtime, _) in &lam.captures {
            if runtime != source {
                if let Some(value) = child.get(runtime).cloned() {
                    child.insert(source.clone(), value);
                }
            }
        }
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

    pub(super) fn eval_tlambda_mut_arg(
        &mut self,
        lam: &'a TLambda,
        arg: CtValue,
        outer: &mut HashMap<String, CtValue>,
    ) -> Result<(CtValue, CtValue), Diagnostic> {
        let Some(param) = lam.source_params.first() else {
            return Err(unsupported("shared lambda parameter", self.span()));
        };
        let mut child = outer.clone();
        child.insert(param.clone(), arg);
        for (source, runtime, _) in &lam.captures {
            if runtime != source {
                if let Some(value) = outer.get(source).cloned() {
                    child.insert(runtime.clone(), value);
                }
            }
        }
        let result = match &lam.executable {
            TLambdaBody::Expr(expr) => self.eval_expr(expr, &mut child)?,
            TLambdaBody::Block(stmts) => match self.exec_stmts(stmts, &mut child)? {
                Flow::Return(value) => value,
                Flow::Normal => CtValue::Unit,
                other => {
                    return Err(unsupported(
                        &format!("control flow {other:?} escaping shared lambda"),
                        self.span(),
                    ));
                }
            },
            TLambdaBody::SharedBlock(stmts) => match self.exec_stmts(&stmts[..], &mut child)? {
                Flow::Return(value) => value,
                Flow::Normal => CtValue::Unit,
                other => {
                    return Err(unsupported(
                        &format!("control flow {other:?} escaping shared lambda"),
                        self.span(),
                    ));
                }
            },
        };
        for (source, runtime, _) in &lam.captures {
            if runtime != source {
                if let Some(value) = child.get(runtime).cloned() {
                    child.insert(source.clone(), value);
                }
            }
        }
        let updated = child.remove(param).unwrap_or(CtValue::Unit);
        for (name, value) in child {
            if outer.contains_key(&name) {
                outer.insert(name, value);
            }
        }
        Ok((result, updated))
    }
}
