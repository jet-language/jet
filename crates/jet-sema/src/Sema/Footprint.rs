//! D-METHOD-FOOTPRINT1=A: method field footprints.
//!
//! A method's footprint is the set of record fields its body reads or changes
//! through `self`. Calls to sibling methods on `self` add their footprints; any
//! other use of the whole `self` makes the footprint the whole record. The fact
//! is inferred from every method body of an owner, solved to a fixpoint over
//! sibling calls, and stored on the receiver parameter. An expert may state it
//! on the receiver (`&self.log`, `&self.[log, hits]`); the stated set must name
//! stored fields and cover the body (E0231). Callers and the API snapshot see
//! the stated set when present.

use std::collections::{BTreeMap, HashMap};

use super::TypeRegistry;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{AccessConvention, Expr, Func, Item, LValue, WalkNode};

/// What one method body does with `self`, before sibling calls are solved.
#[derive(Default)]
struct BodyUses {
    /// Stored fields reached through `self`, with their first use.
    fields: BTreeMap<String, Span>,
    /// Sibling methods called on `self`, with the call's method name span.
    calls: Vec<(String, Span)>,
    /// The first use of the whole `self`.
    whole: Option<Span>,
}

/// A solved footprint: each field with the span that first reaches it, or
/// the whole record with the span that first uses it.
#[derive(Clone, PartialEq)]
enum Footprint {
    Fields(BTreeMap<String, Span>),
    Whole(Span),
}

/// `self`, or a marked `&self` / `^self` place.
fn is_self(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name, _) => name == Syntax::KW_SELF,
        Expr::Place(inner, _, _) | Expr::Paren(inner, _) => is_self(inner),
        _ => false,
    }
}

/// Replace a visited `self` base so the walk does not count it again.
fn settle(base: &mut Expr) {
    let span = base.span();
    *base = Expr::Ident(String::new(), span);
}

fn body_uses(function: &Func, fields: &[String]) -> BodyUses {
    let mut uses = BodyUses::default();
    let field = |uses: &mut BodyUses, name: &str, span: Span| {
        if fields.iter().any(|known| known == name) {
            uses.fields.entry(name.to_string()).or_insert(span);
        } else {
            // A method value or computed field reads the whole record.
            uses.whole.get_or_insert(span);
        }
    };
    for stmt in &function.body {
        let mut stmt = stmt.clone();
        stmt.for_each_node_mut(|node| match node {
            WalkNode::LValue(LValue::Local { name, name_span }) if name.as_str() == Syntax::KW_SELF => {
                uses.whole.get_or_insert(*name_span);
            }
            WalkNode::LValue(LValue::Field { base, field: name, span }) if is_self(base) => {
                field(&mut uses, name, *span);
                settle(base);
            }
            WalkNode::Expr(Expr::Field(base, name, span)) if is_self(base) => {
                field(&mut uses, name, *span);
                settle(base);
            }
            WalkNode::Expr(Expr::MemberSpread { base, members, .. }) if is_self(base) => {
                for (name, span) in members.iter() {
                    field(&mut uses, name, *span);
                }
                settle(base);
            }
            WalkNode::Expr(Expr::MethodCall {
                receiver,
                method,
                method_span,
                ..
            }) if is_self(receiver) => {
                if fields.iter().any(|known| known == method) {
                    // A stored function value called through its field.
                    field(&mut uses, method, *method_span);
                } else {
                    uses.calls.push((method.clone(), *method_span));
                }
                settle(receiver);
            }
            WalkNode::Expr(Expr::Ident(name, span)) if name.as_str() == Syntax::KW_SELF => {
                uses.whole.get_or_insert(*span);
            }
            _ => {}
        });
    }
    uses
}

/// The receiver of a method that can have a footprint: `self` or `&self`.
fn footprint_receiver(function: &Func) -> Option<AccessConvention> {
    let receiver = function.self_param()?;
    (receiver.convention != AccessConvention::Move).then_some(receiver.convention)
}

