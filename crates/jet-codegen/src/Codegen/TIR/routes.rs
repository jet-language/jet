//! Total checked operation routes shared by every TIR consumer.
//!
//! A route is selected from the checked operation and its checked result facts.
//! This module intentionally contains no backend spelling decisions: symbols are
//! the existing Prelude/runtime entry points, and primitive plans are reserved
//! for operations that the TIR-to-MIR projection expands structurally.

#![allow(dead_code)]
use super::{
    LowerError, TClosureOp, TFailureCarrier, THandleOp, TTryConvert, TNumericOp, TPreludeRoute,
    TRoutePlan, TBuiltinOp, ListRemoveMode, THelperKind, TGcEditKind, TTypedTextForm,
    TTypedTextInterpKind,
};
use crate::AST::{BinOp, Type};
use jet_foundation::Diagnostics::Span;
use jet_foundation::Effects::Effect;
use jet_foundation::MIR::{
    MirAccess, MirAuthorityDecision, MirCallSignature, MirCoreClosureKind,
    MirIndexKind, MirLayoutCompareOp, MirPreludeAbi, MirPreludeFamily, MirSymbol,
};
use jet_foundation::UnitRoundingMode;

fn route_error(message: impl Into<String>) -> LowerError {
    LowerError::new(Span::new(0, 0), message)
}
/// One checked route bundle for the canonical MIR loop protocol.
///
/// TIR selects these rows once; the MIR projection interns each row and stores
/// only its `MirPreludeCallId` on the loop operation.  The execution adapters
/// consume those rows and never infer loop semantics from the operation kind.
#[derive(Clone, Debug)]
pub(super) struct TLoopRouteBundle {
    pub range_init: TPreludeRoute,
    pub range_has_next: TPreludeRoute,
    pub range_value: TPreludeRoute,
    pub range_advance: TPreludeRoute,
    pub iter_init: TPreludeRoute,
    pub iter_has_next: TPreludeRoute,
    pub iter_value: TPreludeRoute,
    pub iter_advance: TPreludeRoute,
}

/// Return the eight internal, infallible loop Prelude rows.
///
/// `core.prelude` is the existing compiler-owned module.  These members are
/// not user-facing Core syntax; they are the shared carrier protocol for the
/// canonical MIR loop operations.
pub(super) fn loop_route_bundle() -> TLoopRouteBundle {
    let carrier = TFailureCarrier::Infallible;
    let row = |member: &str, symbol: &str, arity: usize, borrow_mask: &[bool]| {
        TPreludeRoute {
            family: MirPreludeFamily::StaticPrelude,
            module: "core.prelude".to_string(),
            member: member.to_string(),
            symbol: MirSymbol::Prelude(symbol.to_string()),
            signature: MirCallSignature {
                arity,
                max_arity: arity,
                borrow_mask: borrow_mask.to_vec(),
            },
            effect: None,
            fallibility: carrier.clone(),
            abi: MirPreludeAbi::Value,
            authority: None,
            db_metadata: None,
        }
    };
    TLoopRouteBundle {
        range_init: row(
            "loop_range_init",
            "jet_loop_range_init",
            5,
            &[false, false, false, false, false],
        ),
        range_has_next: row("loop_range_has_next", "jet_loop_range_has_next", 1, &[true]),
        range_value: row("loop_range_value", "jet_loop_range_value", 1, &[true]),
        range_advance: row("loop_range_advance", "jet_loop_range_advance", 1, &[true]),
        iter_init: row(
            "loop_iter_init",
            "jet_loop_iter_init",
            5,
            &[false, false, false, false, false],
        ),
        iter_has_next: row("loop_iter_has_next", "jet_loop_iter_has_next", 1, &[true]),
        iter_value: row("loop_iter_value", "jet_loop_iter_value", 1, &[true]),
        iter_advance: row("loop_iter_advance", "jet_loop_iter_advance", 1, &[true]),
    }
}

pub(super) fn zip_closure_route(mode: super::TZipMode) -> TPreludeRoute {
    let (member, symbol, arity) = match mode {
        super::TZipMode::Short => ("zip", "jet_iter_zip", 3),
        super::TZipMode::Strict => ("zip_strict", "jet_iter_zip_strict", 3),
        super::TZipMode::Pad => ("zip_pad", "jet_iter_zip_pad", 5),
    };
    TPreludeRoute {
        family: MirPreludeFamily::ClosureMethod,
        module: "core.iter".to_string(),
        member: member.to_string(),
        symbol: MirSymbol::Prelude(symbol.to_string()),
        signature: MirCallSignature {
            arity,
            max_arity: arity,
            borrow_mask: vec![false; arity],
        },
        effect: None,
        fallibility: TFailureCarrier::Infallible,
        abi: MirPreludeAbi::Value,
        authority: None,
        db_metadata: None,
    }
}


fn prelude(
    family: MirPreludeFamily,
    module: &str,
    member: &str,
    symbol: &str,
    arity: usize,
    max_arity: usize,
    borrow_mask: &[bool],
    effect: Option<Effect>,
    carrier: &TFailureCarrier,
    abi: MirPreludeAbi,
) -> TRoutePlan {
    TRoutePlan::Prelude(TPreludeRoute {
        family,
        module: module.to_string(),
        member: member.to_string(),
        symbol: MirSymbol::Prelude(symbol.to_string()),
        signature: MirCallSignature {
            arity,
            max_arity,
            borrow_mask: borrow_mask.to_vec(),
        },
        effect,
        fallibility: carrier.clone(),
        abi,
        authority: None,
        db_metadata: None,
    })
}

fn primitive() -> TRoutePlan {
    TRoutePlan::Primitive
}

fn as_prelude(plan: TRoutePlan, name: &str) -> Result<TPreludeRoute, LowerError> {
    match plan {
        TRoutePlan::Prelude(route) => Ok(route),
        TRoutePlan::Primitive => Err(route_error(format!(
            "checked operation `{name}` requires structural MIR lowering"
        ))),
    }
}
fn closure_receiver_kind(receiver: &Type) -> Option<&'static str> {
    match receiver {
        Type::Tagged { inner, .. } => closure_receiver_kind(inner),
        Type::List(_) | Type::FixedList { .. } => Some("list"),
        Type::Apply { name, args }
            if args.len() == 1 && matches!(name.as_str(), "Iter" | "ViewIter") =>
        {
            Some("iter")
        }
        Type::Apply { name, args }
            if args.len() == 1
                && matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut") =>
        {
            Some("view")
        }
        Type::Option(_) => Some("option"),
        Type::Map { .. } => Some("map"),
        Type::Apply { name, .. } if name == crate::Syntax::TYPE_TALLY => Some("bag"),
        _ => None,
    }
}

fn collection_closure_route(
    op: &TClosureOp,
    receiver: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    use TClosureOp::*;
    let kind = closure_receiver_kind(receiver).ok_or_else(|| {
        route_error(format!(
            "checked closure operation `{op:?}` has an unsupported receiver type"
        ))
    })?;
    let (module, member, symbol, arity, borrow_mask) = match (op, kind) {
        (Map, "list") => ("core.list", "map", "jet_list_map", 2, &[false, false][..]),
        (Map, "iter") => ("core.iter", "map", "jet_iter_map", 2, &[false, false][..]),
        (MapMut, "list") => ("core.list", "map_mut", "jet_list_map_mut", 2, &[false, false][..]),
        (MapMut, "iter") => ("core.iter", "map_mut", "jet_iter_map_mut", 2, &[false, false][..]),
        (TryMap, "list") => ("core.list", "try_map", "jet_list_try_map", 2, &[false, false][..]),
        (TryMap, "iter") => ("core.iter", "try_map", "jet_iter_try_map", 2, &[false, false][..]),
        (TryMap, "view") => ("core.view", "try_map", "jet_view_try_map", 2, &[true, false][..]),
        (Filter, "list") => ("core.list", "filter", "jet_list_filter", 2, &[false, false][..]),
        (Filter, "iter") => ("core.iter", "filter", "jet_iter_filter", 2, &[false, false][..]),
        (TryFilter, "list") => ("core.list", "try_filter", "jet_list_try_filter", 2, &[false, false][..]),
        (TryFilter, "iter") => ("core.iter", "try_filter", "jet_iter_try_filter", 2, &[false, false][..]),
        (TryFilter, "view") => ("core.view", "try_filter", "jet_view_try_filter", 2, &[true, false][..]),
        (Each | EachMut, "list" | "iter") => {
            if matches!(op, Each) {
                ("core.list", "each", "jet_list_each", 2, &[false, false][..])
            } else {
                ("core.list", "each_mut", "jet_list_each_mut", 2, &[false, false][..])
            }
        }
        (Find, "list" | "iter") => ("core.list", "find", "jet_list_find", 2, &[false, false][..]),
        (Any, "list" | "iter") => ("core.list", "any", "jet_list_any", 2, &[false, false][..]),
        (All, "list" | "iter") => ("core.list", "all", "jet_list_all", 2, &[false, false][..]),
        (Reduce, "list" | "iter") => ("core.list", "reduce", "jet_list_reduce", 3, &[false, false, false][..]),
        (TakeWhile, "list") => (
            "core.list",
            "take_while",
            "jet_list_take_while_iter",
            2,
            &[false, false][..],
        ),
        (TakeWhile, "iter") => (
            "core.iter",
            "take_while",
            "jet_iter_take_while",
            2,
            &[false, false][..],
        ),
        (SkipWhile, "list") => (
            "core.list",
            "skip_while",
            "jet_list_skip_while_iter",
            2,
            &[false, false][..],
        ),
        (SkipWhile, "iter") => (
            "core.iter",
            "skip_while",
            "jet_iter_skip_while",
            2,
            &[false, false][..],
        ),
        (FlatMap, "list") => ("core.list", "flat_map", "jet_list_flat_map", 2, &[false, false][..]),
        (FlatMap, "iter") => ("core.iter", "flat_map", "jet_iter_flat_map", 2, &[false, false][..]),
        (Position, "list" | "iter") => (
            "core.list",
            "position",
            "jet_list_position",
            2,
            &[false, false][..],
        ),
        (MinBy, "list" | "iter") => ("core.list", "min_by", "jet_list_min_by", 2, &[false, false][..]),
        (MaxBy, "list" | "iter") => ("core.list", "max_by", "jet_list_max_by", 2, &[false, false][..]),
        (GroupBy, "list" | "iter") => (
            "core.list",
            "group_by",
            "jet_list_group_by",
            2,
            &[false, false][..],
        ),
        (CountBy, "list" | "iter") => (
            "core.list",
            "count_by",
            "jet_list_count_by",
            2,
            &[false, false][..],
        ),
        (Scan, "list") => ("core.list", "scan", "jet_list_scan_iter", 3, &[false, false, false][..]),
        (Scan, "iter") => ("core.iter", "scan", "jet_iter_scan", 3, &[false, false, false][..]),
        (OptionMap, "option") => ("core.option", "map", "jet_option_map_ref", 2, &[true, false][..]),
        (BagAny, "bag") => ("core.bag", "any", "jet_bag_any", 2, &[true, false][..]),
        _ => {
            return Err(route_error(format!(
                "checked closure operation `{op:?}` has no Prelude route for receiver `{kind}`"
            )))
        }
    };
    Ok(prelude(
        MirPreludeFamily::ClosureMethod,
        module,
        member,
        symbol,
        arity,
        arity,
        borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
    ))
}