/// Solve every method of one owner. `methods` holds each method once; a name
/// declared twice (an inherent method and a trait method) resolves to the
/// whole record at its call sites.
fn solve_owner(
    owner: &str,
    methods: &mut [&mut Func],
    fields: &[String],
    diags: &mut Vec<Diagnostic>,
) {
    let mut index: HashMap<&str, Option<usize>> = HashMap::new();
    for (position, method) in methods.iter().enumerate() {
        index
            .entry(method.name.as_str())
            .and_modify(|slot| *slot = None)
            .or_insert(footprint_receiver(method).map(|_| position));
    }
    let uses: Vec<Option<BodyUses>> = methods
        .iter()
        .map(|method| footprint_receiver(method).map(|_| body_uses(method, fields)))
        .collect();
    let mut solved: Vec<Option<Footprint>> = uses
        .iter()
        .map(|uses| {
            uses.as_ref().map(|uses| match uses.whole {
                Some(span) => Footprint::Whole(span),
                None => Footprint::Fields(uses.fields.clone()),
            })
        })
        .collect();
    loop {
        let mut changed = false;
        for (position, uses) in uses.iter().enumerate() {
            let Some(uses) = uses else { continue };
            for (callee, span) in &uses.calls {
                let next = match index.get(callee.as_str()).copied().flatten() {
                    Some(target) => match (&solved[position], &solved[target]) {
                        (Some(Footprint::Fields(own)), Some(Footprint::Fields(called))) => {
                            let mut merged = own.clone();
                            for name in called.keys() {
                                merged.entry(name.clone()).or_insert(*span);
                            }
                            Footprint::Fields(merged)
                        }
                        (Some(Footprint::Fields(_)), _) => Footprint::Whole(*span),
                        _ => continue,
                    },
                    // A consuming, static, unknown, or ambiguous callee takes
                    // the whole receiver.
                    None => match &solved[position] {
                        Some(Footprint::Fields(_)) => Footprint::Whole(*span),
                        _ => continue,
                    },
                };
                if solved[position].as_ref() != Some(&next) {
                    solved[position] = Some(next);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for (method, footprint) in methods.iter_mut().zip(solved) {
        let Some(footprint) = footprint else { continue };
        let Some(receiver) = method.params.first_mut() else { continue };
        receiver.footprint.inferred = match &footprint {
            Footprint::Fields(fields) => Some(fields.keys().cloned().collect()),
            Footprint::Whole(_) => None,
        };
        let Some(declared) = receiver.footprint.declared.clone() else { continue };
        let names: Vec<&str> = declared.iter().map(|(name, _)| name.as_str()).collect();
        let stated = match names.as_slice() {
            [name] => format!("self.{name}"),
            names => format!("self.[{}]", names.join(", ")),
        };
        let mut named_fields = true;
        for (name, span) in &declared {
            if !fields.iter().any(|known| known == name) {
                named_fields = false;
                diags.push(Diagnostic::error(
                    "E0231",
                    format!("`self.{name}` in the receiver of `{}` names no field of `{owner}`", method.name),
                    "a stated footprint lists the stored fields the method may read or change".to_string(),
                    format!("name fields of `{owner}` only, or remove `.{name}` from the receiver"),
                    Some(*span),
                ));
            }
        }
        if !named_fields {
            continue;
        }
        let outside = match &footprint {
            Footprint::Whole(span) => Some(("all of `self`".to_string(), *span)),
            Footprint::Fields(used) => used
                .iter()
                .find(|(name, _)| !declared.iter().any(|(stated, _)| stated == *name))
                .map(|(name, span)| (format!("`self.{name}`"), *span)),
        };
        if let Some((what, span)) = outside {
            diags.push(Diagnostic::error(
                "E0231",
                format!("`{}` uses {what} outside its stated footprint `{stated}`", method.name),
                "callers and the published API see the stated footprint, so the body must stay inside it"
                    .to_string(),
                format!("add the field to the receiver's footprint, or change `{}` to stay inside `{stated}`", method.name),
                Some(span),
            ));
        }
    }
}

/// Infer the footprint of every method declared in `items`, grouped by owner,
/// and check every stated footprint against its body. An inline code module
/// is its own owner scope.
pub(crate) fn infer_module_footprints(items: &mut [Item], registry: &TypeRegistry) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for item in items.iter_mut() {
        if let Item::CodeModule(module) = item {
            if let Some(body) = &mut module.body {
                diags.extend(infer_module_footprints(body, registry));
            }
        }
    }
    let mut owners: BTreeMap<String, Vec<&mut Func>> = BTreeMap::new();
    for item in items.iter_mut() {
        match item {
            Item::Struct(definition) => {
                let methods = owners.entry(definition.name.clone()).or_default();
                methods.extend(definition.methods.iter_mut());
                for block in &mut definition.trait_impls {
                    methods.extend(block.methods.iter_mut());
                }
            }
            Item::Impl(implementation) => {
                owners
                    .entry(implementation.type_name.clone())
                    .or_default()
                    .extend(implementation.methods.iter_mut());
            }
            _ => {}
        }
    }
    for (owner, mut methods) in owners {
        let Some(fields) = registry.struct_fields(&owner) else { continue };
        let fields: Vec<String> = fields.iter().map(|(name, _, _)| name.clone()).collect();
        solve_owner(&owner, &mut methods, &fields, &mut diags);
    }
    diags
}