impl TClosureOp {
    pub(super) fn route_plan(
        &self,
        receiver: &Type,
        _result: &Type,
        carrier: &TFailureCarrier,
    ) -> Result<TRoutePlan, LowerError> {
        use TClosureOp::*;
        let plan = match self {
            EditDisjoint => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.collections",
                "edit_disjoint",
                "jet_edit_disjoint",
                3,
                3,
                &[true, true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            EachRef => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "each",
                "jet_list_each_ref",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            EachMap => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.map",
                "each",
                "jet_map_each",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            MapAny => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.map",
                "any",
                "jet_map_any",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            MapAll => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.map",
                "all",
                "jet_map_all",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            MapFilter => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.map",
                "filter",
                "jet_map_filter",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            MapMap => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.map",
                "map",
                "jet_map_map_values",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            MapFold => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.map",
                "fold",
                "jet_map_fold",
                3,
                3,
                &[false, false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            MapFlatMap => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.map",
                "flat_map",
                "jet_map_flat_map",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ListBinarySearchBy => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "binary_search_by",
                "jet_list_binary_search_by",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ListMinMaxBy { .. } => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "min_max_by",
                "jet_list_min_max_by",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Aggregate,
            ),
            CountWhere => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "count_where",
                "jet_list_count_where",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            SortBy => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "sort_by",
                "jet_list_sort_by",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            SortByDesc => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "sort_by_desc",
                "jet_list_sort_by_desc",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            TrySortBy => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "try_sort_by",
                "jet_list_try_sort_by",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            TrySortByDesc => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "try_sort_by_desc",
                "jet_list_try_sort_by_desc",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            SortByCompare => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "sort_by_compare",
                "jet_list_sort_by_compare",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ParaMap => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "para_map",
                "jet_list_para_map",
                3,
                3,
                &[false, false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ParaFilter => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "para_filter",
                "jet_list_para_filter",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ParaPartition { .. } => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "para_partition",
                "jet_list_para_partition",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Aggregate,
            ),
            FloatAddFold { f32 } => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                if *f32 {
                    "fold_add_fixed_f32"
                } else {
                    "fold_add_fixed_f64"
                },
                if *f32 {
                    "jet_list_fold_add_fixed_f32"
                } else {
                    "jet_list_fold_add_fixed_f64"
                },
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            FloatAddParaFold { f32 } => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                if *f32 {
                    "para_fold_add_fixed_f32"
                } else {
                    "para_fold_add_fixed_f64"
                },
                if *f32 {
                    "jet_list_para_fold_add_fixed_f32"
                } else {
                    "jet_list_para_fold_add_fixed_f64"
                },
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ParaFold => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "para_fold",
                "jet_list_para_fold",
                4,
                4,
                &[false, false, false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            FilterMap => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.iter",
                "filter_map",
                "jet_iter_filter_map",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            DedupBy => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.iter",
                "dedup_by",
                "jet_iter_dedup_by",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            IsSortedBy => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.iter",
                "is_sorted_by",
                "jet_iter_is_sorted_by",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ChunkWhile => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.iter",
                "chunk_while",
                "jet_iter_chunk_while",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            Scan => return collection_closure_route(self, receiver, carrier),
            Fold => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "fold",
                "jet_list_fold",
                3,
                3,
                &[false, false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ViewFold => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.view",
                "fold",
                "jet_view_fold",
                3,
                3,
                &[true, false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ViewMap => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.view",
                "map",
                "jet_view_map",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            UpdateFirst => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "update_first",
                "jet_list_update_first",
                3,
                3,
                &[true, false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            Partition { .. } => prelude(
                MirPreludeFamily::ClosureMethod,
                "core.list",
                "partition",
                "jet_list_partition",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Aggregate,
            ),
            // These operations select a List, Iter, View, or Option implementation
            // from the receiver type. The checked projection expands that choice
            // before MIR; no route may guess a helper from the enum alone.
            Map | MapMut | TryMap | Filter | TryFilter | Each | EachMut | Find | Any | BagAny
            | All | Reduce | TakeWhile | SkipWhile | FlatMap | Position | MinBy | MaxBy
            | GroupBy | CountBy | OptionMap => return collection_closure_route(self, receiver, carrier),
        };
        Ok(plan)
    }

    pub(super) fn prelude_route(
        &self,
        receiver: &Type,
        result: &Type,
        carrier: &TFailureCarrier,
    ) -> Result<TPreludeRoute, LowerError> {
        as_prelude(
            self.route_plan(receiver, result, carrier)?,
            "closure method",
        )
    }
}

impl TNumericOp {
    pub(super) fn route_plan(
        &self,
        _result: &Type,
        carrier: &TFailureCarrier,
    ) -> Result<TRoutePlan, LowerError> {
        use TNumericOp::*;
        let plan = match self {
            EuclideanDiv { .. } => prelude(
                MirPreludeFamily::Overflow,
                "core.numeric",
                "div_euclid",
                "jet_std::jet_int_div_euclid",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            EuclideanRem { .. } => prelude(
                MirPreludeFamily::Overflow,
                "core.numeric",
                "rem_euclid",
                "jet_std::jet_int_rem_euclid",
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            Predicate(method) => {
                let (member, symbol) = match method.as_str() {
                    "is_nan" => ("is_nan", "jet_std_math_is_nan"),
                    "is_infinite" => ("is_infinite", "jet_std_math_is_infinite"),
                    "is_finite" => ("is_finite", "jet_std_math_is_finite"),
                    _ => {
                        return Err(route_error(format!(
                            "unknown numeric predicate `{method}`"
                        )))
                    }
                };
                prelude(
                    MirPreludeFamily::BuiltinMethod,
                    "core.math",
                    member,
                    symbol,
                    1,
                    1,
                    &[false],
                    None,
                    carrier,
                    MirPreludeAbi::Value,
                )
            }
            _ => primitive(),
        };
        Ok(plan)
    }

    pub(super) fn prelude_route(
        &self,
        result: &Type,
        carrier: &TFailureCarrier,
    ) -> Result<TPreludeRoute, LowerError> {
        as_prelude(self.route_plan(result, carrier)?, "numeric method")
    }
}

fn event_receiver_kind(receiver: &Type) -> Option<&str> {
    match receiver {
        Type::Tagged { inner, .. } | Type::InlineRange { base: inner, .. } => {
            event_receiver_kind(inner)
        }
        Type::Apply { name, .. } | Type::Named(name)
            if matches!(
                name.as_str(),
                "Event"
                    | "AsyncEvent"
                    | "Hook"
                    | "DecisionHook"
                    | "Subscription"
                    | "EventScope"
                    | "EventTrace"
                    | "DispatchReport"
            ) =>
        {
            Some(name.as_str())
        }
        _ => None,
    }
}

fn event_route(
    receiver: &Type,
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let kind = event_receiver_kind(receiver).ok_or_else(|| {
        route_error(format!(
            "checked event method `{method}` has an unsupported receiver type"
        ))
    })?;
    let (symbol, arity, max_arity, borrow_mask) = match (kind, method) {
        ("Event", "on") => ("jet_std::JetEvent::on", 3, 3, &[true, true, false][..]),
        ("Event", "once") => ("jet_std::JetEvent::once", 3, 3, &[true, true, false][..]),
        ("Event", "on_priority") => {
            ("jet_std::JetEvent::on_priority", 4, 4, &[true, true, false, false][..])
        }
        ("Event", "emit") => ("jet_std::JetEvent::emit", 2, 2, &[true, false][..]),
        ("Event", "listener_count") => {
            ("jet_std::JetEvent::listener_count", 1, 1, &[true][..])
        }
        ("Event", "trace") => ("jet_std::JetEvent::trace", 1, 1, &[true][..]),
        ("AsyncEvent", "on") => {
            ("jet_std::JetAsyncEvent::on", 3, 3, &[true, true, false][..])
        }
        ("AsyncEvent", "once") => {
            ("jet_std::JetAsyncEvent::once", 3, 3, &[true, true, false][..])
        }
        ("AsyncEvent", "on_priority") => (
            "jet_std::JetAsyncEvent::on_priority",
            4,
            4,
            &[true, true, false, false][..],
        ),
        ("AsyncEvent", "emit_async") => {
            ("jet_std::JetAsyncEvent::emit_async", 2, 2, &[true, false][..])
        }
        ("AsyncEvent", "close") => ("jet_std::JetAsyncEvent::close", 1, 1, &[true][..]),
        ("AsyncEvent", "listener_count") => {
            ("jet_std::JetAsyncEvent::listener_count", 1, 1, &[true][..])
        }
        ("AsyncEvent", "queued_count") => {
            ("jet_std::JetAsyncEvent::queued_count", 1, 1, &[true][..])
        }
        ("AsyncEvent", "running_count") => {
            ("jet_std::JetAsyncEvent::running_count", 1, 1, &[true][..])
        }
        ("AsyncEvent", "blocked_count") => {
            ("jet_std::JetAsyncEvent::blocked_count", 1, 1, &[true][..])
        }
        ("Hook", "on") => ("jet_std::JetHook::on", 3, 3, &[true, true, false][..]),
        ("Hook", "once") => ("jet_std::JetHook::once", 3, 3, &[true, true, false][..]),
        ("Hook", "on_priority") => {
            ("jet_std::JetHook::on_priority", 4, 4, &[true, true, false, false][..])
        }
        ("Hook", "run") => ("jet_std::JetHook::run", 3, 3, &[true, false, false][..]),
        ("Hook", "listener_count") => {
            ("jet_std::JetHook::listener_count", 1, 1, &[true][..])
        }
        ("Hook", "trace") => ("jet_std::JetHook::trace", 1, 1, &[true][..]),
        ("DecisionHook", "on") => {
            ("jet_std::JetDecisionHook::on", 3, 3, &[true, true, false][..])
        }
        ("DecisionHook", "once") => {
            ("jet_std::JetDecisionHook::once", 3, 3, &[true, true, false][..])
        }
        ("DecisionHook", "on_priority") => (
            "jet_std::JetDecisionHook::on_priority",
            4,
            4,
            &[true, true, false, false][..],
        ),
        ("DecisionHook", "run") => {
            ("jet_std::JetDecisionHook::run", 2, 2, &[true, false][..])
        }
        ("DecisionHook", "listener_count") => {
            ("jet_std::JetDecisionHook::listener_count", 1, 1, &[true][..])
        }
        ("Subscription", "unsubscribe") => {
            ("jet_std::JetSubscription::unsubscribe", 1, 1, &[true][..])
        }
        ("Subscription", "is_active") => {
            ("jet_std::JetSubscription::active", 1, 1, &[true][..])
        }
        ("EventScope", "cancel") => {
            ("jet_std::JetEventScope::cancel", 1, 1, &[true][..])
        }
        ("EventScope", "active_count") => {
            ("jet_std::JetEventScope::active_count", 1, 1, &[true][..])
        }
        ("EventTrace", "summary") => {
            ("jet_std::JetEventTrace::summary", 1, 1, &[true][..])
        }
        ("EventTrace", "delivered") => {
            ("jet_std::JetEventTrace::delivered", 1, 1, &[true][..])
        }
        ("EventTrace", "queued") => {
            ("jet_std::JetEventTrace::queued", 1, 1, &[true][..])
        }
        ("EventTrace", "dropped") => {
            ("jet_std::JetEventTrace::dropped", 1, 1, &[true][..])
        }
        ("DispatchReport", "state") => {
            ("jet_std::JetDispatchReport::state", 1, 1, &[true][..])
        }
        ("DispatchReport", "accepted") => {
            ("jet_std::JetDispatchReport::accepted", 1, 1, &[true][..])
        }
        ("DispatchReport", "delivered_handlers") => {
            ("jet_std::JetDispatchReport::delivered_handlers", 1, 1, &[true][..])
        }
        ("DispatchReport", "failures") => {
            ("jet_std::JetDispatchReport::failures", 1, 1, &[true][..])
        }
        ("DispatchReport", "trace") => {
            ("jet_std::JetDispatchReport::trace", 1, 1, &[true][..])
        }
        _ => {
            return Err(route_error(format!(
                "unknown checked event method `{kind}.{method}`"
            )))
        }
    };
    Ok(prelude(
        MirPreludeFamily::ClosureMethod,
        "core.event",
        method,
        symbol,
        arity,
        max_arity,
        borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
    ))
}
fn task_receiver_args(receiver: &Type) -> Option<&[Type]> {
    match receiver {
        Type::Tagged { inner, .. } | Type::InlineRange { base: inner, .. } => {
            task_receiver_args(inner)
        }
        Type::Apply { name, args } if name == "Task" => Some(args.as_slice()),
        _ => None,
    }
}

fn task_route(
    receiver: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let args = task_receiver_args(receiver)
        .ok_or_else(|| route_error("checked task join has an unsupported receiver type"))?;
    let task_value = args
        .first()
        .ok_or_else(|| route_error("checked task join is missing its value type"))?;
    // `Task<Result<T, E>>` is the private carrier used by fallible task
    // bodies. The shared Prelude helper removes that carrier before exposing
    // the public `Result<T, TaskFailure>` join surface.
    let symbol = if matches!(task_value, Type::Result { .. }) {
        "jet_std::jet_task_join_result"
    } else {
        "jet_std::JetTask::join"
    };
    Ok(prelude(
        MirPreludeFamily::HandleMethod,
        "core.tasks",
        "join",
        symbol,
        1,
        1,
        &[false],
        None,
        carrier,
        MirPreludeAbi::Value,
    ))
}



impl THandleOp {
    pub(super) fn route_plan(
        &self,
        receiver: &Type,
        carrier: &TFailureCarrier,
    ) -> Result<TRoutePlan, LowerError> {
        use THandleOp::*;
        let plan = match self {
            Hardware(op) => hardware_route(op, carrier),
            ReceiptAttach => prelude(
                MirPreludeFamily::HandleMethod,
                "receipt",
                "attach",
                "jet_receipt_attach",
                4,
                4,
                &[true, false, false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            DurationNew { float, .. } => prelude(
                MirPreludeFamily::HandleMethod,
                "core.time",
                "duration_new",
                if *float { "jet_duration_from_float" } else { "jet_duration_from_int" },
                2,
                2,
                &[false, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            FileReaderReadLine => h("file_reader.read_line", "jet_std_file_reader_read_line", 1, 1, &[true], Some(Effect::FS), carrier),
            MappedFileWindow => h("mapped_file.window", "jet_std_fs_map_window_view", 3, 3, &[true, false, false], Some(Effect::FS), carrier),
            MappedFileWindowLen => h("mapped_file.window_len", "jet_std_fs_map_window_len_view", 3, 3, &[true, false, false], Some(Effect::FS), carrier),
            MappedFileLines => h("mapped_file.lines", "jet_std_fs_map_lines_view", 1, 1, &[true], Some(Effect::FS), carrier),
            MappedFileLen => h("mapped_file.len", "jet_std_fs_map_len", 1, 1, &[true], None, carrier),
            MappedFileIsEmpty => h("mapped_file.is_empty", "jet_std_fs_map_is_empty", 1, 1, &[true], None, carrier),
            FileWriterWriteLine => h("file_writer.write_line", "jet_std_file_writer_write_line", 2, 2, &[true, true], Some(Effect::FS), carrier),
            FileWriterFlush => h("file_writer.flush", "jet_std_file_writer_flush", 1, 1, &[true], Some(Effect::FS), carrier),
            JSONReaderNext => h("json_reader.next", "jet_enc_json_reader_next", 1, 1, &[true], Some(Effect::IO), carrier),
            JSONWriterWrite => h("json_writer.write", "jet_enc_json_writer_write", 2, 2, &[true, false], Some(Effect::IO), carrier),
            JSONWriterFlush => h("json_writer.flush", "jet_enc_json_writer_flush", 1, 1, &[true], Some(Effect::IO), carrier),
            JSONWriterFinish => h("json_writer.finish", "jet_enc_json_writer_finish", 1, 1, &[true], Some(Effect::IO), carrier),
            JSONLReaderNext => h("jsonl_reader.next", "jet_enc_jsonl_reader_next", 1, 1, &[true], Some(Effect::IO), carrier),
            JSONLWriterWrite => h("jsonl_writer.write", "jet_enc_jsonl_writer_write", 2, 2, &[true, false], Some(Effect::IO), carrier),
            JSONLWriterFlush => h("jsonl_writer.flush", "jet_enc_jsonl_writer_flush", 1, 1, &[true], Some(Effect::IO), carrier),
            JSONLWriterFinish => h("jsonl_writer.finish", "jet_enc_jsonl_writer_finish", 1, 1, &[true], Some(Effect::IO), carrier),
            CSVReaderNext => h("csv_reader.next", "jet_enc_csv_reader_next", 1, 1, &[true], Some(Effect::IO), carrier),
            DataStreamNext => h("data_stream.next", "jet_data_stream_next", 1, 1, &[true], Some(Effect::IO), carrier),
            StreamWithEventTime => h(
                "stream.with_event_time",
                "jet_stream_with_event_time",
                2,
                2,
                &[false, false],
                Some(Effect::IO),
                carrier,
            ),
            StreamWithEventTimeNs => h(
                "stream.with_event_time.datetime",
                "jet_stream_with_event_time_ns",
                2,
                2,
                &[false, false],
                Some(Effect::IO),
                carrier,
            ),
            StreamKeyBy => h(
                "stream.key_by",
                "jet_stream_key_by",
                2,
                2,
                &[false, false],
                Some(Effect::IO),
                carrier,
            ),
            StreamWindow => h(
                "stream.window",
                "jet_keyed_stream_window",
                4,
                4,
                &[false, false, false, false],
                Some(Effect::IO),
                carrier,
            ),
            XMLReaderNext => h("xml_reader.next", "jet_enc_xml_reader_next", 1, 1, &[true], Some(Effect::IO), carrier),
            XMLWriterWrite => h("xml_writer.write", "jet_enc_xml_writer_write", 2, 2, &[true, false], Some(Effect::IO), carrier),
            XMLWriterFlush => h("xml_writer.flush", "jet_enc_xml_writer_flush", 1, 1, &[true], Some(Effect::IO), carrier),
            XMLWriterFinish => h("xml_writer.finish", "jet_enc_xml_writer_finish", 1, 1, &[true], Some(Effect::IO), carrier),
            CSVWriterWrite => h("csv_writer.write", "jet_enc_csv_writer_write", 2, 2, &[true, false], Some(Effect::IO), carrier),
            CSVWriterFlush => h("csv_writer.flush", "jet_enc_csv_writer_flush", 1, 1, &[true], Some(Effect::IO), carrier),
            CSVWriterFinish => h("csv_writer.finish", "jet_enc_csv_writer_finish", 1, 1, &[true], Some(Effect::IO), carrier),
            CBORReaderNext => h("cbor_reader.next", "jet_enc_cbor_reader_next", 1, 1, &[true], Some(Effect::IO), carrier),
            CBORWriterWrite => h("cbor_writer.write", "jet_enc_cbor_writer_write", 2, 2, &[true, false], Some(Effect::IO), carrier),
            CBORWriterFlush => h("cbor_writer.flush", "jet_enc_cbor_writer_flush", 1, 1, &[true], Some(Effect::IO), carrier),
            CBORWriterFinish => h("cbor_writer.finish", "jet_enc_cbor_writer_finish", 1, 1, &[true], Some(Effect::IO), carrier),
            StdinReadLine => h("stdin.read_line", "jet_std_io_stdin_read_line", 1, 1, &[true], Some(Effect::IO), carrier),
            StdoutWrite => h("stdout.write", "jet_std_io_stdout_write", 2, 2, &[true, true], Some(Effect::IO), carrier),
            StdoutWriteLine => h("stdout.write_line", "jet_std_io_stdout_write_line", 2, 2, &[true, true], Some(Effect::IO), carrier),
            StdoutWriteBytes => h("stdout.write_bytes", "jet_std_io_stdout_write_bytes", 2, 2, &[true, true], Some(Effect::IO), carrier),
            StdoutFlush => h("stdout.flush", "jet_std_io_stdout_flush", 1, 1, &[true], Some(Effect::IO), carrier),
            StdoutIsTty => h("stdout.is_tty", "jet_std_io_stdout_is_tty", 1, 1, &[true], Some(Effect::IO), carrier),
            StderrWrite => h("stderr.write", "jet_std_io_stderr_write", 2, 2, &[true, true], Some(Effect::IO), carrier),
            StderrWriteLine => h("stderr.write_line", "jet_std_io_stderr_write_line", 2, 2, &[true, true], Some(Effect::IO), carrier),
            StderrWriteBytes => h("stderr.write_bytes", "jet_std_io_stderr_write_bytes", 2, 2, &[true, true], Some(Effect::IO), carrier),
            StderrFlush => h("stderr.flush", "jet_std_io_stderr_flush", 1, 1, &[true], Some(Effect::IO), carrier),
            StderrIsTty => h("stderr.is_tty", "jet_std_io_stderr_is_tty", 1, 1, &[true], Some(Effect::IO), carrier),
            StopwatchElapsedMillis => h("stopwatch.elapsed_millis", "jet_stopwatch_elapsed_millis", 1, 1, &[true], Some(Effect::Time), carrier),
            TestSuiteRun => h("test_suite.run", "jet_test_suite_run", 1, 1, &[true], None, carrier),
            ClockNow => h("clock.now", "jet_clock_now", 1, 1, &[true], Some(Effect::Time), carrier),
            ClockTick => h("clock.tick", "jet_clock_tick", 2, 2, &[true, false], Some(Effect::Time), carrier),
            ClockAdvance => h("clock.advance", "jet_clock_advance", 2, 2, &[true, false], Some(Effect::Time), carrier),
            ClockWait => h("clock.wait", "jet_clock_wait", 2, 2, &[true, true], Some(Effect::Time), carrier),
            WorldNow => h("deterministic_world.now", "jet_world_now", 1, 1, &[true], Some(Effect::Time), carrier),
            WorldAdvance => h("deterministic_world.advance", "jet_world_advance", 2, 2, &[true, true], Some(Effect::Time), carrier),
            WorldWaitIdle => h("deterministic_world.wait_idle", "jet_world_wait_idle", 1, 1, &[true], Some(Effect::Time), carrier),
            WorldHistory => h("deterministic_world.history", "jet_world_history", 1, 1, &[true], None, carrier),
            RealtimeNextDeadline => h("realtime.next_deadline", "jet_rt_next_deadline", 1, 1, &[true], None, carrier),
            RealtimeReceipt => h("realtime.receipt", "jet_rt_receipt", 1, 1, &[true], None, carrier),
            RealtimeCancel => h("realtime.cancel", "jet_rt_cancel", 1, 1, &[false], Some(Effect::Time), carrier),
            RealtimeIsCancelled => h("realtime.is_cancelled", "jet_rt_is_cancelled", 1, 1, &[true], None, carrier),
            RngInt => h("rng.int", "jet_rng_int", 3, 3, &[true, false, false], Some(Effect::Rand), carrier),
            RngFloat => h("rng.float", "jet_rng_float", 1, 1, &[true], Some(Effect::Rand), carrier),
            RngFloatRange => h("rng.float_range", "jet_rng_float_range", 3, 3, &[true, false, false], Some(Effect::Rand), carrier),
            RngBool => h("rng.bool", "jet_rng_bool", 1, 1, &[true], Some(Effect::Rand), carrier),
            RngBoolP => h("rng.bool_p", "jet_rng_bool_p", 2, 2, &[true, false], Some(Effect::Rand), carrier),
            RngNormal => h("rng.normal", "jet_rng_normal", 3, 3, &[true, false, false], Some(Effect::Rand), carrier),
            RngExponential => h("rng.exponential", "jet_rng_exponential", 2, 2, &[true, false], Some(Effect::Rand), carrier),
            RngBytes => h("rng.bytes", "jet_rng_bytes", 2, 2, &[true, false], Some(Effect::Rand), carrier),
            RngSplit => h("rng.split", "jet_rng_split", 1, 1, &[true], Some(Effect::Rand), carrier),
            RngPick => h("rng.pick", "jet_rng_pick", 2, 2, &[true, true], Some(Effect::Rand), carrier),
            RngWeightedPick => h("rng.weighted_pick", "jet_rng_weighted_pick", 3, 3, &[true, true, true], Some(Effect::Rand), carrier),
            RngSample => h("rng.sample", "jet_rng_sample", 3, 3, &[true, true, false], Some(Effect::Rand), carrier),
            RngShuffle => h("rng.shuffle", "jet_rng_shuffle", 2, 2, &[true, true], Some(Effect::Rand), carrier),
            HistoryRngNextU64 => h("history_rng.next_u64", "jet_testing_history_rng_next_u64", 1, 1, &[true], Some(Effect::Rand), carrier),
            HistoryRngBelow => h("history_rng.below", "jet_testing_history_rng_below", 2, 2, &[true, false], Some(Effect::Rand), carrier),
            FakeLocale => h("fake.locale", "jet_fake_locale", 2, 2, &[true, true], Some(Effect::Rand), carrier),
            FakeName => h("fake.name", "jet_fake_name", 1, 1, &[true], Some(Effect::Rand), carrier),
            FakeEmail => h("fake.email", "jet_fake_email", 1, 1, &[true], Some(Effect::Rand), carrier),
            FakeHost => h("fake.host", "jet_fake_host", 1, 1, &[true], Some(Effect::Rand), carrier),
            FakeAddress => h("fake.address", "jet_fake_address", 1, 1, &[true], Some(Effect::Rand), carrier),
            SolverNew => h("solver.new", "jet_solver_new", 1, 1, &[false], None, carrier),
            SolverRequire => h("solver.require", "jet_solver_require", 2, 2, &[true, false], None, carrier),
            SolverFailureCount => h("solver.failure_count", "jet_solver_failure_count", 1, 1, &[true], None, carrier),
            SolverStatus => h("solver.status", "jet_solver_status", 1, 1, &[true], None, carrier),
            GameSceneNew => h("game.scene_new", "jet_game_scene_new", 1, 1, &[true], None, carrier),
            GameReplayRecord => h("game.replay_record", "jet_game_replay_record", 1, 1, &[true], None, carrier),
            GameBackendHeadless => prelude(MirPreludeFamily::HandleMethod, "core.game", "backend_headless", "jet_game_backend_headless", 0, 0, &[], None, carrier, MirPreludeAbi::Value),
            GameBackendShouldContinue => h("game.backend_should_continue", "jet_game_backend_should_continue", 1, 1, &[true], None, carrier),
            GameBackendPresent => h("game.backend_present", "jet_game_backend_present", 1, 1, &[true], None, carrier),
            GameSceneOnFrame { .. } => h("game.scene_on_frame", "jet_game_scene_on_frame", 4, 4, &[true, false, false, false], None, carrier),
            GameSceneComponent => h("game.scene_component", "jet_game_scene_component", 2, 2, &[true, true], None, carrier),
            GameSceneQuery => h("game.scene_query", "jet_game_scene_query", 2, 2, &[true, true], None, carrier),
            GameAssetsImage => h("game.assets_image", "jet_game_assets_image", 2, 2, &[true, true], Some(Effect::FS), carrier),
            GameAssetsSound => h("game.assets_sound", "jet_game_assets_sound", 2, 2, &[true, true], Some(Effect::FS), carrier),
            GameInputBind => h("game.input_bind", "jet_game_input_bind", 3, 3, &[true, true, true], None, carrier),
            GameInputPressed => h("game.input_pressed", "jet_game_input_pressed", 2, 2, &[true, true], None, carrier),
            DurationIn { .. } => h("duration.in", "jet_duration_in", 2, 2, &[true, true], None, carrier),
            DurationIsZero => h("duration.is_zero", "jet_duration_is_zero", 1, 1, &[true], None, carrier),
            DurationTotalSeconds => h("duration.total_seconds", "jet_duration_total_seconds", 1, 1, &[true], None, carrier),
            DurationDifference => h("duration.difference", "jet_duration_difference", 2, 2, &[true, true], None, carrier),
            DurationAbs => h("duration.abs", "jet_duration_abs", 1, 1, &[true], None, carrier),
            DurationNegated => h("duration.negated", "jet_duration_negated", 1, 1, &[true], None, carrier),
            DurationSign => h("duration.sign", "jet_duration_sign", 1, 1, &[true], None, carrier),
            DurationTotalIn => h("duration.total_in", "jet_duration_total_in", 2, 2, &[true, true], None, carrier),
            DurationRound => h("duration.round", "jet_duration_round", 4, 4, &[true, true, true, true], None, carrier),
            DurationSecondsValue => h("duration.seconds_value", "jet_duration_seconds_value", 1, 1, &[true], None, carrier),
            DurationScale => h("duration.scale", "jet_duration_scale", 2, 2, &[true, true], None, carrier),
            DurationDivide => h("duration.divide", "jet_duration_divide", 2, 2, &[true, true], None, carrier),
            TcpListenerAccept => h("tcp_listener.accept", "jet_net_tcp_accept", 1, 2, &[true, true], Some(Effect::Net), carrier),
            TcpListenerLocalAddr => h("tcp_listener.local_addr", "jet_net_listener_local_addr", 1, 1, &[true], Some(Effect::Net), carrier),
            TcpStreamRead => h("tcp_stream.read", "jet_net_tcp_read", 1, 1, &[true], Some(Effect::Net), carrier),
            TcpStreamWrite => h("tcp_stream.write", "jet_net_tcp_write", 2, 2, &[true, true], Some(Effect::Net), carrier),
            TcpStreamPeerAddr => h("tcp_stream.peer_addr", "jet_net_tcp_peer_addr", 1, 1, &[true], Some(Effect::Net), carrier),
            TcpStreamLocalAddr => h("tcp_stream.local_addr", "jet_net_tcp_local_addr", 1, 1, &[true], Some(Effect::Net), carrier),
            TcpStreamClose => h("tcp_stream.close", "jet_net_tcp_close", 1, 1, &[true], Some(Effect::Net), carrier),
            TcpStreamReadBytes => h("tcp_stream.read_bytes", "jet_net_tcp_read_bytes", 2, 3, &[true, false, true], Some(Effect::Net), carrier),
            TcpStreamReadText => h("tcp_stream.read_text", "jet_net_tcp_read_text", 2, 3, &[true, false, true], Some(Effect::Net), carrier),
            TcpStreamWriteBytes => h("tcp_stream.write_bytes", "jet_net_tcp_write_bytes", 2, 3, &[true, true, true], Some(Effect::Net), carrier),
            TcpStreamWriteAllBytes => h("tcp_stream.write_all_bytes", "jet_net_tcp_write_all_bytes", 2, 3, &[true, true, true], Some(Effect::Net), carrier),
            TcpStreamWriteText => h("tcp_stream.write_text", "jet_net_tcp_write_text", 2, 3, &[true, true, true], Some(Effect::Net), carrier),
            TcpStreamShutdown => h("tcp_stream.shutdown", "jet_net_tcp_shutdown", 2, 2, &[true, false], Some(Effect::Net), carrier),
            TcpStreamReady => h("tcp_stream.ready", "jet_net_tcp_ready_deadline", 3, 3, &[true, false, true], Some(Effect::Net), carrier),
            UdpSocketReady => h("udp_socket.ready", "jet_net_udp_ready", 3, 3, &[true, false, true], Some(Effect::Net), carrier),
            UdpSocketClose => h("udp_socket.close", "jet_net_udp_close", 1, 1, &[true], Some(Effect::Net), carrier),
            UdpSocketReceiveDeadline => h("udp_socket.receive_deadline", "jet_net_udp_receive_deadline", 3, 3, &[true, false, true], Some(Effect::Net), carrier),
            UdpSocketSendToDeadline => h("udp_socket.send_to_deadline", "jet_net_udp_send_bytes_to_deadline", 4, 4, &[true, true, true, true], Some(Effect::Net), carrier),
            UnixListenerAcceptDeadline => h("unix_listener.accept_deadline", "jet_net_unix_accept_deadline", 2, 2, &[true, true], Some(Effect::Net), carrier),
            UnixStreamReadDeadline => h("unix_stream.read_deadline", "jet_net_unix_read_bytes_deadline", 3, 3, &[true, false, true], Some(Effect::Net), carrier),
            UnixStreamWriteAllDeadline => h("unix_stream.write_all_deadline", "jet_net_unix_write_all_bytes_deadline", 3, 3, &[true, true, true], Some(Effect::Net), carrier),
            UnixStreamReady => h("unix_stream.ready", "jet_net_unix_ready", 3, 3, &[true, false, true], Some(Effect::Net), carrier),
            UnixStreamClose => h("unix_stream.close", "jet_net_unix_close", 1, 1, &[true], Some(Effect::Net), carrier),
            UnixStreamSetTimeout => h("unix_stream.set_timeout", "jet_net_unix_set_timeout", 2, 2, &[true, true], Some(Effect::Net), carrier),
            TLSStreamReadDeadline => h("tls_stream.read_deadline", "jet_net_tls_read_bytes_deadline", 3, 3, &[true, false, true], Some(Effect::Net), carrier),
            TLSStreamWriteAllDeadline => h("tls_stream.write_all_deadline", "jet_net_tls_write_all_bytes_deadline", 3, 3, &[true, true, true], Some(Effect::Net), carrier),
            TLSStreamReady => h("tls_stream.ready", "jet_net_tls_ready", 3, 3, &[true, false, true], Some(Effect::Net), carrier),
            TLSStreamClose => h("tls_stream.close", "jet_net_tls_close", 1, 1, &[true], Some(Effect::Net), carrier),
            TLSStreamCloseWrite => h("tls_stream.close_write", "jet_net_tls_close_write", 2, 2, &[true, true], Some(Effect::Net), carrier),
            TLSStreamPeerIdentity => h("tls_stream.peer_identity", "jet_net_tls_peer_identity", 1, 1, &[true], Some(Effect::Net), carrier),
            TLSClientConfigDefault => h("tls.config_default", "jet_tls_client_config_default", 0, 0, &[], Some(Effect::Net), carrier),
            TLSClientConfigWithAlpn => h("tls.config_with_alpn", "jet_tls_client_config_with_alpn", 2, 2, &[false, true], Some(Effect::Net), carrier),
            TLSRootCertificatesFromPem => h("tls.root_certificates_from_pem", "jet_tls_root_certificates_from_pem", 1, 1, &[true], Some(Effect::Net), carrier),
            TLSClientIdentityFromPem => h("tls.client_identity_from_pem", "jet_tls_client_identity_from_pem", 2, 2, &[true, true], Some(Effect::Net), carrier),
            TLSClientConfigWithTrust => h("tls.config_with_trust", "jet_tls_client_config_with_trust", 2, 2, &[false, false], Some(Effect::Net), carrier),
            TLSClientConfigWithIdentity => h("tls.config_with_identity", "jet_tls_client_config_with_client_identity", 2, 2, &[false, true], Some(Effect::Net), carrier),
            HTTPClientNew => h("http.client_new", "jet_http_client_new_impl", 0, 0, &[], Some(Effect::Net), carrier),
            TLSClientConfigWithVersionBounds => h("tls.config_with_version_bounds", "jet_tls_client_config_with_version_bounds", 3, 3, &[false, false, false], Some(Effect::Net), carrier),
            HTTPReqParam => h("http.request_param", "jet_http_request_param", 2, 2, &[true, true], Some(Effect::Net), carrier),
            HTTPReqHeader => h("http.request_header", "jet_http_srv_req_header", 2, 2, &[true, true], None, carrier),
            HTTPReqTrailers => h("http.request_trailers", "jet_http_srv_req_trailers", 1, 1, &[true], Some(Effect::Net), carrier),
            ArgsSpecFlag => h("args.flag", "jet_args_flag", 3, 3, &[false, true, true], Some(Effect::Env), carrier),
            ArgsSpecFlagShort => h("args.flag_short", "jet_args_flag_short", 4, 4, &[false, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecOption => h("args.option", "jet_args_option", 4, 4, &[false, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecOptionShort => h("args.option_short", "jet_args_option_short", 5, 5, &[false, true, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecOptionDefault => h("args.option_default", "jet_args_option_default", 5, 5, &[false, true, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecOptionEnv => h("args.option_env", "jet_args_option_env", 5, 5, &[false, true, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecOptionInt => h("args.option_int", "jet_args_option_int", 4, 4, &[false, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecOptionFloat => h("args.option_float", "jet_args_option_float", 4, 4, &[false, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecOptionChoice => h("args.option_choice", "jet_args_option_choice", 5, 5, &[false, true, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecRepeat => h("args.repeat", "jet_args_repeat", 4, 4, &[false, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecRequiredOption => h("args.required_option", "jet_args_required_option", 4, 4, &[false, true, true, true], Some(Effect::Env), carrier),
            ArgsSpecPositional => h("args.positional", "jet_args_positional", 3, 3, &[false, true, true], Some(Effect::Env), carrier),
            ArgsSpecDescription => h("args.description", "jet_args_description", 2, 2, &[false, true], Some(Effect::Env), carrier),
            ArgsSpecSubcommand => h("args.subcommand", "jet_args_subcommand", 4, 4, &[false, true, true, false], Some(Effect::Env), carrier),
            ArgsSpecVersion => h("args.version", "jet_args_version", 2, 2, &[false, true], Some(Effect::Env), carrier),
            ArgsSpecCompletion => h("args.completion", "jet_args_completion", 2, 2, &[true, true], Some(Effect::Env), carrier),
            ArgsSpecParse => h("args.parse", "jet_args_parse", 2, 2, &[true, true], Some(Effect::Env), carrier),
            ArgsSpecParseOrExit => h("args.parse_or_exit", "jet_args_parse_or_exit", 2, 2, &[true, true], Some(Effect::Env), carrier),
            ParsedArgsFlag => h("parsed.flag", "jet_parsed_flag", 2, 2, &[true, true], Some(Effect::Env), carrier),
            ParsedArgsOption => h("parsed.option", "jet_parsed_option", 2, 2, &[true, true], Some(Effect::Env), carrier),
            ParsedArgsOptionInt => h("parsed.option_int", "jet_parsed_option_int", 2, 2, &[true, true], Some(Effect::Env), carrier),
            ParsedArgsOptionFloat => h("parsed.option_float", "jet_parsed_option_float", 2, 2, &[true, true], Some(Effect::Env), carrier),
            ParsedArgsOptions => h("parsed.options", "jet_parsed_options", 2, 2, &[true, true], Some(Effect::Env), carrier),
            ParsedArgsPositional => h("parsed.positional", "jet_parsed_positional", 2, 2, &[true, false], Some(Effect::Env), carrier),
            ParsedArgsSubcommand => h("parsed.subcommand", "jet_parsed_subcommand", 1, 1, &[true], Some(Effect::Env), carrier),
            TerminalSessionResize => h("terminal.resize", "jet_terminal_session_resize", 2, 2, &[true, true], Some(Effect::IO), carrier),
            ProcessStdinWrite => h("process.stdin_write", "jet_process_stdin_write", 2, 2, &[true, true], Some(Effect::Exec), carrier),
            ProcessStdinClose => h("process.stdin_close", "jet_process_stdin_close", 1, 1, &[true], Some(Effect::Exec), carrier),
            ExpiringMethod { method } => match method.as_str() {
                "get" => h("expiring.get", "jet_expiring_get", 2, 2, &[true, true], Some(Effect::Time), carrier),
                "is_valid" => primitive(),
                _ => return Err(route_error(format!("unknown checked ExpiringValue method `{method}`"))),
            },
            HTTPServerMethod { kind, method } if kind == "HTTPServer" => match method.as_str() {
                "local_addr" => h("http_server.local_addr", "jet_http_server_local_addr", 1, 1, &[true], Some(Effect::Net), carrier),
                "serve" => h("http_server.serve", "jet_http_server_serve", 1, 1, &[true], Some(Effect::Net), carrier),
                "wait" => h("http_server.wait", "jet_http_server_wait", 1, 1, &[true], Some(Effect::Net), carrier),
                "shutdown" => h("http_server.shutdown", "jet_http_server_shutdown", 2, 2, &[true, true], Some(Effect::Net), carrier),
                _ => primitive(),
            },
            HTTPServerMethod { .. } | HTTPClientMethod { .. } | EmailMethod { .. } => primitive(),
            RegexMethod { kind, method } => regex_route(kind, method, carrier)?,
            UrlMimeMethod { kind, method } => url_mime_route(kind, method, carrier)?,
            SketchMethod { sketch, method } => {
                let (arity, borrow_mask): (usize, &[bool]) = match (sketch.as_str(), method.as_str()) {
                    ("HyperLogLog", "add") | ("CountMinSketch", "add" | "count") => (2, &[true, true]),
                    ("HyperLogLog", "count") | ("ReservoirSampler", "sample") => (1, &[true]),
                    ("TDigest", "add" | "quantile") | ("ReservoirSampler", "add") => (2, &[true, false]),
                    _ => return Err(route_error(format!("unknown checked sketch method `{sketch}.{method}`"))),
                };
                h(&format!("{sketch}.{method}"), &format!("Jet{sketch}::{method}"), arity, arity, borrow_mask, None, carrier)
            }
            CivilTimeMethod { kind, method } => civil_time_route(kind, method, carrier)?,
            ProcessSpecMethod { method } => match method.as_str() {
                "stdin" => h(
                    "process.spec.stdin",
                    "jet_process_spec_stdin",
                    2,
                    2,
                    &[true, true],
                    Some(Effect::Exec),
                    carrier,
                ),
                "spawn" => h(
                    "process.spec.spawn",
                    "jet_process_spec_spawn",
                    1,
                    1,
                    &[true],
                    Some(Effect::Exec),
                    carrier,
                ),
                _ => primitive(),
            },
            ProcessChildMethod { method } if method == "wait" => h(
                "process.child.wait",
                "jet_process_child_wait",
                1,
                1,
                &[true],
                Some(Effect::Exec),
                carrier,
            ),
            ProcessChildMethod { .. } => primitive(),
            PathFrom => h("path.from", "jet_path_from", 1, 1, &[true], None, carrier),
            PathToString => h("path.to_string", "jet_path_to_string", 1, 1, &[true], None, carrier),
            PathHome => h("path.home", "jet_path_home", 0, 0, &[], Some(Effect::FS), carrier),
            PathJoin => h("path.join", "jet_path_join", 2, 2, &[true, true], None, carrier),
            PathParent => h("path.parent", "jet_path_parent", 1, 1, &[true], None, carrier),
            PathExtension => h("path.extension", "jet_path_extension", 1, 1, &[true], None, carrier),
            PathStem => h("path.stem", "jet_path_stem", 1, 1, &[true], None, carrier),
            PathNormalize => h("path.normalize", "jet_path_normalize", 1, 1, &[true], None, carrier),
            PathIsWithin => h("path.is_within", "jet_path_is_within", 2, 2, &[true, true], None, carrier),
            PathWriteAtomic => h("path.write_atomic", "jet_path_write_atomic", 2, 2, &[true, true], Some(Effect::FS), carrier),
            PathWalk => h("path.walk", "jet_path_walk", 1, 1, &[true], Some(Effect::FS), carrier),
            PluginInvoke { .. } => plugin_call_route("plugin.invoke", "jet_plugin_call", carrier),
            ReaderOver { owned } => {
                if *owned {
                    h(
                        "reader.over",
                        "jet_reader_over_owned",
                        1,
                        1,
                        &[false],
                        None,
                        carrier,
                    )
                } else {
                    h("reader.over", "jet_reader_over", 1, 1, &[true], None, carrier)
                }
            }
            ReaderReadU8 => h("reader.read_u8", "jet_reader_read_u8", 1, 1, &[true], None, carrier),
            ReaderReadI8 => h("reader.read_i8", "jet_reader_read_i8", 1, 1, &[true], None, carrier),
            ReaderReadU16Le => h("reader.read_u16_le", "jet_reader_read_u16_le", 1, 1, &[true], None, carrier),
            ReaderReadU16Be => h("reader.read_u16_be", "jet_reader_read_u16_be", 1, 1, &[true], None, carrier),
            ReaderReadI16Le => h("reader.read_i16_le", "jet_reader_read_i16_le", 1, 1, &[true], None, carrier),
            ReaderReadI16Be => h("reader.read_i16_be", "jet_reader_read_i16_be", 1, 1, &[true], None, carrier),
            ReaderReadU32Le => h("reader.read_u32_le", "jet_reader_read_u32_le", 1, 1, &[true], None, carrier),
            ReaderReadU32Be => h("reader.read_u32_be", "jet_reader_read_u32_be", 1, 1, &[true], None, carrier),
            ReaderReadI32Le => h("reader.read_i32_le", "jet_reader_read_i32_le", 1, 1, &[true], None, carrier),
            TaskJoin => task_route(receiver, carrier)?,
            TaskDetach => prelude(
                MirPreludeFamily::HandleMethod,
                "core.tasks",
                "detach",
                "jet_std::JetTask::detach",
                1,
                1,
                &[false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ReaderReadI32Be => h("reader.read_i32_be", "jet_reader_read_i32_be", 1, 1, &[true], None, carrier),
            ReaderReadU64Le => h("reader.read_u64_le", "jet_reader_read_u64_le", 1, 1, &[true], None, carrier),
            ReaderReadU64Be => h("reader.read_u64_be", "jet_reader_read_u64_be", 1, 1, &[true], None, carrier),
            ReaderReadI64Le => h("reader.read_i64_le", "jet_reader_read_i64_le", 1, 1, &[true], None, carrier),
            ReaderReadI64Be => h("reader.read_i64_be", "jet_reader_read_i64_be", 1, 1, &[true], None, carrier),
            ReaderReadF32Le => h("reader.read_f32_le", "jet_reader_read_f32_le", 1, 1, &[true], None, carrier),
            ReaderReadF32Be => h("reader.read_f32_be", "jet_reader_read_f32_be", 1, 1, &[true], None, carrier),
            ReaderReadF64Le => h("reader.read_f64_le", "jet_reader_read_f64_le", 1, 1, &[true], None, carrier),
            ReaderReadF64Be => h("reader.read_f64_be", "jet_reader_read_f64_be", 1, 1, &[true], None, carrier),
            ReaderPeek => h("reader.peek", "jet_reader_peek", 1, 1, &[true], None, carrier),
            ReaderSeek => h("reader.seek", "jet_reader_seek", 2, 2, &[true, false], None, carrier),
            ReaderSkip => h("reader.skip", "jet_reader_skip", 2, 2, &[true, false], None, carrier),
            ReaderTake => h("reader.take", "jet_reader_take", 2, 2, &[true, false], None, carrier),
            ReaderRemaining => h("reader.remaining", "jet_reader_remaining", 1, 1, &[true], None, carrier),
            ReaderAtEnd => h("reader.at_end", "jet_reader_at_end", 1, 1, &[true], None, carrier),
            CursorOver => h("cursor.over", "jet_cursor_over", 1, 1, &[true], None, carrier),
            CursorTakeUntil => h("cursor.take_until", "jet_cursor_take_until", 2, 2, &[true, true], None, carrier),
            CursorSkipWs => h("cursor.skip_ws", "jet_cursor_skip_ws", 1, 1, &[true], None, carrier),
            CursorTakePattern { .. } | ReaderTakePattern { .. } => primitive(),
            HTTPRespTrailers => h("http.response_trailers", "jet_http_srv_response_trailers", 2, 2, &[true, false], Some(Effect::Net), carrier),
            FfiCallbackEventStop => prelude(
                MirPreludeFamily::HandleMethod,
                "core.ffi",
                "callback_event_stop",
                "jet_ffi_callback_event_stop_unit",
                0,
                0,
                &[],
                Some(Effect::FFI),
                carrier,
                MirPreludeAbi::Value,
            ),
            // These methods are inherent Rust methods or require a type-specific
            // receiver/FFI policy not represented by this operation alone. Their
            // checked TIR projection must expand them before MIR.
            EventMethod { method } => event_route(receiver, method, carrier)?,
            PreciseMethod { type_name, method } => {
                let arity = match method.as_str() {
                    "add" | "sub" | "mul" | "div" | "equal" => 2,
                    "to_string" | "numerator" | "denominator" | "to_float" | "is_zero" => 1,
                    _ => return Err(route_error(format!("unregistered precise method `{type_name}.{method}`"))),
                };
                TRoutePlan::Prelude(precise_builtin_route(type_name, method, arity, receiver, carrier)?)
            }
            ReactiveGet => reactive_method_route(receiver, "get", carrier)?,
            ReactiveSet => reactive_method_route(receiver, "set", carrier)?,
            ChannelReceive => prelude(
                MirPreludeFamily::HandleMethod,
                "core.channels",
                "receiver.receive",
                "jet_std::JetReceiver::receive",
                1,
                1,
                &[true],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            ChannelClose => {
                let (member, symbol) =
                    if receiver.without_user_tags().base_name() == Some("Sender") {
                        ("sender.close", "jet_std::JetSender::close")
                    } else {
                        ("receiver.close", "jet_std::JetReceiver::close")
                    };
                prelude(
                    MirPreludeFamily::HandleMethod,
                    "core.channels",
                    member,
                    symbol,
                    1,
                    1,
                    &[true],
                    None,
                    carrier,
                    MirPreludeAbi::Value,
                )
            }
            SenderSend => prelude(
                MirPreludeFamily::HandleMethod,
                "core.channels",
                "sender.send",
                "jet_std::JetSender::send",
                2,
                2,
                &[true, false],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            AllocAlloc | AllocTryAlloc | AllocReset => {
                let allocator = match receiver.without_user_tags() {
                    Type::Named(name) if matches!(name.as_str(), "Arena" | "Bump" | "Pool" | "Fixed") => name,
                    _ => return Err(route_error("checked allocator method has no allocator receiver")),
                };
                let (method, arity, borrow_mask): (&str, usize, &[bool]) = match self {
                    AllocAlloc => ("alloc", 2, &[true, false]),
                    AllocTryAlloc => ("try_alloc", 2, &[true, false]),
                    AllocReset => ("reset", 1, &[true]),
                    _ => unreachable!(),
                };
                h(&format!("{allocator}.{method}"), &format!("jet_mem::Jet{allocator}::{method}"), arity, arity, borrow_mask, Some(Effect::Mem), carrier)
            }
            TaskPause => h("task.pause", "jet_std::JetTask::pause", 1, 1, &[true], None, carrier),
            TaskResume => h("task.resume", "jet_std::JetTask::resume", 1, 1, &[true], None, carrier),
            TaskCancel => h("task.cancel", "jet_std::JetTask::cancel", 1, 1, &[true], None, carrier),
            HTTPReqField(_) | HTTPRespField(_) | HTTPRespHeader
            | ArgsSpecHelp | WatchMethod { .. }
            | ReflectValueTypeName
            | ReflectValuePath | ReflectValueDisplay | ReflectValueFields | ReflectFieldName
            | ReflectFieldValue | DataTreeField | DataTreeAt | DataTreeInt | DataTreeText
            | DataTreeBool | DataTreeFloat | DataTreeToText | DataTreeEqualUnordered
            | DataTreeDecode(_) | SerdeEncode | JSONField | JSONAt | JSONInt | JSONText
            | JSONBool | JSONFloat | JSONToText | JSONEqualUnordered
            | DBWithPolicy | DBBegin | DBCommit | DBRollback | DBClose | DBValueInt
            | DBValueFloat | DBValueText | DBValueBool | DBValueBlob | DBValueIsNull
            | ModOnTick => primitive(),
            MeasurementMethod { method } => measurement_route(method, carrier)?,
            WebVirtualWindowFacts => prelude(
                MirPreludeFamily::HandleMethod,
                "core.web.virtual",
                "facts_json",
                "jet_web_virtual_window_facts",
                1,
                1,
                &[true],
                None,
                carrier,
                MirPreludeAbi::Value,
            ),
            AppMethod { method, args_len } => app_route(method, *args_len, carrier),
            DevServerMethod { method } => devserver_route(method, carrier),
            LayoutMethod { method } => layout_method_route(method, carrier)?,
            LoadableMethod { method } => loadable_method_route(method, carrier)?,
            UiBackendMethod { method } => ui_backend_method_route(method, receiver, carrier)?,
            HTTPRouterRegister {
                handler_param_names,
                ..
            } => {
                let (member, symbol, arity, borrow_mask) =
                    if matches!(receiver, Type::Named(name) if name == "HTTPMux") {
                        if handler_param_names.is_empty() {
                            (
                                "mux_add_zero",
                                "jet_http_mux_add_zero_handler",
                                4,
                                vec![true, false, false, false],
                            )
                        } else {
                            (
                                "mux_add",
                                "jet_http_mux_add_handler",
                                4,
                                vec![true, false, false, false],
                            )
                        }
                    } else {
                        (
                            "router_register",
                            "jet_http_router_register",
                            7,
                            vec![true, false, false, false, true, false, false],
                        )
                    };
                prelude(
                    MirPreludeFamily::StaticPrelude,
                    "core.http",
                    member,
                    symbol,
                    arity,
                    arity,
                    &borrow_mask,
                    None,
                    carrier,
                    MirPreludeAbi::Control,
                )
            }
            MathMethod {
                type_name,
                method,
                ..
            } if crate::Sema::is_geometry_type(type_name) => {
                geometry_method_route(type_name, method, receiver, carrier)?
            }
            MathMethod { type_name, method, reduce_op } => {
                let (suffix, arity) = match (method.as_str(), reduce_op.as_deref()) {
                    ("reduce", Some("Add")) => ("reduce_add", 1),
                    ("reduce", Some("Mul")) => ("reduce_mul", 1),
                    ("reduce", Some("Min")) => ("reduce_min", 1),
                    ("reduce", Some("Max")) => ("reduce_max", 1),
                    ("reduce", Some("Avg")) => ("reduce_avg", 1),
                    ("to_array" | "sum" | "product" | "min" | "max", None)
                        if crate::Sema::is_simd_lane_type(type_name) => (method.as_str(), 1),
                    ("to_array" | "length" | "normalize", None)
                        if matches!(type_name.as_str(), "Vec2" | "Vec3" | "Vec4") => (method.as_str(), 1),
                    ("dot", None) if matches!(type_name.as_str(), "Vec2" | "Vec3" | "Vec4") => ("dot", 2),
                    ("cross", None) if type_name == "Vec3" => ("cross", 2),
                    ("to_array" | "transpose", None) if matches!(type_name.as_str(), "Mat3" | "Mat4") => (method.as_str(), 1),
                    ("matmul" | "transform", None) if matches!(type_name.as_str(), "Mat3" | "Mat4") => (method.as_str(), 2),
                    _ => return Err(route_error(format!("checked math method `{type_name}.{method}` requires structural MIR lowering"))),
                };
                let borrow_mask = if arity == 1 { vec![true] } else { vec![true, false] };
                h(&format!("{type_name}.{suffix}"), &format!("jet_math_{type_name}_{suffix}"), arity, arity, &borrow_mask, None, carrier)
            }
            ReactiveEffectMethod { .. }
            | ServiceRuntimeSend | ServiceRuntimeRetry | ServiceRuntimeDeadLetter
            | ServiceRuntimeRetain | ServiceRuntimeCommit => primitive(),
            DBLeaseClose => h("db_lease.close", "jet_db_lease_close", 1, 1, &[false], Some(Effect::DB), carrier),
            DBPoolAcquire => h("db_pool.acquire", "jet_db_pool_acquire", 1, 1, &[true], Some(Effect::DB), carrier),
            DBPoolAcquireDeadline => h("db_pool.acquire_deadline", "jet_db_pool_acquire_deadline", 2, 2, &[true, true], Some(Effect::DB), carrier),
            DBPoolReady => h("db_pool.ready", "jet_db_pool_ready", 1, 1, &[true], Some(Effect::DB), carrier),
            DBPoolDrain => h("db_pool.drain", "jet_db_pool_drain", 1, 1, &[true], Some(Effect::DB), carrier),
            DBPoolReceipt => h("db_pool.receipt", "jet_db_pool_receipt", 1, 1, &[true], Some(Effect::DB), carrier),
            DBQuery { metadata } => db_h(
                "query",
                "jet_db_scope_query_with_metadata",
                metadata,
                carrier,
            ),
            DBQueryOne { metadata } => db_h(
                "query_one",
                "jet_db_scope_query_one_with_metadata",
                metadata,
                carrier,
            ),
            DBExecute { metadata } => db_h(
                "execute",
                "jet_db_scope_execute_with_metadata",
                metadata,
                carrier,
            ),
            DBLive => primitive(),
        };
        Ok(plan)
    }

    pub(super) fn prelude_route(
        &self,
        receiver: &Type,
        carrier: &TFailureCarrier,
    ) -> Result<TPreludeRoute, LowerError> {
        as_prelude(self.route_plan(receiver, carrier)?, "handle method")
    }
}

fn hardware_route(
    op: &super::THardwareCall,
    carrier: &TFailureCarrier,
) -> TRoutePlan {
    let (member, symbol, arity, borrow_mask) = match op {
        super::THardwareCall::RegisterRead { .. } => (
            "register_read",
            "jet_hardware_register_read",
            0,
            Vec::new(),
        ),
        super::THardwareCall::RegisterWrite { .. } => (
            "register_write",
            "jet_hardware_register_write",
            1,
            vec![false],
        ),
        super::THardwareCall::DmaStart { .. } => (
            "dma_start",
            "jet_hardware_dma_start",
            1,
            vec![false],
        ),
        super::THardwareCall::DmaWait { .. } => (
            "dma_wait",
            "jet_hardware_dma_wait",
            0,
            Vec::new(),
        ),
    };
    prelude(
        MirPreludeFamily::HandleMethod,
        "core.hardware",
        member,
        symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
    )
}
fn devserver_route(method: &str, carrier: &TFailureCarrier) -> TRoutePlan {
    let (symbol, arity, borrow_mask, effect) = match method {
        "html" => ("jet_devserver_html", 2, vec![true, false], None),
        "port" => ("jet_devserver_port", 2, vec![true, false], None),
        "serve" => ("jet_devserver_serve", 1, vec![true], Some(Effect::Net)),
        _ => return primitive(),
    };
    prelude(
        MirPreludeFamily::HandleMethod,
        "core.web.devserver",
        method,
        symbol,
        arity,
        arity,
        &borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Value,
    )
}

fn app_route(method: &str, args_len: usize, carrier: &TFailureCarrier) -> TRoutePlan {
    // Route and loader registrations carry the checked input binding as one
    // trailing String argument appended by TIR lowering.
    let (symbol, arity, borrow_mask, effect) = match (method, args_len) {
        ("route", 2) => ("jet_app_route", 4, vec![true, false, false, false], None),
        ("page", 2) => ("jet_app_page", 4, vec![true, false, false, false], None),
        ("layout", 2) => ("jet_app_layout", 4, vec![true, false, false, false], None),
        ("loader", 2) => ("jet_app_loader", 4, vec![true, false, false, false], None),
        ("loader", 3) => (
            "jet_app_loader_preload",
            5,
            vec![true, false, false, false, false],
            None,
        ),
        ("pending", 1) => ("jet_app_pending", 2, vec![true, false], None),
        ("not_found", 1) => ("jet_app_not_found", 2, vec![true, false], None),
        ("error", 1) => ("jet_app_error", 2, vec![true, false], None),
        ("action", 2) => ("jet_app_action", 3, vec![true, false, false], None),
        ("form", 2) => ("jet_app_form", 3, vec![true, false, false], None),
        ("data", 2) => ("jet_app_data", 3, vec![true, false, false], None),
        ("mount", 2) => ("jet_app_mount", 3, vec![true, false, false], None),
        ("mount", 3) => (
            "jet_app_mount_with_effect",
            4,
            vec![true, false, false, false],
            None,
        ),
        ("mount", 4) => (
            "jet_app_mount_with_policy",
            5,
            vec![true, false, false, false, false],
            None,
        ),
        ("routes", 1) => ("jet_app_routes", 2, vec![true, false], None),
        ("security", 1) => ("jet_app_security", 2, vec![true, false], None),
        ("assets", 1) => ("jet_app_assets", 2, vec![true, false], None),
        ("split", 1) => ("jet_app_split", 2, vec![true, false], None),
        ("code_split", 1) => ("jet_app_code_split", 2, vec![true, false], None),
        ("cache", 1) => ("jet_app_cache", 2, vec![true, false], None),
        ("a11y", 1) => ("jet_app_a11y", 2, vec![true, false], None),
        ("adapter", 1) => ("jet_app_adapter", 2, vec![true, false], None),
        ("csr", 0) => ("jet_app_csr", 1, vec![true], None),
        ("ssr", 0) => ("jet_app_ssr", 1, vec![true], None),
        ("ssg", 0) => ("jet_app_ssg", 1, vec![true], None),
        ("stream", 0) => ("jet_app_stream", 1, vec![true], None),
        ("streaming", 0) => ("jet_app_streaming", 1, vec![true], None),
        ("island", 0) => ("jet_app_island", 1, vec![true], None),
        ("hydration_dev", 0) => ("jet_app_hydration_dev", 1, vec![true], None),
        ("hydration_release", 0) => ("jet_app_hydration_release", 1, vec![true], None),
        ("facts_json", 0) => ("jet_app_facts_json", 1, vec![true], None),
        ("serve", 0) => ("jet_app_serve", 1, vec![true], Some(Effect::Net)),
        ("serve", 1) => (
            "jet_app_serve_with_port",
            2,
            vec![true, false],
            Some(Effect::Net),
        ),
        ("serve_on", 1) => (
            "jet_app_serve_on",
            2,
            vec![true, false],
            Some(Effect::Net),
        ),
        _ => return primitive(),
    };
    prelude(
        MirPreludeFamily::HandleMethod,
        "core.web.app",
        method,
        symbol,
        arity,
        arity,
        &borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Value,
    )
}
fn h(
    member: &str,
    symbol: &str,
    arity: usize,
    max_arity: usize,
    borrow_mask: &[bool],
    effect: Option<Effect>,
    carrier: &TFailureCarrier,
) -> TRoutePlan {
    prelude(
        MirPreludeFamily::HandleMethod,
        "core.handle",
        member,
        symbol,
        arity,
        max_arity,
        borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Value,
    )
}
fn regex_route(
    kind: &str,
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (symbol, borrow_mask): (&str, &[bool]) = match (kind, method) {
        ("Regex", "pattern") => ("jet_std::JetRegex::pattern", &[true][..]),
        ("Regex", "source") => ("jet_std::JetRegex::source", &[true][..]),
        ("Regex", "flags") => ("jet_std::JetRegex::flags", &[true][..]),
        ("Regex", "options") => ("jet_std::JetRegex::options", &[true][..]),
        ("Regex", "names") => ("jet_std::JetRegex::names", &[true][..]),
        ("Regex", "count") => ("jet_std::JetRegex::count", &[true, true][..]),
        ("Regex", "is_match") => ("jet_std::JetRegex::is_match", &[true, true][..]),
        ("Regex", "full_match") => ("jet_std::JetRegex::full_match", &[true, true][..]),
        ("Regex", "match") => ("jet_std::JetRegex::match_value", &[true, true][..]),
        ("Regex", "find") => ("jet_std::JetRegex::find", &[true, true][..]),
        ("Regex", "find_all") => ("jet_std::JetRegex::find_all", &[true, true][..]),
        ("Regex", "matches") => ("jet_std::JetRegex::matches", &[true, true][..]),
        ("Regex", "split") => ("jet_std::JetRegex::split", &[true, true][..]),
        ("Regex", "replace") => ("jet_std::JetRegex::replace", &[true, true, true][..]),
        ("Regex", "replace_first") => (
            "jet_std::JetRegex::replace_first",
            &[true, true, true][..],
        ),
        ("Regex", "replace_all_with") => (
            "jet_std::JetRegex::replace_all_with",
            &[true, true, false][..],
        ),
        ("Regex", "split_limit") => (
            "jet_std::JetRegex::split_limit",
            &[true, true, false][..],
        ),
        ("Match", "start") => ("jet_std::JetRegexMatch::start", &[true][..]),
        ("Match", "end") => ("jet_std::JetRegexMatch::end", &[true][..]),
        ("Match", "named_captures") => {
            ("jet_std::JetRegexMatch::named_captures", &[true][..])
        }
        ("Match", "group") => ("jet_std::JetRegexMatch::group", &[true, false][..]),
        ("Match", "name") => ("jet_std::JetRegexMatch::name", &[true, true][..]),
        ("Match", "group_start") => {
            ("jet_std::JetRegexMatch::group_start", &[true, false][..])
        }
        ("Match", "group_end") => ("jet_std::JetRegexMatch::group_end", &[true, false][..]),
        _ => {
            return Err(route_error(format!(
                "unknown checked regex method `{kind}.{method}`"
            )))
        }
    };
    let member = format!("{kind}.{method}");
    Ok(h(
        &member,
        symbol,
        borrow_mask.len(),
        borrow_mask.len(),
        borrow_mask,
        None,
        carrier,
    ))
}

fn url_mime_route(
    kind: &str,
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (symbol, borrow_mask): (&str, &[bool]) = match (kind, method) {
        ("Url", "scheme") => ("jet_std::JetURL::scheme", &[true][..]),
        ("Url", "host") => ("jet_std::JetURL::host", &[true][..]),
        ("Url", "port") => ("jet_std::JetURL::port", &[true][..]),
        ("Url", "path") => ("jet_std::JetURL::path", &[true][..]),
        ("Url", "path_segments") => ("jet_std::JetURL::path_segments", &[true][..]),
        ("Url", "query") => ("jet_std::JetURL::query", &[true][..]),
        ("Url", "query_pairs") => ("jet_std::JetURL::query_pairs", &[true][..]),
        ("Url", "fragment") => ("jet_std::JetURL::fragment", &[true][..]),
        ("Url", "normalize") => ("jet_std::JetURL::normalize", &[true][..]),
        ("Url", "to_string") => ("jet_std::JetURL::to_string_value", &[true][..]),
        ("Url", "username") => ("jet_std::JetURL::username", &[true][..]),
        ("Url", "password") => ("jet_std::JetURL::password", &[true][..]),
        ("Url", "userinfo") => ("jet_std::JetURL::userinfo", &[true][..]),
        ("Url", "authority") => ("jet_std::JetURL::authority", &[true][..]),
        ("Url", "default_port") => ("jet_std::JetURL::default_port", &[true][..]),
        ("Url", "join") => ("jet_std::JetURL::join", &[true, true][..]),
        ("Url", "set_query") => ("jet_std::JetURL::set_query", &[true, true, true][..]),
        ("Url", "add_query") => ("jet_std::JetURL::add_query", &[true, true, true][..]),
        ("Mime", "media_type") => ("jet_std::JetMIME::media_type", &[true][..]),
        ("Mime", "subtype") => ("jet_std::JetMIME::subtype", &[true][..]),
        ("Mime", "essence") => ("jet_std::JetMIME::essence", &[true][..]),
        ("Mime", "params") => ("jet_std::JetMIME::params", &[true][..]),
        ("Mime", "to_string") => ("jet_std::JetMIME::to_string_value", &[true][..]),
        ("Mime", "param") => ("jet_std::JetMIME::param", &[true, true][..]),
        _ => {
            return Err(route_error(format!(
                "unknown checked URL/MIME method `{kind}.{method}`"
            )))
        }
    };
    let member = format!("{kind}.{method}");
    Ok(h(
        &member,
        symbol,
        borrow_mask.len(),
        borrow_mask.len(),
        borrow_mask,
        None,
        carrier,
    ))
}

fn measurement_route(
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (symbol, borrow_mask): (&str, &[bool]) = match method {
        "value" => ("jet_std::JetMeasurement::value", &[true][..]),
        "uncertainty" => ("jet_std::JetMeasurement::uncertainty", &[true][..]),
        "add" => ("jet_std::JetMeasurement::add", &[true, false][..]),
        "sub" => ("jet_std::JetMeasurement::sub", &[true, false][..]),
        "mul" => ("jet_std::JetMeasurement::mul", &[true, false][..]),
        "div" => ("jet_std::JetMeasurement::div", &[true, false][..]),
        "sqrt" => ("jet_std::JetMeasurement::sqrt", &[true][..]),
        _ => return Err(route_error(format!("unknown checked measurement method `{method}`"))),
    };
    let member = format!("Measurement.{method}");
    Ok(h(
        &member,
        symbol,
        borrow_mask.len(),
        borrow_mask.len(),
        borrow_mask,
        None,
        carrier,
    ))
}


fn reactive_method_route(
    receiver: &Type,
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (member, symbol, borrow_mask): (&str, &str, &[bool]) =
        match (receiver.without_user_tags().base_name(), method) {
            (Some("Signal"), "get") =>
                ("Signal.get", "jet_std::JetSignal::get", &[true]),
            (Some("Derived"), "get") =>
                ("Derived.get", "jet_std::JetDerived::get", &[true]),
            (Some("Signal"), "set") =>
                ("Signal.set", "jet_std::JetSignal::set", &[true, false]),
            _ => return Err(route_error(format!(
                "checked reactive method `{method}` has no receiver route for {}",
                receiver.name(),
            ))),
        };
    Ok(prelude(
        MirPreludeFamily::HandleMethod,
        "core.reactive",
        member,
        symbol,
        borrow_mask.len(),
        borrow_mask.len(),
        borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
    ))
}

fn layout_method_route(
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (symbol, arity, borrow_mask) = match method {
        "h" | "v" => (format!("jet_layout::Handle::{method}"), 3, vec![true, false, false]),
        "value" => ("jet_layout::Handle::value".to_string(), 2, vec![true, false]),
        "suggest" => ("jet_layout::Handle::suggest".to_string(), 3, vec![true, false, false]),
        "is_feasible" => ("jet_layout::Handle::is_feasible".to_string(), 1, vec![true]),
        "conflict" => ("jet_layout::Handle::conflict".to_string(), 1, vec![true]),
        "required" | "strong" | "medium" | "weak" => (
            format!("jet_layout::Constraint::{method}"),
            1,
            vec![false],
        ),
        _ => return Err(route_error(format!("unknown checked layout method `{method}`"))),
    };
    prelude_route_row(
        MirPreludeFamily::HandleMethod,
        "core.layout",
        &format!("layout.{method}"),
        &symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "layout method",
    )
    .map(TRoutePlan::Prelude)
}

fn loadable_method_route(
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (arity, borrow_mask) = match method {
        "is_loading" | "is_loaded" | "is_failed" | "is_idle" | "loaded" => (1, vec![true]),
        "or_else" => (2, vec![true, false]),
        _ => return Err(route_error(format!("unknown checked Loadable method `{method}`"))),
    };
    let symbol = format!("JetLoadable::{method}");
    prelude_route_row(
        MirPreludeFamily::HandleMethod,
        "core.pending",
        &format!("loadable.{method}"),
        &symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "loadable method",
    )
    .map(TRoutePlan::Prelude)
}

fn ui_backend_method_route(
    method: &str,
    receiver: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let name = match receiver {
        Type::Named(name) | Type::Apply { name, .. } => name.as_str(),
        _ => return Err(route_error("checked UI backend receiver has no nominal type")),
    };
    let name = name.rsplit("::").next().unwrap_or(name);
    let name = name.rsplit('.').next().unwrap_or(name);
    let backend = match name {
        "NullBackend" => "JetNullBackend",
        "TuiBackend" => "JetTuiBackend",
        "GtkBackend" => "JetGtkBackend",
        _ => return Err(route_error(format!("checked UI backend type `{name}` has no Prelude carrier"))),
    };
    let (member, borrow_mask): (&str, &[bool]) = match method {
        "measure" => ("measure_node", &[true, false, false]),
        "layout" => ("layout_node", &[true, false, false]),
        "paint" => ("paint_node", &[true, false]),
        "on_event" => ("dispatch_event", &[true, false]),
        "mount_default" => ("mount_node_default", &[true, false]),
        "mount" => ("mount_node", &[true, false, false]),
        "commands" if name == "NullBackend" => ("paint_commands", &[true]),
        "frame_lines" | "render_count" if name == "TuiBackend" => (method, &[true]),
        _ => return Err(route_error(format!("checked UI backend method `{name}.{method}` has no Prelude route"))),
    };
    prelude_route_row(
        MirPreludeFamily::HandleMethod,
        "core.ui",
        &format!("{name}.{method}"),
        &format!("{backend}::{member}"),
        borrow_mask.len(),
        borrow_mask.len(),
        borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "UI backend method",
    )
    .map(TRoutePlan::Prelude)
}

fn db_h(
    member: &str,
    symbol: &str,
    metadata: &Option<super::TDbQueryMetadata>,
    carrier: &TFailureCarrier,
) -> TRoutePlan {
    let TRoutePlan::Prelude(mut route) = prelude(
        MirPreludeFamily::HandleMethod,
        "core.handle",
        member,
        symbol,
        3,
        3,
        &[true, true, true],
        Some(Effect::DB),
        carrier,
        MirPreludeAbi::Value,
    ) else {
        unreachable!("database route is always a Prelude route");
    };
    route.db_metadata = metadata.clone();
    TRoutePlan::Prelude(route)
}


fn plugin_call_route(member: &str, symbol: &str, carrier: &TFailureCarrier) -> TRoutePlan {
    let TRoutePlan::Prelude(mut route) = prelude(
        MirPreludeFamily::HandleMethod,
        "core.handle",
        member,
        symbol,
        3,
        3,
        &[true, true, true],
        Some(Effect::FFI),
        carrier,
        MirPreludeAbi::Value,
    ) else {
        unreachable!("plugin call route is always a Prelude route");
    };
    route.authority = Some(MirAuthorityDecision::plugin_call());
    TRoutePlan::Prelude(route)
}


fn is_string_receiver(receiver: &Type) -> bool {
    match receiver {
        Type::Tagged { inner, .. } => is_string_receiver(inner),
        Type::String => true,
        _ => false,
    }
}


fn builtin_collection_route(
    op: &TBuiltinOp,
    receiver: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let receiver = match receiver {
        Type::Tagged { inner, .. } => {
            return builtin_collection_route(op, inner, carrier);
        }
        ty => ty,
    };
    let row = |member: &str, symbol: &str, arity: usize, borrow_mask: &[bool]| {
        b(member, symbol, arity, arity, borrow_mask, None, carrier)
    };
    let result = match (op, receiver) {
        (TBuiltinOp::Contains, Type::List(_) | Type::FixedList { .. }) => {
            row("contains", "jet_list_contains", 2, &[true, true])
        }
        (TBuiltinOp::Contains, Type::Apply { name, .. })
            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut") =>
        {
            row("contains", "jet_list_contains", 2, &[true, true])
        }
        (TBuiltinOp::Contains, Type::Named(name)) if name == "Range" => {
            row("contains", "jet_range_contains", 2, &[true, false])
        }
        (TBuiltinOp::Contains, Type::InlineRange { .. }) => {
            row("contains", "jet_range_contains", 2, &[true, false])
        }
        (TBuiltinOp::LenList, Type::List(_) | Type::FixedList { .. }) => {
            row("len", "jet_list_len", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::Apply { name, .. }) if name == "Iter" || name == "ViewIter" => {
            row("len", "jet_iter_len", 1, &[false])
        }
        (TBuiltinOp::LenList, Type::Apply { name, .. })
            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut") =>
        {
            row("len", "jet_list_len", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::Map { .. }) => row("len", "jet_map_len", 1, &[true]),
        (TBuiltinOp::LenList, Type::Apply { name, .. }) if name == "Set" => {
            row("len", "jet_set_len", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_RANK => {
            row("len", "jet_sorted_set_len", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::Apply { name, .. })
            if name == crate::Syntax::TYPE_PRIORITY_QUEUE =>
        {
            row("len", "jet_priority_queue_len", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_LRU => {
            row("len", "jet_lru_len", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_TALLY => {
            row("len", "jet_bag_len", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::Named(name)) if name == crate::Syntax::TYPE_BITS => {
            row("len", "jet_bit_set_len", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_QUEUE => {
            row("len", "jet_deque_len", 1, &[true])
        }
        (TBuiltinOp::DequeCapacity, Type::Apply { name, .. })
            if name == crate::Syntax::TYPE_QUEUE =>
        {
            row("capacity", "jet_deque_capacity", 1, &[true])
        }
        (TBuiltinOp::LenList, Type::String) => row("len", "jet_char_len", 1, &[true]),
        (TBuiltinOp::IsEmpty, Type::List(_) | Type::FixedList { .. }) => {
            row("is_empty", "jet_list_is_empty", 1, &[true])
        }
        (TBuiltinOp::IsEmpty, Type::Apply { name, .. }) if name == "Iter" || name == "ViewIter" => {
            row("is_empty", "jet_iter_is_empty", 1, &[false])
        }
        (TBuiltinOp::IsEmpty, Type::Apply { name, .. })
            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut") =>
        {
            row("is_empty", "jet_list_is_empty", 1, &[true])
        }
        (TBuiltinOp::IsEmpty, Type::Map { .. }) => row("is_empty", "jet_map_is_empty", 1, &[true]),
        (TBuiltinOp::IsEmpty, Type::String) => row("is_empty", "jet_string_is_empty", 1, &[true]),
        (TBuiltinOp::IsEmpty, Type::Apply { name, .. }) if name == "Set" => {
            row("is_empty", "jet_set_is_empty", 1, &[true])
        }
        (TBuiltinOp::IsEmpty, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_RANK => {
            row("is_empty", "jet_sorted_set_is_empty", 1, &[true])
        }
        (TBuiltinOp::IsEmpty, Type::Apply { name, .. })
            if name == crate::Syntax::TYPE_PRIORITY_QUEUE =>
        {
            row("is_empty", "jet_priority_queue_is_empty", 1, &[true])
        }
        (TBuiltinOp::IsEmpty, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_LRU => {
            row("is_empty", "jet_lru_is_empty", 1, &[true])
        }
        (TBuiltinOp::IsEmpty, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_TALLY => {
            row("is_empty", "jet_bag_is_empty", 1, &[true])
        }
        (TBuiltinOp::IsEmpty, Type::Named(name)) if name == crate::Syntax::TYPE_BITS => {
            row("is_empty", "jet_bit_set_is_empty", 1, &[true])
        }
        (TBuiltinOp::IsEmpty, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_QUEUE => {
            row("is_empty", "jet_deque_is_empty", 1, &[true])
        }
        (TBuiltinOp::GetList, Type::List(_) | Type::FixedList { .. }) => {
            row("get", "jet_list_get_opt", 2, &[true, false])
        }
        (TBuiltinOp::GetList, Type::Apply { name, .. })
            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut") =>
        {
            row("get", "jet_list_get_opt", 2, &[true, false])
        }
        (TBuiltinOp::GetMap, Type::Map { .. }) => {
            row("get", "jet_map_get_opt", 2, &[true, true])
        }
        (TBuiltinOp::First, Type::List(_) | Type::FixedList { .. }) => {
            row("first", "jet_list_first", 1, &[true])
        }
        (TBuiltinOp::First, Type::Apply { name, .. }) if name == "Iter" || name == "ViewIter" => {
            row("first", "jet_iter_first", 1, &[false])
        }
        (TBuiltinOp::First, Type::Apply { name, .. })
            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut") =>
        {
            row("first", "jet_list_first", 1, &[true])
        }
        (TBuiltinOp::First, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_RANK => {
            row("first", "jet_sorted_set_first", 1, &[true])
        }
        (TBuiltinOp::Last, Type::List(_) | Type::FixedList { .. }) => {
            row("last", "jet_list_last", 1, &[true])
        }
        (TBuiltinOp::Last, Type::Apply { name, .. })
            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut") =>
        {
            row("last", "jet_list_last", 1, &[true])
        }
        (TBuiltinOp::Last, Type::Apply { name, .. }) if name == crate::Syntax::TYPE_RANK => {
            row("last", "jet_sorted_set_last", 1, &[true])
        }
        _ => return Err(route_error(format!(
            "checked builtin operation `{op:?}` has an unsupported receiver type"
        ))),
    };
    Ok(result)
}

fn iterator_builtin_route(
    op: &TBuiltinOp,
    receiver: &Type,
    carrier: &TFailureCarrier,
) -> Option<TRoutePlan> {
    let receiver = match receiver {
        Type::Tagged { inner, .. } => return iterator_builtin_route(op, inner, carrier),
        ty => ty,
    };
    let is_iter = crate::Collections::is_iter_type(receiver);
    let is_list = matches!(receiver, Type::List(_) | Type::FixedList { .. });
    let route = |member: &str, symbol: &str, arity: usize, borrow_mask: &[bool]| {
        b(member, symbol, arity, arity, borrow_mask, None, carrier)
    };
    let plan = match op {
        TBuiltinOp::IterToList if is_iter => {
            route("iter_to_list", "jet_iter_to_list", 1, &[false])
        }
        TBuiltinOp::IterCollect if is_iter => {
            route("iter_collect", "jet_iter_collect", 1, &[false])
        }
        TBuiltinOp::ListLazy if is_list => {
            route("list_lazy", "jet_iter_from_vec", 1, &[false])
        }
        TBuiltinOp::Take if is_iter => route("iter_take", "jet_iter_take", 2, &[false, false]),
        TBuiltinOp::Skip if is_iter => route("iter_skip", "jet_iter_skip", 2, &[false, false]),
        TBuiltinOp::StepBy if is_iter => {
            route("iter_step_by", "jet_iter_step_by", 2, &[false, false])
        }
        TBuiltinOp::Dedup if is_iter => {
            route("iter_dedup", "jet_iter_dedup", 1, &[false])
        }
        TBuiltinOp::Chunks if is_iter => {
            route("iter_chunks", "jet_iter_chunks", 2, &[false, false])
        }
        TBuiltinOp::Windows if is_iter => {
            route("iter_windows", "jet_iter_windows", 2, &[false, false])
        }
        TBuiltinOp::Flatten if is_iter => {
            route("iter_flatten", "jet_iter_flatten", 1, &[false])
        }
        TBuiltinOp::Flatten if is_list => route("list_flatten", "jet_list_flatten", 1, &[false]),
        TBuiltinOp::Intersperse if is_iter => {
            route("iter_intersperse", "jet_iter_intersperse", 2, &[false, false])
        }
        TBuiltinOp::IterRepeat if is_iter => {
            route("iter_repeat", "jet_iter_repeat", 2, &[false, false])
        }
        TBuiltinOp::IterCycle if is_iter => {
            route("iter_cycle", "jet_iter_cycle", 2, &[false, false])
        }
        TBuiltinOp::IterDropLast if is_iter => {
            route("iter_drop_last", "jet_iter_drop_last", 2, &[false, false])
        }
        TBuiltinOp::IterShuffle if is_iter => {
            route("iter_shuffle", "jet_iter_shuffle", 1, &[false])
        }
        TBuiltinOp::IterIsSorted if is_iter => {
            route("iter_is_sorted", "jet_iter_is_sorted", 1, &[false])
        }
        TBuiltinOp::IterLastIndexOf if is_iter => {
            route("iter_last_index_of", "jet_iter_last_index_of", 2, &[false, false])
        }
        TBuiltinOp::IterAverage { float: true } if is_iter => {
            route("iter_average_float", "jet_iter_average_float", 1, &[false])
        }
        TBuiltinOp::IterAverage { float: false } if is_iter => {
            route("iter_average_int", "jet_iter_average_int", 1, &[false])
        }
        TBuiltinOp::IterCompare if is_iter => {
            route("iter_compare", "jet_iter_compare", 2, &[false, false])
        }
        TBuiltinOp::TryCollect if is_iter => {
            route("try_collect", "jet_list_try_collect", 1, &[false])
        }
        _ => return None,
    };
    Some(plan)
}

impl TBuiltinOp {
    pub(super) fn route_plan(
        &self,
        result: &Type,
        carrier: &TFailureCarrier,
    ) -> Result<TRoutePlan, LowerError> {
        use TBuiltinOp::*;
        let plan = match self {
            AtomicMethod { method } => atomic_method_route(method, carrier)?,
            LenString => b("len_string", "jet_char_len", 1, 1, &[true], None, carrier),
            ListTryNew => b("list_try_new", "jet_list_try_new", 1, 1, &[false], Some(Effect::Mem), carrier),
            ListTryWithCapacity => b("list_try_with_capacity", "jet_list_try_with_capacity", 2, 2, &[false, false], Some(Effect::Mem), carrier),
            TryPush => b("list_try_push", "jet_list_try_push", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            TryReserve => b("list_try_reserve", "jet_list_try_reserve", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            TryInsertMap => b("map_try_insert", "jet_map_try_insert", 3, 3, &[true, false, false], Some(Effect::Mem), carrier),
            TryStringPush => b("string_try_push", "jet_string_try_push", 2, 2, &[true, true], Some(Effect::Mem), carrier),
            Pop => b("list_pop", "jet_list_pop_kernel", 1, 1, &[true], Some(Effect::Mem), carrier),
            PriorityQueuePop => b("priority_queue_pop", "jet_priority_queue_pop_kernel", 1, 1, &[true], Some(Effect::Mem), carrier),
            MapMerge => b("map_merge", "jet_map_merge", 2, 2, &[true, true], None, carrier),
            MapFromKeys => b("map_from_keys", "jet_map_from_keys_kernel", 2, 2, &[false, false], None, carrier),
            ListReplace => b("list_replace", "jet_list_replace", 3, 3, &[true, false, false], None, carrier),
            ListEqual => b("list_equal", "jet_list_equal", 2, 2, &[true, true], None, carrier),
            ByteBufferWithCapacity => b("byte_buffer_with_capacity", "JetByteBuffer::with_capacity", 1, 1, &[false], None, carrier),
            MatchGroup => b("match_group", "jet_std::JetRegexMatch::group", 2, 2, &[true, false], None, carrier),
            IterSplit { .. } => b("iter_split", "jet_iter_split_at", 3, 3, &[false, false, false], None, carrier),
            Indexed { .. } => b("iter_enumerate", "jet_iter_enumerate", 2, 2, &[false, false], None, carrier),
            MapMergeWith => b("map_merge_with", "jet_map_merge_with", 3, 3, &[true, true, false], None, carrier),
            InsertList => b("list_insert", "jet_list_insert", 3, 3, &[true, false, false], Some(Effect::Mem), carrier),
            RemoveMap => b("map_remove", "jet_map_pop_kernel", 2, 2, &[true, true], Some(Effect::Mem), carrier),
            SetFrom => b("set_from", "jet_set_from", 1, 1, &[false], None, carrier),
            SortedSetFrom => b("sorted_set_from", "jet_sorted_set_from", 1, 1, &[false], None, carrier),
            SortedSetToList => b("sorted_set_to_list", "jet_sorted_set_to_list", 1, 1, &[true], None, carrier),
            RemoveList { mode, .. } => match mode {
                ListRemoveMode::Value => b("list_remove_value", "jet_list_remove_value", 2, 2, &[true, false], Some(Effect::Mem), carrier),
                ListRemoveMode::Slot => b("list_remove_slot", "jet_list_remove_slot", 2, 2, &[true, false], Some(Effect::Mem), carrier),
                ListRemoveMode::Dynamic => primitive(),
            },
            Push => b("list_push", "jet_list_push", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            InsertMap => b("map_insert", "jet_map_insert", 3, 3, &[true, false, false], Some(Effect::Mem), carrier),
            AddNewMap => b("map_add_new", "jet_map_add_new", 3, 3, &[true, false, false], Some(Effect::Mem), carrier),
            ExtendList => b("list_extend", "jet_list_extend", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            Reverse => b("list_reverse", "jet_list_reverse", 1, 1, &[true], Some(Effect::Mem), carrier),
            Sort => b("list_sort", "jet_list_sort", 1, 1, &[true], Some(Effect::Mem), carrier),
            Clear => b("list_clear", "jet_list_clear", 1, 1, &[true], Some(Effect::Mem), carrier),
            MapPopFirst => b("map_pop_first", "jet_map_pop_first", 1, 1, &[true], Some(Effect::Mem), carrier),
            CountList => b("list_count", "jet_list_count", 2, 2, &[true, true], None, carrier),
            Counts => b("list_counts", "jet_list_counts", 1, 1, &[false], None, carrier),
            ConcatList => b("list_concat", "jet_list_concat", 2, 2, &[true, true], None, carrier),
            SortDesc => b("list_sort_desc", "jet_list_sort_desc", 1, 1, &[true], Some(Effect::Mem), carrier),
            OrderingThen => b("ordering_then", "jet_ordering_then", 2, 2, &[true, true], None, carrier),
            OrderingReverse => b("ordering_reverse", "jet_ordering_reverse", 1, 1, &[true], None, carrier),
            Flatten => b("list_flatten", "jet_list_flatten", 1, 1, &[false], None, carrier),
            Intersperse => b("iter_intersperse", "jet_iter_intersperse", 2, 2, &[false, false], None, carrier),
            StringFromBytes => b("string_from_bytes", "jet_string_from_bytes", 1, 1, &[true], None, carrier),
            StringFromBytesLossy => b("string_from_bytes_lossy", "jet_string_from_bytes_lossy", 1, 1, &[true], None, carrier),
            Bytes { owned: true } => b("bytes", "jet_string_bytes", 1, 1, &[false], None, carrier),
            Bytes { owned: false } => b("bytes", "jet_string_bytes", 1, 1, &[true], None, carrier),
            Trim => b("string_trim", "jet_unicode_trim", 1, 1, &[true], None, carrier),
            TrimStart => b("string_trim_start", "jet_text_trim_start", 1, 1, &[true], None, carrier),
            TrimEnd => b("string_trim_end", "jet_text_trim_end", 1, 1, &[true], None, carrier),
            PadStart => b("string_pad_start", "jet_text_pad_start", 3, 3, &[true, false, true], None, carrier),
            PadEnd => b("string_pad_end", "jet_text_pad_end", 3, 3, &[true, false, true], None, carrier),
            StringIndexOf => b("string_index_of", "jet_unicode_index_of", 2, 2, &[true, true], None, carrier),
            StringCount => b("string_count", "jet_unicode_count", 2, 2, &[true, true], None, carrier),
            StringIsAlphabetic => b("string_is_alphabetic", "jet_text_is_alphabetic", 1, 1, &[true], None, carrier),
            StringIsNumeric => b("string_is_numeric", "jet_text_is_numeric", 1, 1, &[true], None, carrier),
            StringIsWhitespace => b("string_is_whitespace", "jet_text_is_whitespace", 1, 1, &[true], None, carrier),
            StringIsAscii => b("string_is_ascii", "jet_text_unicode_is_ascii", 1, 1, &[true], None, carrier),
            StringToTitle => b("string_to_title", "jet_text_title", 1, 1, &[true], None, carrier),
            Split => b("string_split", "jet_iter_string_split", 2, 2, &[true, true], None, carrier),
            Lines => b("string_lines", "jet_string_lines", 1, 1, &[true], None, carrier),
            ParseInt => b("int_parse", "jet_std::jet_int_parse", 1, 1, &[true], None, carrier),
            IntToRadix { .. } => b("int_to_radix", "jet_std::jet_int_to_radix", 2, 2, &[false, false], None, carrier),
            IntFromRadix { .. } => b("int_from_radix", "jet_std::jet_int_from_radix", 2, 2, &[true, false], None, carrier),
            ToUpper => b("string_upper", "jet_unicode_upper", 1, 1, &[true], None, carrier),
            ToLower => b("string_lower", "jet_unicode_lower", 1, 1, &[true], None, carrier),
            ToAsciiLower => b("string_ascii_lower", "jet_text_ascii_lower", 1, 1, &[true], None, carrier),
            ToAsciiUpper => b("string_ascii_upper", "jet_text_ascii_upper", 1, 1, &[true], None, carrier),
            Slice { .. } => b("string_slice", "jet_string_slice", 3, 3, &[true, false, false], None, carrier),
            After => b("string_after", "jet_string_after", 2, 2, &[true, true], None, carrier),
            Before => b("string_before", "jet_string_before", 2, 2, &[true, true], None, carrier),
            TrimView => b("string_trim_view", "jet_unicode_trim_view", 1, 1, &[true], None, carrier),
            AfterView => b("string_after_view", "jet_string_after_view", 2, 2, &[true, true], None, carrier),
            BeforeView => b("string_before_view", "jet_string_before_view", 2, 2, &[true, true], None, carrier),
            Keys => b("map_keys", "jet_map_keys", 1, 1, &[true], None, carrier),
            Values => b("map_values", "jet_map_values", 1, 1, &[true], None, carrier),
            MapTopN { .. } => b("map_top_n", "jet_map_top_n", 2, 2, &[true, false], None, carrier),
            MapMin => b("map_min", "jet_map_min_value_kernel", 1, 1, &[true], None, carrier),
            MapMax => b("map_max", "jet_map_max_value_kernel", 1, 1, &[true], None, carrier),
            SetUnion => b("set_union", "jet_set_union", 2, 2, &[true, true], None, carrier),
            SetIntersection => b("set_intersection", "jet_set_intersection", 2, 2, &[true, true], None, carrier),
            SetDifference => b("set_difference", "jet_set_difference", 2, 2, &[true, true], None, carrier),
            SetSymmetricDifference => b("set_symmetric_difference", "jet_set_symmetric_difference", 2, 2, &[true, true], None, carrier),
            SetIsSubset => b("set_is_subset", "jet_set_is_subset", 2, 2, &[true, true], None, carrier),
            SetIsSuperset => b("set_is_superset", "jet_set_is_superset", 2, 2, &[true, true], None, carrier),
            SetIsDisjoint => b("set_is_disjoint", "jet_set_is_disjoint", 2, 2, &[true, true], None, carrier),
            SetValues => b("set_values", "jet_iter_from_vec", 1, 1, &[true], None, carrier),
            SetPop => b("set_pop", "jet_set_pop_kernel", 2, 2, &[true, true], Some(Effect::Mem), carrier),
            SetInsert => b("set_insert", "jet_set_insert", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            SetRemove => b("set_remove", "jet_set_remove", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            SortedSetInsert => b("sorted_set_insert", "jet_sorted_set_insert", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            SortedSetRemove => b("sorted_set_remove", "jet_sorted_set_remove", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            BitSetAdd => b("bitset_add", "jet_bitset_add", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            BitSetRemove => b("bitset_remove", "jet_bitset_remove", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            BagAdd => b("bag_add", "jet_bag_add", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            BagRemove => b("bag_remove", "jet_bag_remove", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            PriorityQueueRemove { mode, .. } => match mode {
                ListRemoveMode::Value => b("priority_queue_remove_value", "jet_priority_queue_remove_value", 2, 2, &[true, false], Some(Effect::Mem), carrier),
                ListRemoveMode::Slot => b("priority_queue_remove_slot", "jet_priority_queue_remove_slot_canonical", 2, 2, &[true, false], Some(Effect::Mem), carrier),
                ListRemoveMode::Dynamic => primitive(),
            },
            LruPut => b("lru_put", "jet_lru_put", 3, 3, &[true, false, false], Some(Effect::Mem), carrier),
            LruAddNew => b("lru_add_new", "jet_lru_add_new", 3, 3, &[true, false, false], Some(Effect::Mem), carrier),
            LruGet => b("lru_get", "jet_lru_get", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            DequePushFront => b("deque_push_front", "jet_deque_push_front", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            ByteBufferMethod { method } => byte_buffer_method_route(method, carrier)?,
            StringMethod { method } => string_method_route(method, result, carrier)?,
            ByteBufferWrite { method } => byte_buffer_write_route(method, carrier)?,
            DequePushBack => b("deque_push_back", "jet_deque_push_back", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            DequePopFront => b("deque_pop_front", "jet_deque_pop_front_kernel", 1, 1, &[true], Some(Effect::Mem), carrier),
            DequePopBack => b("deque_pop_back", "jet_deque_pop_back_kernel", 1, 1, &[true], Some(Effect::Mem), carrier),
            DequeDelete => b("deque_delete", "jet_deque_delete", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            DequeReverse => b("deque_reverse", "jet_deque_reverse", 1, 1, &[true], Some(Effect::Mem), carrier),
            DequeSplit => b("deque_split", "jet_deque_split", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            SplitWrite { .. } => b("split_write", "jet_split_write", 2, 2, &[true, false], Some(Effect::Mem), carrier),
            SortedSetUnion => b("sorted_set_union", "jet_sorted_set_union", 2, 2, &[true, true], None, carrier),
            SortedSetIntersection => b("sorted_set_intersection", "jet_sorted_set_intersection", 2, 2, &[true, true], None, carrier),
            SortedSetDifference => b("sorted_set_difference", "jet_sorted_set_difference", 2, 2, &[true, true], None, carrier),
            SortedSetSymmetricDifference => b("sorted_set_symmetric_difference", "jet_sorted_set_symmetric_difference", 2, 2, &[true, true], None, carrier),
            SortedSetIsSubset => b("sorted_set_is_subset", "jet_sorted_set_is_subset", 2, 2, &[true, true], None, carrier),
            SortedSetIsSuperset => b("sorted_set_is_superset", "jet_sorted_set_is_superset", 2, 2, &[true, true], None, carrier),
            SortedSetIsDisjoint => b("sorted_set_is_disjoint", "jet_sorted_set_is_disjoint", 2, 2, &[true, true], None, carrier),
            TryCollect => b("try_collect", "jet_list_try_collect", 1, 1, &[false], None, carrier),
            BitSetCopy => b("bitset_copy", "jet_bits_copy", 1, 1, &[true], None, carrier),
            ViewNew { .. } => b("view_new", "jet_view_new", 4, 4, &[true, false, false, false], Some(Effect::Mem), carrier),
            ViewMutNew { .. } => b("view_mut_new", "jet_view_mut_new", 4, 4, &[true, false, false, false], Some(Effect::Mem), carrier),
            ComputeViewNew { .. } => b("compute_view_new", "jet_compute_view", 5, 5, &[true, false, false, false, false], Some(Effect::Mem), carrier),
            ComputeViewMutNew { .. } => b("compute_view_mut_new", "jet_compute_view_mut", 5, 5, &[true, false, false, false, false], Some(Effect::Mem), carrier),
            GetDisjointWrite => b("get_disjoint_write", "jet_get_disjoint_write", 2, 2, &[true, true], Some(Effect::Mem), carrier),
            // Receiver-sensitive, native, callback-building, or type-parameterized
            // forms are expanded by the TIR-to-MIR projection.
            Sum { float: true, f32: true } => b(
                "sum_fixed_f32",
                "jet_list_sum_fixed_f32",
                1,
                1,
                &[false],
                None,
                carrier,
            ),
            Sum { float: true, f32: false } => b(
                "sum_fixed_f64",
                "jet_list_sum_fixed_f64",
                1,
                1,
                &[false],
                None,
                carrier,
            ),
            Sum { float: false, .. } => b("sum", "jet_list_sum", 1, 1, &[false], None, carrier),
            Min { float: false, .. } => b("min", "jet_list_min", 1, 1, &[false], None, carrier),
            Max { float: false, .. } => b("max", "jet_list_max", 1, 1, &[false], None, carrier),
            ListMinMax { .. } => prelude(
                MirPreludeFamily::BuiltinMethod,
                "core.list",
                "min_max",
                "jet_list_min_max",
                1,
                1,
                &[true],
                None,
                carrier,
                MirPreludeAbi::Aggregate,
            ),
            Product { float: false, .. } => b("product", "jet_list_product", 1, 1, &[false], None, carrier),
            StringSplitOnce { .. } => b("string_split_once", "jet_unicode_split_once", 2, 2, &[true, true], None, carrier),
            StringCutLast { .. } => b("string_cut_last", "jet_unicode_cut_last", 2, 2, &[true, true], None, carrier),
            ParseFloat => b("float_parse", "jet_std::jet_float_parse", 1, 1, &[true], None, carrier),
            Repeat => b("string_repeat", "jet_string_repeat", 2, 2, &[true, false], None, carrier),
            StartsWith => b("list_starts_with", "jet_list_starts_with", 2, 2, &[true, true], None, carrier),
            ListSlice => b("list_slice", "jet_list_slice", 3, 3, &[true, false, false], None, carrier),
            ListBinarySearch => b(
                "list_binary_search",
                "jet_list_binary_search",
                2,
                2,
                &[true, true],
                None,
                carrier,
            ),
            ByteBufferToBytes => b(
                "byte_buffer_to_bytes",
                "JetByteBuffer::to_bytes",
                1,
                1,
                &[true],
                None,
                carrier,
            ),
            ByteBufferFrom => b(
                "byte_buffer_from",
                "JetByteBuffer::from",
                1,
                1,
                &[true],
                None,
                carrier,
            ),
            MapEqual => b("map_equal", "jet_map_equal", 2, 2, &[true, true], None, carrier),
            MapFirst => b("map_first", "jet_map_first_key", 1, 1, &[true], None, carrier),
            MapIntersection => b("map_intersection", "jet_map_intersection", 2, 2, &[true, true], None, carrier),
            MapSliceKeys { .. } => b("map_slice", "jet_map_slice", 2, 2, &[true, false], None, carrier),
            BagCount => b("bag_count", "jet_bag_count", 2, 2, &[true, true], None, carrier),
            DequePeekFront => b("deque_peek_front", "jet_deque_peek_front", 1, 1, &[true], None, carrier),
            PriorityQueuePeek => b("priority_queue_peek", "jet_priority_queue_peek", 1, 1, &[true], None, carrier),
            PriorityQueueFrom => b("priority_queue_from", "jet_priority_queue_from", 1, 1, &[false], None, carrier),
            BagHas => b("bag_has", "jet_bag_has", 2, 2, &[true, true], None, carrier),
            DequePeekBack => b("deque_peek_back", "jet_deque_peek_back", 1, 1, &[true], None, carrier),
            DequeCapacity => b("deque_capacity", "jet_deque_capacity", 1, 1, &[true], None, carrier),
            DequeContains => b("deque_contains", "jet_deque_contains", 2, 2, &[true, true], None, carrier),
            DequeGet => b("deque_get", "jet_deque_get", 2, 2, &[true, false], None, carrier),
            DequeToList => b("deque_to_list", "jet_deque_to_list", 1, 1, &[true], None, carrier),
            DequeJoin => b("deque_join", "jet_deque_join", 2, 2, &[true, true], None, carrier),
            DequeFrom => b("deque_from", "jet_deque_from", 1, 1, &[false], None, carrier),
            MapToList { .. } => prelude(
                MirPreludeFamily::BuiltinMethod,
                "core.map",
                "to_list",
                "jet_map_to_list",
                1,
                1,
                &[true],
                None,
                carrier,
                MirPreludeAbi::Aggregate,
            ),
            MapContainsValue => b("map_contains_value", "jet_map_contains_value", 2, 2, &[true, true], None, carrier),
            LenList | IsEmpty | GetMap | GetList | First | Last | Contains
            | IndexOf | JoinSep | Product { .. }
            | Min { float: true, .. } | Max { float: true, .. } | Unzip { .. } | Chars | EndsWith | Replace
            | ContainsKey | ToString | Take | Skip | IterToList | IterCollect
            | ListLazy | StepBy | Dedup | Chunks | Windows | IterRepeat | IterCycle
            | IterDropLast | IterShuffle | IterIsSorted | IterLastIndexOf | IterAverage { .. }
            | IterCompare | ListCopy
            | ListUnion | ListIntersection | ListDifference | ListRandom
            | MapCopy
            | MapNew
            | Indexes | Zip { .. } | OptionZip { .. }
            | SetToList | SetCopy | SetEqual | SetCapacity | SetFirst | SetSort
            | SetShuffle
            | PriorityQueueToSortedList
            | BitSetCount | BitSetToList | BitSetNew | ByteBufferNew
            | BagLen => primitive(),
        };
        Ok(plan)
    }

    pub(super) fn prelude_route(
        &self,
        receiver: &Type,
        result: &Type,
        carrier: &TFailureCarrier,
    ) -> Result<TPreludeRoute, LowerError> {
        let plan = if let Some(plan) = iterator_builtin_route(self, receiver, carrier) {
            plan
        } else {
            match self {
                TBuiltinOp::JoinSep => b(
                    "join",
                    "jet_list_join",
                    2,
                    2,
                    &[true, true],
                    None,
                    carrier,
                ),
                TBuiltinOp::StartsWith if is_string_receiver(receiver) => b(
                    "string_starts_with",
                    "jet_string_starts_with",
                    2,
                    2,
                    &[true, true],
                    None,
                    carrier,
                ),
                TBuiltinOp::StartsWith | TBuiltinOp::EndsWith if !is_string_receiver(receiver) => b(
                    if matches!(self, TBuiltinOp::StartsWith) { "list_starts_with" } else { "list_ends_with" },
                    if matches!(self, TBuiltinOp::StartsWith) { "jet_list_starts_with" } else { "jet_list_ends_with" },
                    2, 2, &[true, true], None, carrier,
                ),
                TBuiltinOp::Contains if is_string_receiver(receiver) => b(
                    "contains",
                    "jet_string_contains",
                    2,
                    2,
                    &[true, true],
                    None,
                    carrier,
                ),
                TBuiltinOp::LenList
                | TBuiltinOp::Contains
                | TBuiltinOp::IsEmpty
                | TBuiltinOp::DequeCapacity
                | TBuiltinOp::GetMap
                | TBuiltinOp::GetList
                | TBuiltinOp::First
                | TBuiltinOp::Last => builtin_collection_route(self, receiver, carrier)?,
                _ => self.route_plan(result, carrier)?,
            }
        };
        as_prelude(plan, "builtin method")
    }
}

fn b(
    member: &str,
    symbol: &str,
    arity: usize,
    max_arity: usize,
    borrow_mask: &[bool],
    effect: Option<Effect>,
    carrier: &TFailureCarrier,
) -> TRoutePlan {
    prelude(
        MirPreludeFamily::BuiltinMethod,
        "core.builtin",
        member,
        symbol,
        arity,
        max_arity,
        borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Value,
    )
}

fn atomic_method_route(
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (member, symbol, arity, borrow_mask, effect) = match method {
        "load" => ("load", "jet_atomic_load", 1, &[true][..], None),
        "store" => ("store", "jet_atomic_store", 2, &[true, false][..], None),
        "add" => ("add", "jet_atomic_add", 2, &[true, false][..], None),
        "try_add" => (
            "try_add",
            "jet_atomic_try_add",
            2,
            &[true, false][..],
            Some(Effect::Mem),
        ),
        "compare_exchange" => (
            "compare_exchange",
            "jet_atomic_compare_exchange",
            3,
            &[true, false, false][..],
            None,
        ),
        "publish" => ("publish", "jet_atomic_publish", 2, &[true, false][..], None),
        "observe" => ("observe", "jet_atomic_observe", 1, &[true][..], None),
        _ => return Err(route_error(format!("unknown checked Atomic method `{method}`"))),
    };
    Ok(prelude(
        MirPreludeFamily::BuiltinMethod,
        "core.mem",
        member,
        symbol,
        arity,
        arity,
        borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Value,
    ))
}

fn byte_buffer_method_route(
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    if method == "capacity" {
        return Ok(b(
            "byte_buffer_capacity",
            "JetByteBuffer::capacity",
            1,
            1,
            &[true],
            None,
            carrier,
        ));
    }
    let arity = match method {
        "len"
        | "is_empty"
        | "clear"
        | "position"
        | "eof"
        | "rewind"
        | "flush"
        | "close"
        | "shutdown"
        | "get_buffer"
        | "buffer"
        | "to_string"
        | "string"
        | "trim"
        | "trim_start"
        | "trim_end"
        | "to_lower"
        | "to_upper"
        | "to_title"
        | "title"
        | "clone"
        | "copy"
        | "lines"
        | "first"
        | "next"
        | "read_byte"
        | "read"
        | "is_ascii" => 1,
        "get"
        | "seek"
        | "read_bytes"
        | "read_string"
        | "contains"
        | "starts_with"
        | "ends_with"
        | "index_of"
        | "last_index_of"
        | "split"
        | "join"
        | "equal"
        | "compare"
        | "copy_to"
        | "write_to" => 2,
        "replace" => 3,
        _ => return Ok(primitive()),
    };
    let member = format!("byte_buffer_{method}");
    let symbol = format!("JetByteBuffer::{method}");
    Ok(b(
        &member,
        &symbol,
        arity,
        arity,
        &vec![true; arity],
        Some(Effect::Mem),
        carrier,
    ))
}

fn byte_buffer_write_route(
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let member = format!("byte_buffer_{method}");
    let symbol = format!("JetByteBuffer::{method}");
    Ok(b(
        &member,
        &symbol,
        2,
        2,
        &[true, false],
        Some(Effect::Mem),
        carrier,
    ))
}

pub(super) fn string_method_route(
    method: &str,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (member, symbol, arity) = match method {
        "count_bytes" => ("count_bytes", "jet_string_count_bytes", 1),
        "last_index_of" => ("last_index_of", "jet_unicode_last_index_of", 2),
        "is_lower" => ("is_lower", "jet_text_is_lower", 1),
        "is_upper" => ("is_upper", "jet_text_is_upper", 1),
        "capitalize" => ("capitalize", "jet_text_capitalize", 1),
        "swapcase" => ("swapcase", "jet_text_swapcase", 1),
        "remove_prefix" => ("remove_prefix", "jet_text_remove_prefix", 2),
        "remove_suffix" => ("remove_suffix", "jet_text_remove_suffix", 2),
        "compare" => ("compare", "jet_text_compare", 2),
        "reverse" => ("reverse", "jet_text_reverse", 1),
        "normalize" => ("normalize", "jet_text_normalize_nfc", 1),
        "rsplit" => ("rsplit", "jet_iter_string_rsplit", 2),
        "matches" => ("matches", "jet_std::jet_regex_compile", 2),
        "match" => ("match", "jet_std::jet_regex_compile", 2),
        _ => return Err(route_error(format!("unknown checked String method `{method}`"))),
    };
    let _ = result;
    Ok(b(member, symbol, arity, arity, &vec![true; arity], None, carrier))
}

fn overflow_input_type(ty: &Type) -> &Type {
    let ty = ty.without_user_tags();
    match ty {
        Type::InlineRange { base, .. } => overflow_input_type(base),
        Type::Tagged { inner, .. } => overflow_input_type(inner),
        other => other,
    }
}

pub(super) fn fixed_width_name(ty: &Type) -> Option<&'static str> {
    match overflow_input_type(ty) {
        Type::IntN {
            signed: true,
            bits: 8,
        } => Some("i8"),
        Type::IntN {
            signed: true,
            bits: 16,
        } => Some("i16"),
        Type::IntN {
            signed: true,
            bits: 32,
        } => Some("i32"),
        Type::IntN {
            signed: true,
            bits: 64,
        } => Some("i64"),
        Type::IntN {
            signed: false,
            bits: 8,
        } => Some("u8"),
        Type::IntN {
            signed: false,
            bits: 16,
        } => Some("u16"),
        Type::IntN {
            signed: false,
            bits: 32,
        } => Some("u32"),
        Type::IntN {
            signed: false,
            bits: 64,
        } => Some("u64"),
        _ => None,
    }
}

fn numeric_binary(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::FloorDiv
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
    )
}

fn exact_int_binary_route(
    op: BinOp,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let (member, symbol, arity, effect) = match op {
        BinOp::Add => ("add", "jet_std::jet_int_add", 2, None),
        BinOp::Sub => ("sub", "jet_std::jet_int_sub", 2, None),
        BinOp::Mul => ("mul", "jet_std::jet_int_mul", 2, None),
        BinOp::BitAnd => ("bit_and", "jet_std::jet_int_bit_and", 2, None),
        BinOp::BitOr => ("bit_or", "jet_std::jet_int_bit_or", 2, None),
        BinOp::BitXor => ("bit_xor", "jet_std::jet_int_bit_xor", 2, None),
        BinOp::Div => ("div", "jet_std::jet_int_div", 4, Some(Effect::Panic)),
        BinOp::Rem => ("rem", "jet_std::jet_int_rem", 4, Some(Effect::Panic)),
        BinOp::FloorDiv => ("floor_div", "jet_std::jet_int_floor_div", 4, Some(Effect::Panic)),
        BinOp::Mod => ("mod", "jet_std::jet_int_mod", 4, Some(Effect::Panic)),
        BinOp::Pow => ("pow", "jet_std::jet_int_pow", 4, Some(Effect::Panic)),
        BinOp::Shl => ("shl", "jet_std::jet_int_shl", 4, Some(Effect::Panic)),
        BinOp::Shr => ("shr", "jet_std::jet_int_shr", 4, Some(Effect::Panic)),
        _ => return Ok(TRoutePlan::Primitive),
    };
    let borrow_mask = vec![false; arity];
    Ok(prelude(
        MirPreludeFamily::Overflow,
        "core.numeric",
        member,
        symbol,
        arity,
        arity,
        &borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Value,
    ))
}

fn fixed_trap_binary_route(
    width: &str,
    op: BinOp,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let op_name = match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::Rem => "rem",
        BinOp::FloorDiv => "floor_div",
        BinOp::Mod => "mod",
        BinOp::Pow => "pow",
        BinOp::Shl => "shl",
        BinOp::Shr => "shr",
        _ => return Ok(TRoutePlan::Primitive),
    };
    let member = format!("{width}.trap.{op_name}");
    let symbol = format!("jet_{width}_trap_{op_name}");
    prelude_route_row(
        MirPreludeFamily::Overflow,
        "core.numeric",
        &member,
        &symbol,
        4,
        4,
        &[false, false, false, false],
        Some(Effect::Panic),
        carrier,
        MirPreludeAbi::Value,
        "fixed-width trap arithmetic",
    )
    .map(TRoutePlan::Prelude)
}

fn math_type_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(name) if crate::Sema::is_math_type(name) => Some(name),
        Type::Tagged { inner, .. } => math_type_name(inner),
        _ => None,
    }
}

fn math_binary_route(
    op: BinOp,
    input: &Type,
    rhs: &Type,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<Option<TRoutePlan>, LowerError> {
    let Some(left_name) = math_type_name(input) else {
        return Ok(None);
    };
    let Some(right_name) = math_type_name(rhs) else {
        return Ok(None);
    };
    let Some(result_name) = math_type_name(result) else {
        return Ok(None);
    };
    let Some(op_name) = (match op {
        BinOp::Add => Some("add"),
        BinOp::Sub => Some("sub"),
        BinOp::Mul => Some("mul"),
        BinOp::Div => Some("div"),
        _ => None,
    }) else {
        return Ok(None);
    };
    let symbol = match op {
        BinOp::Add | BinOp::Sub
            if left_name == right_name && result_name == left_name =>
        {
            format!("jet_math_{left_name}_{op_name}")
        }
        BinOp::Div
            if left_name == right_name
                && result_name == left_name
                && crate::Sema::is_simd_lane_type(left_name) =>
        {
            format!("jet_math_{left_name}_{op_name}")
        }
        BinOp::Mul if left_name == right_name && result_name == left_name => {
            if left_name == "Vec3" {
                "jet_math_Vec3_hadamard_mul".to_string()
            } else {
                format!("jet_math_{left_name}_{op_name}")
            }
        }
        BinOp::Mul if left_name == "Mat3" && right_name == "Vec3" && result_name == "Vec3" => {
            "jet_math_Mat3_transform".to_string()
        }
        BinOp::Mul if left_name == "Mat4" && right_name == "Vec4" && result_name == "Vec4" => {
            "jet_math_Mat4_transform".to_string()
        }
        _ => return Ok(None),
    };
    let member = format!("binary.{left_name}.{op_name}");
    let route = prelude_route_row(
        MirPreludeFamily::MathBuiltin,
        "core.math",
        &member,
        &symbol,
        2,
        2,
        &[true, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "math binary",
    )?;
    Ok(Some(TRoutePlan::Prelude(route)))
}

pub(super) fn binary_route(
    op: BinOp,
    overflow: bool,
    input: &Type,
    rhs: &Type,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    if let Some(route) = math_binary_route(op, input, rhs, result, carrier)? {
        return Ok(route);
    }
    if input == &Type::Int {
        if numeric_binary(op) && result != &Type::Int {
            return Err(route_error(
                "exact Int arithmetic has a non-Int resolved result type",
            ));
        }
        return exact_int_binary_route(op, carrier);
    }
    if !numeric_binary(op) {
        return Ok(TRoutePlan::Primitive);
    }
    let Some(width) = fixed_width_name(input) else {
        return if overflow {
            Err(route_error(
                "checked arithmetic has no resolved fixed numeric type",
            ))
        } else {
            Ok(TRoutePlan::Primitive)
        };
    };
    if fixed_width_name(result) != Some(width) {
        return Err(route_error(
            "fixed-width arithmetic has a mismatched resolved result type",
        ));
    }
    if !overflow && !matches!(op, BinOp::Rem | BinOp::FloorDiv | BinOp::Mod | BinOp::Pow) {
        return Ok(TRoutePlan::Primitive);
    }
    fixed_trap_binary_route(width, op, carrier)
}

pub(super) fn compare_route(
    op: BinOp,
    hook: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let _ = result;
    if !hook {
        return Ok(TRoutePlan::Primitive);
    }
    match op {
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne => Ok(prelude(
            MirPreludeFamily::BuiltinMethod,
            "core.compare",
            op.spell(),
            "jet_int_compare",
            2,
            2,
            &[true, true],
            None,
            carrier,
            MirPreludeAbi::Value,
        )),
        BinOp::Compare => Ok(prelude(
            MirPreludeFamily::BuiltinMethod,
            "core.compare",
            "compare",
            "jet_int_compare",
            2,
            2,
            &[true, true],
            None,
            carrier,
            MirPreludeAbi::Value,
        )),
        _ => Ok(TRoutePlan::Primitive),
    }
}


fn runtime(
    family: MirPreludeFamily,
    module: &str,
    member: &str,
    symbol: &str,
    arity: usize,
    max_arity: usize,
    borrow_mask: &[bool],
    effect: Option<Effect>,
    carrier: &TFailureCarrier,
    abi: MirPreludeAbi,
) -> TRoutePlan {
    TRoutePlan::Prelude(TPreludeRoute {
        family,
        module: module.to_string(),
        member: member.to_string(),
        symbol: MirSymbol::Runtime(symbol.to_string()),
        signature: MirCallSignature {
            arity,
            max_arity,
            borrow_mask: borrow_mask.to_vec(),
        },
        effect,
        fallibility: carrier.clone(),
        authority: None,
        abi,
        db_metadata: None,
    })
}
pub(super) fn civil_time_route(
    kind: &str,
    method: &str,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    if !super::is_civil_time_method_name(Some(kind), method) {
        return Err(route_error(format!(
            "unknown checked civil time method `{kind}.{method}`"
        )));
    }
    let (symbol, arity, borrow_mask) = match (kind, method) {
        ("Date" | "LocalDate", "year") => ("JetDate::year", 1, &[true][..]),
        ("Date" | "LocalDate", "month") => ("JetDate::month", 1, &[true][..]),
        ("Date" | "LocalDate", "day") => ("JetDate::day", 1, &[true][..]),
        ("Date" | "LocalDate", "weekday") => ("JetDate::weekday", 1, &[true][..]),
        ("Date" | "LocalDate", "iso_weekday") => ("JetDate::iso_weekday", 1, &[true][..]),
        ("Date" | "LocalDate", "day_of_year") => ("JetDate::day_of_year", 1, &[true][..]),
        ("Date" | "LocalDate", "iso_week") => ("JetDate::iso_week", 1, &[true][..]),
        ("Date" | "LocalDate", "iso_week_year") => ("JetDate::iso_week_year", 1, &[true][..]),
        ("Date" | "LocalDate", "quarter_of_year") => {
            ("JetDate::quarter_of_year", 1, &[true][..])
        }
        ("Date" | "LocalDate", "days_in_month") => ("JetDate::days_in_month", 1, &[true][..]),
        ("Date" | "LocalDate", "is_leap_year") => ("JetDate::is_leap_year", 1, &[true][..]),
        ("Date" | "LocalDate", "to_string") => ("JetDate::to_string_fmt", 1, &[true][..]),
        ("Date" | "LocalDate", "add_days") => ("JetDate::add_days", 2, &[true, false][..]),
        ("Date" | "LocalDate", "add_months") => ("JetDate::add_months", 2, &[true, false][..]),
        ("Date" | "LocalDate", "add_period") => {
            ("JetDate::add_period_value", 2, &[true, false][..])
        }
        ("Date" | "LocalDate", "subtract_period") => {
            ("JetDate::subtract_period_value", 2, &[true, false][..])
        }
        ("Date" | "LocalDate", "diff_days") => {
            ("JetDate::diff_days_value", 2, &[true, false][..])
        }
        ("Date" | "LocalDate", "truncate") => ("JetDate::truncate", 2, &[true, true][..]),
        ("Date" | "LocalDate", "replace") => {
            ("JetDate::replace", 4, &[true, false, false, false][..])
        }
        ("Date" | "LocalDate", "format") => ("JetDate::format_pattern", 2, &[true, true][..]),
        ("Date" | "LocalDate", "format_checked") => {
            ("JetDate::format_checked_text", 2, &[true, true][..])
        }
        ("Date" | "LocalDate", "until") => (
            "JetDate::until_duration",
            6,
            &[true, false, true, true, true, false][..],
        ),
        ("Date" | "LocalDate", "since") => (
            "JetDate::since_duration",
            6,
            &[true, false, true, true, true, false][..],
        ),
        ("Date" | "LocalDate", "with") => (
            "JetDate::with_overflow",
            5,
            &[true, false, false, false, true][..],
        ),
        ("Date" | "LocalDate", "equal") => ("JetDate::equal_value", 2, &[true, false][..]),
        ("Date" | "LocalDate", "compare") => ("JetDate::compare_value", 2, &[true, false][..]),
        ("LocalTime", "hour") => ("JetLocalTime::hour", 1, &[true][..]),
        ("LocalTime", "minute") => ("JetLocalTime::minute", 1, &[true][..]),
        ("LocalTime", "second") => ("JetLocalTime::second", 1, &[true][..]),
        ("LocalTime", "millisecond") => ("JetLocalTime::millisecond", 1, &[true][..]),
        ("LocalTime", "microsecond") => ("JetLocalTime::microsecond", 1, &[true][..]),
        ("LocalTime", "nanosecond") => ("JetLocalTime::nanosecond", 1, &[true][..]),
        ("LocalTime", "to_string") => ("JetLocalTime::to_string_fmt", 1, &[true][..]),
        ("LocalTime", "add_duration") => {
            ("JetLocalTime::add_duration_value", 2, &[true, false][..])
        }
        ("LocalTime", "subtract_duration") => {
            ("JetLocalTime::subtract_duration_value", 2, &[true, false][..])
        }
        ("LocalTime", "round") => ("JetLocalTime::round_with", 4, &[true, true, false, true][..]),
        ("LocalTime", "truncate") => {
            ("JetLocalTime::truncate_with", 3, &[true, true, false][..])
        }
        ("LocalTime", "floor") => ("JetLocalTime::floor_with", 3, &[true, true, false][..]),
        ("LocalTime", "ceil") => ("JetLocalTime::ceil_with", 3, &[true, true, false][..]),
        ("LocalTime", "until") => (
            "JetLocalTime::until_duration",
            6,
            &[true, false, true, true, true, false][..],
        ),
        ("LocalTime", "since") => (
            "JetLocalTime::since_duration",
            6,
            &[true, false, true, true, true, false][..],
        ),
        ("LocalTime", "format") => ("JetLocalTime::format_pattern", 2, &[true, true][..]),
        ("LocalTime", "format_checked") => {
            ("JetLocalTime::format_checked_text", 2, &[true, true][..])
        }
        ("LocalTime", "equal") => ("JetLocalTime::equal_value", 2, &[true, false][..]),
        ("LocalTime", "compare") => ("JetLocalTime::compare_value", 2, &[true, false][..]),
        ("DateTime", "hour") => ("JetDateTime::hour", 1, &[true][..]),
        ("DateTime", "minute") => ("JetDateTime::minute", 1, &[true][..]),
        ("DateTime", "second") => ("JetDateTime::second", 1, &[true][..]),
        ("DateTime", "millisecond") => ("JetDateTime::millisecond", 1, &[true][..]),
        ("DateTime", "microsecond") => ("JetDateTime::microsecond", 1, &[true][..]),
        ("DateTime", "nanosecond") => ("JetDateTime::nanosecond", 1, &[true][..]),
        ("DateTime", "to_timestamp") => ("JetDateTime::to_timestamp", 1, &[true][..]),
        ("DateTime", "to_unix_ms") => ("JetDateTime::to_unix_ms_value", 1, &[true][..]),
        ("DateTime", "to_unix_s") => ("JetDateTime::to_unix_seconds_value", 1, &[true][..]),
        ("DateTime", "to_unix_us") => (
            "JetDateTime::to_unix_microseconds_value",
            1,
            &[true][..],
        ),
        ("DateTime", "to_unix_ns") => ("JetDateTime::to_unix_nanoseconds_value", 1, &[true][..]),
        ("DateTime", "date") => ("JetDateTime::date", 1, &[true][..]),
        ("DateTime", "time") => ("JetDateTime::time_for_output", 1, &[true][..]),
        ("DateTime", "to_string") => ("JetDateTime::to_string_fmt", 1, &[true][..]),
        ("DateTime", "format_rfc3339") => ("JetDateTime::format_rfc3339", 1, &[true][..]),
        ("DateTime", "format") => ("JetDateTime::format_pattern", 2, &[true, true][..]),
        ("DateTime", "format_checked") => {
            ("JetDateTime::format_checked_text", 2, &[true, true][..])
        }
        ("DateTime", "plus_duration") => {
            ("JetDateTime::plus_duration_value", 2, &[true, false][..])
        }
        ("DateTime", "subtract_duration") => {
            ("JetDateTime::subtract_duration_value", 2, &[true, false][..])
        }
        ("DateTime", "add_nanoseconds") => {
            ("JetDateTime::add_nanoseconds", 2, &[true, false][..])
        }
        ("DateTime", "difference") => {
            ("JetDateTime::difference_duration", 2, &[true, false][..])
        }
        ("DateTime", "add_period") => {
            ("JetDateTime::add_period_value", 2, &[true, false][..])
        }
        ("DateTime", "subtract_period") => {
            ("JetDateTime::subtract_period_value", 2, &[true, false][..])
        }
        ("DateTime", "until") => (
            "JetDateTime::until_duration",
            6,
            &[true, false, true, true, true, false][..],
        ),
        ("DateTime", "since") => (
            "JetDateTime::since_duration",
            6,
            &[true, false, true, true, true, false][..],
        ),
        ("DateTime", "truncate") => {
            ("JetDateTime::truncate_with", 3, &[true, true, false][..])
        }
        ("DateTime", "round") => ("JetDateTime::round_with", 4, &[true, true, false, true][..]),
        ("DateTime", "floor") => ("JetDateTime::floor_with", 3, &[true, true, false][..]),
        ("DateTime", "ceil") => ("JetDateTime::ceil_with", 3, &[true, true, false][..]),
        ("DateTime", "replace") => (
            "JetDateTime::replace",
            7,
            &[true, false, false, false, false, false, false][..],
        ),
        ("DateTime", "in_zone") => ("JetDateTime::in_zone_value", 2, &[true, false][..]),
        ("DateTime", "with") => (
            "JetDateTime::with_overflow",
            8,
            &[true, false, false, false, false, false, false, true][..],
        ),
        ("DateTime", "equal") => ("JetDateTime::equal_value", 2, &[true, false][..]),
        ("DateTime", "compare") => ("JetDateTime::compare_value", 2, &[true, false][..]),
        ("Instant", "elapsed_millis") => ("JetInstant::elapsed_millis", 1, &[true][..]),
        ("Instant", "elapsed") => ("JetInstant::elapsed_duration", 1, &[true][..]),
        ("Instant", "equal") => ("JetInstant::equal_value", 2, &[true, false][..]),
        ("Instant", "compare") => ("JetInstant::compare_value", 2, &[true, false][..]),
        ("Period", "years") => ("JetPeriod::years_value", 1, &[true][..]),
        ("Period", "months") => ("JetPeriod::months_value", 1, &[true][..]),
        ("Period", "days") => ("JetPeriod::days_value", 1, &[true][..]),
        ("Period", "sign") => ("JetPeriod::sign", 1, &[true][..]),
        ("Period", "is_zero") => ("JetPeriod::is_zero", 1, &[true][..]),
        ("Period", "abs") => ("JetPeriod::abs", 1, &[true][..]),
        ("Period", "negated") => ("JetPeriod::negated", 1, &[true][..]),
        ("Period", "add") => ("JetPeriod::add_value", 2, &[true, false][..]),
        ("Period", "sub") => ("JetPeriod::sub_value", 2, &[true, false][..]),
        ("Period", "total_in") => ("JetPeriod::total_in_value", 3, &[true, true, true][..]),
        ("Period", "to_string") => ("JetPeriod::to_string_fmt", 1, &[true][..]),
        ("Zone", "name") => ("JetZone::name", 1, &[true][..]),
        ("Zone", "next_transition") => (
            "JetZone::next_transition_value",
            2,
            &[true, false][..],
        ),
        ("Zone", "previous_transition") => (
            "JetZone::previous_transition_value",
            2,
            &[true, false][..],
        ),
        ("Zone", "start_of_day") => ("JetZone::start_of_day_value", 2, &[true, false][..]),
        ("Zone", "hours_in_day") => ("JetZone::hours_in_day_value", 2, &[true, false][..]),
        ("ZonedDateTime", "date") => ("JetZonedDateTime::date", 1, &[true][..]),
        ("ZonedDateTime", "time") => ("JetZonedDateTime::time", 1, &[true][..]),
        ("ZonedDateTime", "offset_seconds") => ("JetZonedDateTime::offset_seconds", 1, &[true][..]),
        ("ZonedDateTime", "is_dst") => ("JetZonedDateTime::is_dst", 1, &[true][..]),
        ("ZonedDateTime", "to_datetime") => ("JetZonedDateTime::to_datetime", 1, &[true][..]),
        ("ZonedDateTime", "zone") => ("JetZonedDateTime::zone", 1, &[true][..]),
        ("ZonedDateTime", "to_string") => ("JetZonedDateTime::to_string_fmt", 1, &[true][..]),
        ("ZonedDateTime", "format") => ("JetZonedDateTime::format_pattern", 2, &[true, true][..]),
        ("ZonedDateTime", "format_rfc9557") => {
            ("JetZonedDateTime::format_rfc9557", 1, &[true][..])
        }
        ("ZonedDateTime", "format_checked") => (
            "JetZonedDateTime::format_checked_text",
            2,
            &[true, true][..],
        ),
        ("ZonedDateTime", "add_duration") => (
            "JetZonedDateTime::add_duration_value",
            2,
            &[true, false][..],
        ),
        ("ZonedDateTime", "subtract_duration") => (
            "JetZonedDateTime::subtract_duration_value",
            2,
            &[true, false][..],
        ),
        ("ZonedDateTime", "add_period") => (
            "JetZonedDateTime::add_period_value",
            2,
            &[true, false][..],
        ),
        ("ZonedDateTime", "subtract_period") => (
            "JetZonedDateTime::subtract_period_value",
            2,
            &[true, false][..],
        ),
        ("ZonedDateTime", "with_time") => (
            "JetZonedDateTime::with_time",
            3,
            &[true, true, true][..],
        ),
        ("ZonedDateTime", "with_zone") => ("JetZonedDateTime::with_zone", 2, &[true, true][..]),
        ("ZonedDateTime", "until") => (
            "JetZonedDateTime::until_duration",
            6,
            &[true, false, true, true, true, false][..],
        ),
        ("ZonedDateTime", "since") => (
            "JetZonedDateTime::since_duration",
            6,
            &[true, false, true, true, true, false][..],
        ),
        ("ZonedDateTime", "next_transition") => (
            "JetZonedDateTime::next_transition_value",
            1,
            &[true][..],
        ),
        ("ZonedDateTime", "previous_transition") => (
            "JetZonedDateTime::previous_transition_value",
            1,
            &[true][..],
        ),
        ("ZonedDateTime", "start_of_day") => ("JetZonedDateTime::start_of_day", 1, &[true][..]),
        ("ZonedDateTime", "hours_in_day") => ("JetZonedDateTime::hours_in_day", 1, &[true][..]),
        ("ZonedDateTime", "equal") => ("JetZonedDateTime::equal_value", 2, &[true, false][..]),
        ("ZonedDateTime", "compare") => ("JetZonedDateTime::compare_value", 2, &[true, false][..]),
        _ => unreachable!("civil time method whitelist and route table diverged"),
    };
    let member = format!("{kind}.{method}");
    Ok(runtime(
        MirPreludeFamily::HandleMethod,
        "core.time",
        &member,
        symbol,
        arity,
        arity,
        borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
    ))
}


pub(super) fn layout_compare_route(
    op: MirLayoutCompareOp,
    _result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let (member, symbol) = match op {
        MirLayoutCompareOp::Equal => ("eq", "jet_layout::eq_"),
        MirLayoutCompareOp::LessEqual => ("le", "jet_layout::le"),
        MirLayoutCompareOp::GreaterEqual => ("ge", "jet_layout::ge"),
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.layout",
        member,
        symbol,
        2,
        2,
        &[false, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "layout comparison",
    )
}

pub(super) fn condition_notify_route(
    all: bool,
    _result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let (member, symbol) = if all {
        ("notify_all", "jet_std::JetCondition::notify_all")
    } else {
        ("notify_one", "jet_std::JetCondition::notify_one")
    };
    prelude_route_row(
        MirPreludeFamily::HandleMethod,
        "core.shared_guard",
        member,
        symbol,
        1,
        1,
        &[true],
        None,
        carrier,
        MirPreludeAbi::Value,
        "condition notification",
    )
}

pub(super) fn alloc_new_route(
    ctor: &super::TAllocCtor,
    _result: &Type,
    arg_count: usize,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let (member, symbol, arity, max_arity, borrow_mask) = match ctor {
        super::TAllocCtor::Arena if arg_count == 0 => (
            "arena.new",
            "jet_mem::JetArena::new",
            0,
            0,
            &[][..],
        ),
        super::TAllocCtor::Arena => (
            "arena.new",
            "jet_mem::JetArena::with_capacity",
            1,
            1,
            &[false][..],
        ),
        super::TAllocCtor::Bump if arg_count == 0 => (
            "bump.new",
            "jet_mem::JetBump::new",
            0,
            0,
            &[][..],
        ),
        super::TAllocCtor::Bump => (
            "bump.new",
            "jet_mem::JetBump::with_capacity",
            1,
            1,
            &[false][..],
        ),
        super::TAllocCtor::Pool => (
            "pool.new",
            "jet_mem::JetPool::with_slots",
            1,
            1,
            &[false][..],
        ),
        super::TAllocCtor::Fixed { .. } => (
            "fixed.new",
            "jet_mem::JetFixed::new",
            1,
            1,
            &[false][..],
        ),
        super::TAllocCtor::FixedOver => (
            "fixed.over",
            "jet_mem::JetFixed::over",
            1,
            1,
            &[true][..],
        ),
    };
    if arg_count < arity || arg_count > max_arity {
        return Err(route_error(format!(
            "allocator constructor route `{member}` accepts {arity}..={max_arity} argument(s), got {arg_count}"
        )));
    }
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.mem",
        member,
        symbol,
        arity,
        max_arity,
        borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "allocator constructor",
    )
}

fn prelude_route_row(
    family: MirPreludeFamily,
    module: &str,
    member: &str,
    symbol: &str,
    arity: usize,
    max_arity: usize,
    borrow_mask: &[bool],
    effect: Option<Effect>,
    carrier: &TFailureCarrier,
    abi: MirPreludeAbi,
    context: &str,
) -> Result<TPreludeRoute, LowerError> {
    as_prelude(
        prelude(
            family,
            module,
            member,
            symbol,
            arity,
            max_arity,
            borrow_mask,
            effect,
            carrier,
            abi,
        ),
        context,
    )
}

fn runtime_route_row(
    family: MirPreludeFamily,
    module: &str,
    member: &str,
    symbol: &str,
    arity: usize,
    max_arity: usize,
    borrow_mask: &[bool],
    effect: Option<Effect>,
    carrier: &TFailureCarrier,
    abi: MirPreludeAbi,
    context: &str,
) -> Result<TPreludeRoute, LowerError> {
    as_prelude(
        runtime(
            family,
            module,
            member,
            symbol,
            arity,
            max_arity,
            borrow_mask,
            effect,
            carrier,
            abi,
        ),
        context,
    )
}

/// Select the checked index kernel from the sema-selected index kind and
/// access. The kind and access are compile-time MIR facts; they never become
/// runtime arguments. Every index ABI carries only base, index, source path,
/// and source line.
pub(super) fn index_route(
    kind: MirIndexKind,
    access: MirAccess,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol, arity, borrow_mask) = match (kind, access) {
        (MirIndexKind::List | MirIndexKind::FixedListProof, MirAccess::Read) => {
            ("index_list", "jet_index_vec", 4, vec![true, false, true, false])
        }
        (MirIndexKind::List | MirIndexKind::FixedListProof, MirAccess::Write) => {
            ("index_list_mut", "jet_index_vec_mut", 4, vec![true, false, true, false])
        }
        (MirIndexKind::List | MirIndexKind::FixedListProof, MirAccess::Move) => {
            return Err(route_error("index move has no checked Prelude kernel"));
        }
        (MirIndexKind::Map, MirAccess::Read) => (
            "index_map",
            "jet_index_map",
            8,
            vec![true, true, true, false, true, true, false, false],
        ),
        (MirIndexKind::Map, MirAccess::Write) => (
            "index_map_mut",
            "jet_index_map_mut",
            8,
            vec![true, true, true, false, true, true, false, false],
        ),
        (MirIndexKind::Map, MirAccess::Move) => {
            return Err(route_error("map index move has no checked Prelude kernel"));
        }
        (MirIndexKind::Pool, MirAccess::Read) => (
            "index_pool",
            "jet_std::jet_pool_get",
            6,
            vec![true, false, true, false, true, true],
        ),
        (MirIndexKind::Pool, MirAccess::Write) => (
            "index_pool_mut",
            "jet_std::jet_pool_get_mut",
            6,
            vec![true, false, true, false, true, true],
        ),
        (MirIndexKind::Pool, MirAccess::Move) => {
            return Err(route_error("pool index move has no checked Prelude kernel"));
        }
        (MirIndexKind::Lane, _) => {
            return Err(route_error(
                "lane index requires the checked lane type route",
            ));
        }
    };
    let _ = result;
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.index",
        member,
        symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "index operation",
    )
}
/// Route an indexed assignment through a typed setter. The index kind is
/// sema-resolved and therefore never crosses the ABI as a runtime descriptor.
pub(super) fn index_write_route(
    kind: MirIndexKind,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol, arity, borrow_mask) = match kind {
        MirIndexKind::List | MirIndexKind::FixedListProof => (
            "index_list_set",
            "jet_index_vec_set",
            5,
            vec![true, false, false, true, false],
        ),
        MirIndexKind::Map => (
            "index_map_set",
            "jet_map_insert",
            3,
            vec![true, false, false],
        ),
        MirIndexKind::Pool => (
            "index_pool_set",
            "jet_std::jet_pool_set",
            7,
            vec![true, false, false, true, false, true, true],
        ),
        MirIndexKind::Lane => {
            return Err(route_error(
                "lane index assignment has no checked typed setter route",
            ));
        }
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.index",
        member,
        symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "index assignment",
    )
}

/// Route a mapped guard through the shared lock protocol. The projection
/// identity is selected by sema; `field` is the adapter's canonical path
/// bridge and is not a runtime kind descriptor.
pub(super) fn shared_guard_map_route(
    editable: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let member = if editable { "map_edit" } else { "map_read" };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.shared_guard",
        member,
        "jet_shared_guard_map",
        3,
        3,
        &[true, false, false],
        None,
        carrier,
        MirPreludeAbi::Control,
        "shared guard map",
    )
}

/// Route a disjoint guard split through the shared lock protocol. Rust lowers
/// the row to the typed `split_read`/`split_edit` method, while other adapters
/// use the same four-argument protocol shape.
pub(super) fn shared_guard_split_route(
    editable: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let member = if editable { "split_edit" } else { "split_read" };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.shared_guard",
        member,
        "jet_shared_guard_split",
        4,
        4,
        &[true, false, false, false],
        None,
        carrier,
        MirPreludeAbi::Control,
        "shared guard split",
    )
}

/// Select one of the checked clock helper kernels. The legacy helper spelling
/// is retained only by the old renderer; MIR sees this closed set.
pub(super) fn helper_route(
    helper: THelperKind,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol, arity, mask) = match helper {
        THelperKind::ClockNew => ("clock_new", "jet_std_clock_new", 1, vec![false]),
        THelperKind::ClockSystem => ("clock_system", "jet_std_clock_system", 0, Vec::new()),
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.clock",
        member,
        symbol,
        arity,
        arity,
        &mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "clock helper",
    )
}

/// Route a task-local Cell guard projection. The field/path descriptors stay
/// in the structured MIR operation; this row carries only the selected kernel
/// ABI and never a synthesized member string.
pub(super) fn cell_guard_map_route(
    editable: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let member = if editable { "map_edit" } else { "map_read" };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.cell_guard",
        member,
        "jet_cell_guard_map",
        2,
        2,
        &[true, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "cell guard map",
    )
}

/// Route the two-projection Cell guard split. The two checked field identities
/// are explicit MIR facts, not runtime strings.
pub(super) fn cell_guard_split_route(
    editable: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let member = if editable { "split_edit" } else { "split_read" };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.cell_guard",
        member,
        "jet_cell_guard_split",
        3,
        3,
        &[true, false, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "cell guard split",
    )
}

/// Select the exact Prelude row for one checked closure-taking Core operation.
/// The closure kind is semantic TIR, so adapters never inspect a rendered
/// member or append an implicit site/kind argument.
pub(super) fn core_closure_route(
    kind: MirCoreClosureKind,
    grouped_spawn: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (module, member, symbol, arity, borrow_mask) = match &kind {
        MirCoreClosureKind::Spawn if grouped_spawn => (
            "core.tasks",
            "spawn_grouped",
            "jet_std::JetTaskGroup::spawn_at",
            4,
            vec![false, false, false, false],
        ),
        MirCoreClosureKind::Spawn => (
            "core.tasks",
            "spawn",
            "jet_std::JetTask::spawn_at",
            3,
            vec![false, false, false],
        ),
        MirCoreClosureKind::Realtime => (
            "core.rt",
            "callback",
            "jet_rt_callback",
            3,
            vec![false, false, false],
        ),
        MirCoreClosureKind::Serve => (
            "core.http",
            "serve",
            "jet_http_serve",
            2,
            vec![true, false],
        ),
        MirCoreClosureKind::OnInterrupt => (
            "core.sys",
            "on_interrupt",
            "jet_std_os_on_interrupt",
            1,
            vec![false],
        ),
        MirCoreClosureKind::Guard => (
            "core.mem.scope",
            "guard",
            "jet_scope_guard",
            1,
            vec![false],
        ),
        MirCoreClosureKind::OnCommit => (
            "core.transaction",
            "on_commit",
            "jet_transaction_on_commit",
            2,
            vec![true, false],
        ),
        MirCoreClosureKind::OnRollback => (
            "core.transaction",
            "on_rollback",
            "jet_transaction_on_rollback",
            2,
            vec![true, false],
        ),
        MirCoreClosureKind::ReactiveDerived => (
            "core.reactive",
            "derived",
            "jet_std::JetDerived::new",
            1,
            vec![false],
        ),
        MirCoreClosureKind::ReactiveEffect => (
            "core.reactive",
            "effect",
            "jet_std::jet_reactive_effect",
            1,
            vec![false],
        ),
        MirCoreClosureKind::UiMount => (
            "core.ui",
            "reactive_render",
            "jet_ui_reactive_render",
            1,
            vec![false],
        ),
        // D-UI-PREVIEW1=A: the callback route supplies name + optional
        // viewport before the checked zero-argument UiNode closure.
        MirCoreClosureKind::UiPreview { playground, .. } => (
            "core.ui",
            if *playground { "playground" } else { "preview" },
            if *playground {
                "jet_ui_playground_with_viewport"
            } else {
                "jet_ui_preview_with_viewport"
            },
            3,
            vec![true, false, false],
        ),
        // D-UI-CLOSURE1=A: display, shortcut, accessible label, callback.
        MirCoreClosureKind::UiAction => (
            "core.ui",
            "button",
            "jet_ui_button_on_click",
            4,
            vec![true, false, false, false],
        ),
        // D-UI-DROP1=A: state, IME mode, and the typed drop callback.
        MirCoreClosureKind::UiTextInputOnDrop => (
            "core.ui",
            "text_input",
            "jet_ui_text_input_on_drop",
            3,
            vec![true, false, false],
        ),
    };
    let effect = matches!(&kind, MirCoreClosureKind::Realtime).then_some(Effect::Time);
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        module,
        member,
        symbol,
        arity,
        arity,
        &borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Value,
        "core closure call",
    )
}

/// Route the condition wait bridge. The predicate remains a checked callable
/// value; the wait kernel owns release, park, reacquire, and cancellation.
pub(super) fn shared_guard_wait_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.shared_guard",
        "wait",
        "jet_shared_guard_wait_once",
        3,
        3,
        &[true, true, false],
        None,
        carrier,
        MirPreludeAbi::Control,
        "shared guard wait",
    )
}

/// Select the rich runtime stop kernel for `require`, `require_eq`, or
/// `panic`. `value_arity` is the checked semantic payload count before the
/// adapter appends the source/context arguments required by the kernel ABI.
/// Route the checked binary equality that supplies a `require_eq` condition.
/// The formatter/control row is separate and consumes canonical debug strings.
pub(super) fn require_eq_condition_route(
    left: &Type,
    right: &Type,
) -> Result<TPreludeRoute, LowerError> {
    if left != right {
        return Err(route_error("require_eq operands have different checked types"));
    }
    prelude_route_row(
        MirPreludeFamily::BuiltinMethod,
        "core.compare",
        "eq",
        "jet_eq",
        2,
        2,
        &[true, true],
        None,
        &TFailureCarrier::Infallible,
        MirPreludeAbi::Value,
        "require_eq equality condition",
    )
}

pub(super) fn require_stop_route(
    kind: &str,
    value_arity: usize,
    _result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let (member, symbol, kernel_arity, borrow_mask, effect): (
        &str,
        &str,
        usize,
        &[bool],
        Option<Effect>,
    ) = match kind {
        "require" if value_arity <= 1 => (
            "require",
            "jet_require",
            9,
            &[false, true, true, false, true, true, false, false, true],
            Some(Effect::Panic),
        ),
        "require_eq" if value_arity == 2 => (
            "require_eq",
            "jet_require_eq",
            10,
            &[false, true, true, true, false, true, true, false, false, true],
            Some(Effect::Panic),
        ),
        "panic" if value_arity == 1 => (
            "panic",
            "jet_panic_rich",
            8,
            &[true, false, true, true, false, false, true, true],
            Some(Effect::Panic),
        ),
        _ => {
            return Err(route_error(format!(
                "require stop `{kind}` has unsupported checked value arity {value_arity}",
            )));
        }
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.require",
        member,
        symbol,
        kernel_arity,
        kernel_arity,
        &borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Control,
        "require stop",
    )
}

/// Route an analytically impossible index miss to the shared panic stop.
///
/// The checked index hook supplies the source file and line as static MIR
/// arguments, followed by the fixed diagnostic message `"index miss"`.
pub(super) fn index_miss_route(
    result: &Type,
    _carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let carrier = TFailureCarrier::Diverges {
        value: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.index",
        "index_miss",
        "jet_panic",
        3,
        3,
        &[true, false, true],
        Some(Effect::Panic),
        &carrier,
        MirPreludeAbi::Control,
        "index miss",
    )
}


pub(super) fn lane_index_route(
    lane_ty: &str,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let symbol = format!("jet_math_{lane_ty}_lane");
    prelude_route_row(
        MirPreludeFamily::MathBuiltin,
        "core.math",
        "lane_index",
        &symbol,
        4,
        4,
        &[true, false, true, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "lane index",
    )
}

/// Select the owned or range slicing kernel from the checked base type. The
/// direct ABI is `(base,start,end,file,line)`; range slicing is
/// `(base,range,file,line)`.
pub(super) fn slice_route(
    base: &Type,
    has_range: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    match base {
        Type::Shared(inner)
        | Type::Tagged { inner, .. }
        | Type::InlineRange { base: inner, .. } => slice_route(inner, has_range, result, carrier),
        Type::String if has_range => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.collections",
            "slice_string_range",
            "jet_slice_range",
            4,
            4,
            &[true, true, false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "string range slice",
        ),
        Type::String => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.collections",
            "slice_string",
            "jet_string_slice",
            5,
            5,
            &[true, false, false, false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "string slice",
        ),
        Type::List(_) | Type::FixedList { .. } if has_range => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.collections",
            "slice_list_range",
            "jet_slice_vec_range",
            4,
            4,
            &[true, true, false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "list range slice",
        ),
        Type::List(_) | Type::FixedList { .. } => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.collections",
            "slice_list",
            "jet_slice_vec",
            5,
            5,
            &[true, false, false, false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "list slice",
        ),
        _ => Err(route_error(format!(
            "checked slice base `{}` has no canonical slice route",
            base.name()
        ))),
    }
}

/// A fused columnar field read is identified by the checked owner, field, and
/// column number. Those identities select the route row and are not runtime
/// dispatch data.
pub(super) fn columnar_read_route(
    owner: &str,
    field: &str,
    column: usize,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let member = format!("{owner}::{field}#{column}");
    prelude_route_row(
        MirPreludeFamily::ColumnarAccess,
        "core.columnar",
        &member,
        "jet_columns_gather_cell",
        3,
        3,
        &[true, false, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "columnar read",
    )
}

pub(super) fn math_builtin_route(
    type_name: &str,
    func: &str,
    arity: usize,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    if func == "abs" {
        if arity != 1 {
            return Err(route_error(format!(
                "math builtin `{type_name}::abs` has unsupported arity {arity}"
            )));
        }
        let symbol = match (type_name, result) {
            ("Int", Type::Int) => "jet_std::jet_int_abs",
            ("Float", Type::Float) => "jet_std_math_abs_f64",
            ("F32", Type::Float32) => "jet_std_math_abs_f32",
            ("Complex", Type::Float) => "jet_complex_abs",
            _ => {
                return Err(route_error(format!(
                    "math builtin `{type_name}::abs` has no canonical typed route"
                )));
            }
        };
        return prelude_route_row(
            MirPreludeFamily::MathBuiltin,
            "core.math",
            "abs",
            symbol,
            1,
            1,
            &[false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "math builtin",
        );
    }
    let _ = result;
    let symbol = format!("jet_math_{type_name}_{func}");
    let borrow_mask = vec![false; arity];
    prelude_route_row(
        MirPreludeFamily::MathBuiltin,
        type_name,
        func,
        &symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "math builtin",
    )
}
/// D-SPACE-GEOMETRY1=A: coordinate-space calls use the same MathBuiltin
/// family as vectors, but their direct symbol is explicit so AOT, JIT, and
/// Web never reinterpret a nominal space at runtime.
pub(super) fn geometry_builtin_route(
    type_name: &str,
    func: &str,
    arity: usize,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let symbol = format!("jet_math_{type_name}_{func}");
    prelude_route_row(
        MirPreludeFamily::MathBuiltin,
        "core.math",
        &format!("geometry.{type_name}.{func}"),
        &symbol,
        arity,
        arity,
        &vec![false; arity],
        None,
        carrier,
        MirPreludeAbi::Value,
        "coordinate-space builtin",
    )
}

fn geometry_method_arity(method: &str) -> Option<usize> {
    match method {
        "add" | "sub" | "then" | "ray" => Some(2),
        "inverse" => Some(1),
        "point" => Some(2),
        "point_at_depth" => Some(3),
        _ => None,
    }
}

fn geometry_method_route(
    type_name: &str,
    method: &str,
    _receiver: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let arity = geometry_method_arity(method).ok_or_else(|| {
        route_error(format!(
            "coordinate-space method `{type_name}.{method}` has no registered route"
        ))
    })?;
    let symbol = format!("jet_math_{type_name}_{method}");
    let borrow_mask = std::iter::once(true)
        .chain(std::iter::repeat_n(false, arity.saturating_sub(1)))
        .collect::<Vec<_>>();
    Ok(TRoutePlan::Prelude(prelude_route_row(
        MirPreludeFamily::MathBuiltin,
        "core.math",
        &format!("geometry.{type_name}.{method}"),
        &symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "coordinate-space method",
    )?))
}

pub(super) fn precise_builtin_route(
    type_name: &str,
    func: &str,
    arity: usize,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    // The shared Prelude spells every precise-numeric entry point as
    // `jet_<type>_<func>` (`Prelude/CoreLib/Top/MathRandomTime.rs`:
    // `jet_decimal_from_str(&String)`, `jet_decimal_add(&a, &b)`,
    // `jet_fraction_from_parts(i64, i64)`, `jet_fraction_to_string(&a)`);
    // the resident host registers the same spelling. Constructors take
    // their parts by value, `from_str` and every method borrow.
    let symbol = format!("jet_{}_{func}", type_name.to_ascii_lowercase());
    let by_value = matches!(func, "new" | "from_parts")
        || (func.starts_with("from_") && func != "from_str");
    let borrow_mask = vec![!by_value; arity];
    prelude_route_row(
        MirPreludeFamily::PreciseBuiltin,
        type_name,
        func,
        &symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "precise builtin",
    )
}

pub(super) fn print_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.term",
        "print",
        "jet_term_write_stdout_line",
        2,
        2,
        &[true, false],
        Some(Effect::IO),
        carrier,
        MirPreludeAbi::Effect,
        "print",
    )
}

pub(super) fn ambient_input_route(
    has_prompt: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = (has_prompt, result);
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.term",
        "input",
        "jet_std_io_input",
        1,
        1,
        &[true],
        Some(Effect::IO),
        carrier,
        MirPreludeAbi::Effect,
        "ambient input",
    )
}
/// Convert one checked runtime entry vector into the canonical typed map.
pub(super) fn data_entries_to_map_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.collections",
        "entries_to_map",
        "jet_data_entries_to_map",
        1,
        1,
        &[false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "map entries conversion",
    )
}


pub(super) fn todo_route(
    result: &Type,
    _carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let carrier = TFailureCarrier::Diverges {
        value: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
    };
    runtime_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.runtime",
        "todo",
        "jet_todo_stop",
        3,
        3,
        &[true, false, true],
        Some(Effect::IO),
        &carrier,
        MirPreludeAbi::Value,
        "todo",
    )
}


pub(super) fn string_format_route(
    format: &crate::AST::StrFormat,
    value_ty: &Type,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let integer = matches!(value_ty, Type::Int | Type::IntN { .. });
    let (member, symbol, arity, borrow_mask) = match format {
        crate::AST::StrFormat::Display => ("display", "jet_fmt_display", 1, vec![true]),
        crate::AST::StrFormat::Debug => ("debug", "jet_fmt_debug", 1, vec![true]),
        crate::AST::StrFormat::Pretty => ("pretty", "jet_fmt_pretty", 1, vec![true]),
        crate::AST::StrFormat::Unit(_) => {
            return Err(route_error(
                "unit formatting is lowered through its checked String function",
            ));
        }
        crate::AST::StrFormat::Fixed(_) => (
            "fixed",
            if integer {
                "jet_fmt_decimal_int"
            } else {
                "jet_fmt_decimal"
            },
            2,
            vec![true, false],
        ),
        crate::AST::StrFormat::Grouped(_) => (
            "grouped",
            if integer {
                "jet_fmt_grouped_int"
            } else {
                "jet_fmt_grouped"
            },
            2,
            vec![true, false],
        ),
        crate::AST::StrFormat::Hex(_) => (
            "hex",
            if integer {
                "jet_fmt_hex"
            } else {
                "jet_fmt_hex_decimal"
            },
            2,
            vec![true, false],
        ),
        crate::AST::StrFormat::Pad { .. } => {
            ("pad", "jet_fmt_pad", 3, vec![true, false, true])
        }
        crate::AST::StrFormat::PadLeft { .. } => {
            ("pad_left", "jet_fmt_pad_left", 3, vec![true, false, true])
        }
        crate::AST::StrFormat::Sci(_) => ("sci", "jet_fmt_sci", 2, vec![true, false]),
        crate::AST::StrFormat::Percent(_) => {
            ("percent", "jet_fmt_percent", 2, vec![true, false])
        }
        crate::AST::StrFormat::Bin => (
            "bin",
            if integer {
                "jet_fmt_bin"
            } else {
                "jet_fmt_bin_decimal"
            },
            1,
            vec![true],
        ),
        crate::AST::StrFormat::Oct => (
            "oct",
            if integer {
                "jet_fmt_oct"
            } else {
                "jet_fmt_oct_decimal"
            },
            1,
            vec![true],
        ),
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.text.fmt",
        member,
        symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "string interpolation format",
    )
}


pub(super) fn static_prelude_route(
    module: &str,
    member: &str,
    borrow_mask: &[bool],
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let effect = (module == "::JetAtomic" && member == "try_new").then_some(Effect::Mem);
    let arity = borrow_mask.len();
    let symbol = if let Some(rooted_module) = module.strip_prefix("::") {
        format!("{rooted_module}::{member}")
    } else {
        format!("{module}::{member}")
    };
    let plan = if module.starts_with("::") {
        prelude(
            MirPreludeFamily::StaticPrelude,
            module,
            member,
            &symbol,
            arity,
            arity,
            borrow_mask,
            effect,
            carrier,
            MirPreludeAbi::Value,
        )
    } else {
        runtime(
            MirPreludeFamily::StaticPrelude,
            module,
            member,
            &symbol,
            arity,
            arity,
            borrow_mask,
            effect,
            carrier,
            MirPreludeAbi::Value,
        )
    };
    as_prelude(plan, "static Prelude call")
}

pub(super) fn decode_under_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.encoding",
        "decode_under",
        "jet_std::FieldError::under",
        2,
        2,
        &[true, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "decode error context",
    )
}

pub(super) fn compare_chain_route(
    op: BinOp,
    hook: bool,
    left_ty: &Type,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TRoutePlan, LowerError> {
    let _ = left_ty;
    compare_route(op, hook, result, carrier)
}

pub(super) fn overflow_opt_route(
    prefix: &str,
    op: &str,
    line: u32,
    input: &Type,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = line;
    let Some(width) = fixed_width_name(input) else {
        return Err(route_error(
            "overflow option has no resolved fixed numeric receiver type",
        ));
    };
    let checked = prefix == "checked";
    if checked {
        let Type::Option(value) = result else {
            return Err(route_error(
                "checked overflow option has no Option result type",
            ));
        };
        if value.as_ref() != input {
            return Err(route_error(
                "checked overflow option has a mismatched payload type",
            ));
        }
        if !matches!(carrier, TFailureCarrier::Optional { .. }) {
            return Err(route_error(
                "checked overflow option has a mismatched failure carrier",
            ));
        }
    } else if result != input {
        return Err(route_error(
            "overflow option has a mismatched resolved result type",
        ));
    } else if matches!(carrier, TFailureCarrier::Optional { .. }) {
        return Err(route_error(
            "value-producing overflow option has an optional failure carrier",
        ));
    }
    let (mode, op_name, arity, effect) = match (prefix, op) {
        ("checked", "add" | "sub" | "mul" | "div" | "rem" | "pow") => {
            ("checked", op, 2, None)
        }
        ("checked_policy", "add" | "sub" | "mul" | "div" | "rem" | "pow") => {
            ("trap", op, 4, Some(Effect::Panic))
        }
        ("wrapping", "add" | "sub" | "mul") => ("wrapping", op, 2, None),
        ("wrapping", "div" | "pow") => ("wrapping", op, 4, Some(Effect::Panic)),
        ("saturating", "add" | "sub" | "mul") => ("saturating", op, 2, None),
        ("saturating", "div" | "pow") => ("saturating", op, 4, Some(Effect::Panic)),
        ("rotate_left" | "rotate_right", "rotate") => {
            (prefix, "rotate", 4, Some(Effect::Panic))
        }
        _ => {
            return Err(route_error(format!(
                "unsupported fixed-width overflow operation `{prefix}_{op}`",
            )));
        }
    };
    let symbol = if matches!(prefix, "rotate_left" | "rotate_right") {
        format!("jet_{width}_{prefix}")
    } else {
        format!("jet_{width}_{mode}_{op_name}")
    };
    let member = format!("{width}.{prefix}.{op}");
    let borrow_mask = vec![false; arity];
    prelude_route_row(
        MirPreludeFamily::Overflow,
        "core.numeric",
        &member,
        &symbol,
        arity,
        arity,
        &borrow_mask,
        effect,
        carrier,
        MirPreludeAbi::Value,
        "fixed-width overflow option",
    )
}

pub(super) fn try_conversion_route(
    convert: &TTryConvert,
    source: &Type,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = (source, result, carrier);
    match convert {
        TTryConvert::DefaultErr => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.errors",
            "from_message",
            "jet_err_from_message",
            1,
            1,
            &[false],
            None,
            &TFailureCarrier::Infallible,
            MirPreludeAbi::Value,
            "default error conversion",
        ),
        TTryConvert::Typed { .. } => Err(route_error(
            "typed error conversion requires checked MIR function lowering",
        )),
        TTryConvert::WidenUnion { .. } => Err(route_error(
            "union error conversion requires checked MIR enum lowering",
        )),
        TTryConvert::ProtocolExit => {
            let carrier = TFailureCarrier::Diverges {
                value: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
            };
            prelude_route_row(
                MirPreludeFamily::StaticPrelude,
                "core.errors",
                "entry_error_exit",
                "jet_entry_error_exit_jet",
                1,
                1,
                &[false],
                Some(Effect::Panic),
                &carrier,
                MirPreludeAbi::Control,
                "protocol error exit",
            )
        }
        TTryConvert::None | TTryConvert::Never => Err(route_error(
            "bare try propagation has no conversion Prelude route",
        )),
    }
}

pub(super) fn journey_reset_route() -> Result<TPreludeRoute, LowerError> {
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.errors",
        "journey_reset",
        "jet_journey_reset",
        0,
        0,
        &[],
        None,
        &TFailureCarrier::Infallible,
        MirPreludeAbi::Effect,
        "successful try journey reset",
    )
}

pub(super) fn failure_context_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = (result, carrier);
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.errors",
        "err_with_context_frame",
        "jet_err_with_context_frame",
        5,
        5,
        &[false, true, false, true, false],
        None,
        &TFailureCarrier::Infallible,
        MirPreludeAbi::Value,
        "default error context frame",
    )
}

pub(super) fn failure_note_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = (result, carrier);
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.errors",
        "journey_frame_text",
        "jet_journey_frame_text",
        4,
        4,
        &[true, false, true, true],
        None,
        &TFailureCarrier::Infallible,
        MirPreludeAbi::Effect,
        "failure journey frame",
    )
}


pub(super) fn range_checked_ctor_route(
    name: &str,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = (name, result);
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.numeric",
        "inline_range",
        "jet_inline_range_from_int",
        3,
        3,
        &[false, false, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "range checked constructor",
    )
}

pub(super) fn distinct_conversion_route(
    name: &str,
    source: &Type,
    op: &TNumericOp,
    range: Option<(i64, i64)>,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = (name, range, result);
    match op {
        TNumericOp::CheckedIntToFloat { .. } if matches!(source, Type::Int) => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.numeric",
            "checked_widen",
            "jet_std::jet_int_checked_widen",
            4,
            4,
            &[false, false, true, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "integer widening",
        ),
        TNumericOp::CheckedIntToFloat { .. } => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.numeric",
            "checked_widen",
            "jet_numeric_checked_widen_at",
            5,
            5,
            &[false, false, false, true, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "numeric widening",
        ),
        TNumericOp::TryFrom { .. } if matches!(source, Type::Int) => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.numeric",
            "int_try_from",
            "jet_std::jet_int_try_from_checked",
            2,
            2,
            &[false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "integer narrowing conversion",
        ),
        TNumericOp::TryFrom { .. } => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.numeric",
            "fixed_try_from",
            "jet_numeric_try_from_fixed",
            3,
            3,
            &[false, false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "fixed integer narrowing conversion",
        ),
        TNumericOp::InlineRange { .. } => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.numeric",
            "inline_range",
            "jet_inline_range_from_int",
            3,
            3,
            &[false, false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "inline range conversion",
        ),
        TNumericOp::CheckedIntToFixed { .. } => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.numeric",
            "int_checked_fixed",
            "jet_std::jet_int_checked_fixed",
            4,
            4,
            &[false, false, true, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "checked fixed integer construction",
        ),
        TNumericOp::FloatToInt { .. } => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.numeric",
            "float_to_int",
            "jet_numeric_float_to_int",
            2,
            2,
            &[false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "float to integer conversion",
        ),
        TNumericOp::FloatNarrow { .. } => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.numeric",
            "float_narrow",
            "jet_numeric_float_narrow",
            1,
            1,
            &[false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "float narrowing conversion",
        ),
        TNumericOp::CastAs { .. }
        | TNumericOp::Predicate(_)
        | TNumericOp::BitCount { .. }
        | TNumericOp::ToShow
        | TNumericOp::EuclideanDiv { .. }
        | TNumericOp::EuclideanRem { .. } => Err(route_error(
            "numeric operation is not a checked distinct conversion",
        )),
    }
}

pub(super) fn unit_conversion_route(
    destination: &str,
    scale: &crate::AST::UnitRatio,
    offset: &crate::AST::UnitRatio,
    rounding: Option<UnitRoundingMode>,
    relative_uncertainty: Option<f64>,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = (destination, scale, offset, relative_uncertainty, result);
    match rounding {
        None => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.units",
            "conversion_exact",
            "jet_unit_conversion_exact",
            5,
            5,
            &[false, true, true, true, true],
            None,
            carrier,
            MirPreludeAbi::Value,
            "exact unit conversion",
        ),
        Some(_) => prelude_route_row(
            MirPreludeFamily::StaticPrelude,
            "core.units",
            "conversion_rounded",
            "jet_unit_conversion_rounded",
            7,
            7,
            &[false, true, true, true, true, false, false],
            None,
            carrier,
            MirPreludeAbi::Value,
            "rounded unit conversion",
        ),
    }
}

/// Read one checked carrier fact through the shared Prelude projector. The
/// field identity remains a MIR field ID; this row chooses only the kernel.
pub(super) fn carrier_fact_route(
    notes: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol) = if notes {
        ("notes", "jet_notes")
    } else {
        ("partial", "jet_partial")
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.outcome",
        member,
        symbol,
        1,
        1,
        &[true],
        None,
        carrier,
        MirPreludeAbi::Value,
        "carrier fact",
    )
}

/// Read the value protected by a checked GC root.
pub(super) fn gc_read_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.gc",
        "read",
        "jet_gc_read",
        1,
        1,
        &[true],
        Some(Effect::Mem),
        carrier,
        MirPreludeAbi::Value,
        "GC read",
    )
}

/// Select the one exact GC edit wrapper for a checked edit shape. Edge IDs are
/// passed as one aggregate operand, rather than as a variable-arity name list.
pub(super) fn gc_edit_route(
    kind: TGcEditKind,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol, arity, borrow_mask) = match kind {
        TGcEditKind::Clear => ("edit_clear", "jet_gc_edit_clear", 2, vec![true, false]),
        TGcEditKind::Pop => ("edit_pop", "jet_gc_edit_pop", 2, vec![true, false]),
        TGcEditKind::RemoveIndex => (
            "edit_remove_index",
            "jet_gc_edit_remove_index",
            3,
            vec![true, false, false],
        ),
        TGcEditKind::InsertIndex => (
            "edit_insert_index",
            "jet_gc_edit_insert_index",
            4,
            vec![true, false, true, false],
        ),
        TGcEditKind::Prepend => (
            "edit_prepend",
            "jet_gc_edit_prepend",
            3,
            vec![true, true, false],
        ),
        TGcEditKind::Additive => (
            "edit_additive",
            "jet_gc_edit_additive",
            3,
            vec![true, true, false],
        ),
        TGcEditKind::Plain => ("edit_plain", "jet_gc_edit_plain", 2, vec![true, false]),
        TGcEditKind::EdgeSlot => (
            "edit_edge_slot",
            "jet_gc_edit_edge_slot",
            4,
            vec![true, true, false, false],
        ),
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.gc",
        member,
        symbol,
        arity,
        arity,
        &borrow_mask,
        Some(Effect::Mem),
        carrier,
        MirPreludeAbi::Value,
        "GC edit",
    )
}

/// Route a checked typed-text projection to its exact Prelude kernel.
pub(super) fn typed_text_route(
    kind: TTypedTextForm,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol, borrow_mask) = match kind {
        TTypedTextForm::SQLRaw => ("sql_raw", "jet_typed_sql_raw", vec![false]),
        TTypedTextForm::HTMLRaw => ("html_raw", "jet_typed_html_raw", vec![false]),
        TTypedTextForm::ShRaw => ("sh_raw", "jet_typed_sh_raw", vec![false]),
        TTypedTextForm::SQLTemplate => ("sql_template", "jet_typed_sql_template", vec![true]),
        TTypedTextForm::SQLParams => ("sql_params", "jet_typed_sql_params", vec![true]),
        TTypedTextForm::HTMLText => ("html_text", "jet_typed_html_text", vec![false]),
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.typed_text",
        member,
        symbol,
        1,
        1,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "typed text projection",
    )
}

/// Route a checked typed-head interpolation. Literal and hole/trust metadata
/// are carried in the structured MIR operation, so this row has a fixed ABI.
pub(super) fn typed_text_interp_route(
    kind: TTypedTextInterpKind,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol, arity, borrow_mask) = match kind {
        TTypedTextInterpKind::SQL => (
            "sql_interpolate",
            "jet_typed_sql_interpolate",
            2,
            vec![true, false],
        ),
        TTypedTextInterpKind::HTML => (
            "html_interpolate",
            "jet_typed_html_interpolate",
            3,
            vec![true, false, true],
        ),
        TTypedTextInterpKind::Sh => (
            "sh_interpolate",
            "jet_typed_sh_interpolate",
            2,
            vec![true, false],
        ),
        TTypedTextInterpKind::URL => (
            "url_literal",
            "jet_std::jet_typed_url_literal",
            2,
            vec![true, false],
        ),
        TTypedTextInterpKind::Path => (
            "path_literal",
            "jet_typed_path_literal",
            2,
            vec![true, false],
        ),
        TTypedTextInterpKind::DateTime => (
            "datetime_literal",
            "jet_typed_datetime_literal",
            2,
            vec![true, false],
        ),
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.typed_text",
        member,
        symbol,
        arity,
        arity,
        &borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "typed text interpolation",
    )
}

/// Snapshot comparison is a single harness kernel. The path is a data operand,
/// never part of a synthesized symbol or member string.
pub(super) fn expect_snapshot_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.test",
        "expect_snapshot",
        "jet_expect_snapshot",
        2,
        2,
        &[false, false],
        Some(Effect::FS),
        carrier,
        MirPreludeAbi::Value,
        "expect snapshot",
    )
}

/// Select the generic expiring-value constructor. `secret` chooses the
/// capability-bearing wrapper while the element type remains a MIR type arg.
pub(super) fn expiring_route(
    secret: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol) = if secret {
        ("secret_new", "jet_expiring_secret_new")
    } else {
        ("new", "jet_expiring_new")
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.expiring",
        member,
        symbol,
        3,
        3,
        &[false, true, true],
        Some(Effect::Time),
        carrier,
        MirPreludeAbi::Value,
        "expiring constructor",
    )
}

/// C callback construction has one runtime operand: the lowered closure.
/// Extern symbol and return type stay as structured metadata on the MIR op.
pub(super) fn c_callback_route(
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.ffi",
        "callback_boundary",
        "jet_ffi_callback_boundary",
        1,
        1,
        &[false],
        Some(Effect::FFI),
        carrier,
        MirPreludeAbi::Value,
        "C callback boundary",
    )
}
/// Route the structured text/binary pattern matcher. Pattern parts remain
/// checked metadata on the MIR operation; this row only fixes the subject ABI.
pub(super) fn pattern_match_route(
    binary: bool,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let (member, symbol) = if binary {
        ("binary_match", "jet_binary_pattern_match")
    } else {
        ("text_match", "jet_text_pattern_match")
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.pattern",
        member,
        symbol,
        2,
        2,
        &[true, false],
        None,
        carrier,
        MirPreludeAbi::Value,
        "pattern match",
    )
}
/// Route the closed set of runtime-backed host methods to exact Prelude wrappers.
///
/// The receiver is always argument zero. Transactional Shared methods add the
/// explicit STM handle as argument one; the lowerer supplies that value from the
/// checked transaction local before the user closure. No receiver/type strings
/// cross the MIR boundary.
pub(super) fn host_method_route(
    recv: &Type,
    method: &str,
    arg_count: usize,
    result: &Type,
    carrier: &TFailureCarrier,
) -> Result<TPreludeRoute, LowerError> {
    let _ = result;
    let mut shape = recv;
    while let Type::Tagged { inner, .. } = shape {
        shape = inner;
    }
    let (member, symbol, arity, borrow_mask): (&str, &str, usize, &[bool]) = match shape {
        Type::Apply { name, .. } if name == "Pool" => match method {
            "add" => ("Pool.add", "jet_std::JetPool::add", 2, &[true, false]),
            "remove" => ("Pool.remove", "jet_std::JetPool::remove", 2, &[true, false]),
            "ids" => ("Pool.ids", "jet_std::JetPool::ids", 1, &[true]),
            _ => {
                return Err(route_error(format!(
                    "unknown checked Pool host method `{method}`"
                )))
            }
        },
        Type::Shared(_) => match (method, arg_count) {
            ("get", 0) => ("Shared.get", "jet_shared_get", 1, &[true]),
            ("set", 1) => ("Shared.set", "jet_shared_set", 2, &[true, false]),
            ("replace", 1) => ("Shared.replace", "jet_shared_replace", 2, &[true, false]),
            ("read", 1) => ("Shared.read", "jet_shared_read", 2, &[true, false]),
            ("edit", 1) => ("Shared.edit", "jet_shared_edit", 2, &[true, false]),
            ("capture", 0) => ("Shared.capture", "jet_shared_capture", 1, &[true]),
            ("capture", 1) => (
                "Shared.capture",
                "jet_shared_capture_with",
                2,
                &[true, false],
            ),
            ("capture_txn", 0) => (
                "Shared.capture_txn",
                "jet_shared_capture_txn_plain",
                2,
                &[true, true],
            ),
            ("capture_txn", 1) => (
                "Shared.capture_txn",
                "jet_shared_capture_txn",
                3,
                &[true, true, false],
            ),
            ("try_replace", 2) => (
                "Shared.try_replace",
                "jet_shared_try_replace",
                3,
                &[true, false, false],
            ),
            ("read_txn", 1) => (
                "Shared.read_txn",
                "jet_shared_read_txn",
                3,
                &[true, true, false],
            ),
            ("edit_txn", 1) => (
                "Shared.edit_txn",
                "jet_shared_edit_txn",
                3,
                &[true, true, false],
            ),
            ("guard_read", 0) => ("Shared.guard_read", "jet_shared_guard_read", 1, &[true]),
            ("guard_edit", 0) => ("Shared.guard_edit", "jet_shared_guard_edit", 1, &[true]),
            ("downgrade", 0) => ("Shared.downgrade", "jet_shared_downgrade", 1, &[true]),
            ("strong_count", 0) => ("Shared.strong_count", "jet_shared_strong_count", 1, &[true]),
            _ => {
                return Err(route_error(format!(
                    "unknown checked Shared host method `{method}`"
                )))
            }
        },
        Type::Apply { name, .. } if name == crate::Syntax::TYPE_SHARED_SNAPSHOT => match method {
            "value" if arg_count == 0 => (
                "SharedSnapshot.value",
                "jet_shared_snapshot_value",
                1,
                &[true],
            ),
            _ => {
                return Err(route_error(format!(
                    "unknown checked SharedSnapshot host method `{method}`"
                )))
            }
        },
        Type::Apply { name, .. } if name == crate::Syntax::TYPE_SHARED_WEAK => {
            match method {
                "upgrade" => (
                    "Shared.Weak.upgrade",
                    "jet_shared_weak_upgrade",
                    1,
                    &[true],
                ),
                _ => {
                    return Err(route_error(format!(
                        "unknown checked Shared.Weak host method `{method}`"
                    )))
                }
            }
        }
        Type::Apply { name, .. } if name == "Cell" => match method {
            "get" => ("Cell.get", "jet_cell_get", 1, &[true]),
            "guard_read" => ("Cell.guard_read", "jet_cell_guard_read", 1, &[true]),
            "guard_edit" => ("Cell.guard_edit", "jet_cell_guard_edit", 1, &[true]),
            "set" => ("Cell.set", "jet_cell_set", 2, &[true, false]),
            "replace" => ("Cell.replace", "jet_cell_replace", 2, &[true, false]),
            "read" => ("Cell.read", "jet_cell_read", 2, &[true, false]),
            "edit" => ("Cell.edit", "jet_cell_edit", 2, &[true, false]),
            "get_or_set" => ("Cell.get_or_set", "jet_cell_get_or_set", 2, &[true, false]),
            _ => {
                return Err(route_error(format!(
                    "unknown checked Cell host method `{method}`"
                )))
            }
        },
        Type::Apply { name, .. } if name == "CellReadGuard" => match method {
            "get" => (
                "CellReadGuard.get",
                "jet_cell_read_guard_get",
                1,
                &[true],
            ),
            "read" => (
                "CellReadGuard.read",
                "jet_cell_read_guard_read",
                2,
                &[true, false],
            ),
            _ => {
                return Err(route_error(format!(
                    "unknown checked CellReadGuard host method `{method}`"
                )))
            }
        },
        Type::Apply { name, .. } if name == "CellEditGuard" => match method {
            "get" => (
                "CellEditGuard.get",
                "jet_cell_edit_guard_get",
                1,
                &[true],
            ),
            "set" => (
                "CellEditGuard.set",
                "jet_cell_edit_guard_set",
                2,
                &[true, false],
            ),
            "read" => (
                "CellEditGuard.read",
                "jet_cell_edit_guard_read",
                2,
                &[true, false],
            ),
            "edit" => (
                "CellEditGuard.edit",
                "jet_cell_edit_guard_edit",
                2,
                &[true, false],
            ),
            _ => {
                return Err(route_error(format!(
                    "unknown checked CellEditGuard host method `{method}`"
                )))
            }
        },
        Type::Apply { name, .. } if name == "ExpiringSecret" => match method {
            "with" => (
                "ExpiringSecret.with",
                "jet_expiring_secret_with",
                2,
                &[true, false],
            ),
            _ => {
                return Err(route_error(format!(
                    "unknown checked ExpiringSecret host method `{method}`"
                )))
            }
        },
        _ => {
            return Err(route_error(format!(
                "unknown checked host method `{method}` on `{}`",
                recv.name()
            )))
        }
    };
    prelude_route_row(
        MirPreludeFamily::StaticPrelude,
        "core.host",
        member,
        symbol,
        arity,
        arity,
        borrow_mask,
        None,
        carrier,
        MirPreludeAbi::Value,
        "runtime host method",
    )
}
