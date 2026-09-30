//! D-CAP-RECEIVER1=D: mark the value, not the call.
//!
//! A `&` or `^` mark sits on the named place a method call writes or takes:
//! `&buf.append(" world")`, `^buf.seal()`, `&items[i].append("!")`. The parser
//! binds the mark to the maximal place (a name plus fields, indexes, or
//! ranges) that the first call of a chain receives. Sema owns the check: the
//! MethodCall arm lifts the mark off the receiver before resolution, every
//! receiver-convention hook records the resolved convention for that
//! receiver, and the arm then compares the written mark with that
//! convention. Resolution, place evaluation, argument order, two-phase
//! borrowing, and move checks see exactly the unmarked call they always saw.
//! A write window receiver (`&values[0..1].sort()`) keeps its `&place`
//! construction (D-SHAPE-PLACE1=A) and is not re-checked here.

use super::*;
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Diagnostics::type_is_copy;
use crate::Syntax;
use crate::AST::{AccessConvention, Expr, PlaceAccess, Type};

/// One written receiver mark, keyed in the checker by the marked place's span.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReceiverMark {
    access: PlaceAccess,
    mark_span: Span,
}

/// The receiver of one method call being inferred and the strongest
/// convention any receiver hook recorded for it.
#[derive(Debug, Clone)]
pub(crate) struct ReceiverMarkFrame {
    receiver: (usize, usize),
    convention: Option<AccessConvention>,
}

fn convention_rank(convention: AccessConvention) -> u8 {
    match convention {
        AccessConvention::Read => 0,
        AccessConvention::Write => 1,
        AccessConvention::Move => 2,
    }
}

/// The root name of a syntactic place: a name followed by fields, indexes,
/// or ranges. A call anywhere in the chain makes the receiver fresh.
fn place_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(name, _) => Some(name),
        Expr::Field(base, _, _) | Expr::Index { base, .. } | Expr::Slice { base, .. } => {
            place_root(base)
        }
        _ => None,
    }
}

/// The source span of a whole syntactic place. Field, index, and range nodes
/// span only their own suffix (`.buf`, `[i]`), so the place starts at its root.
pub(crate) fn place_span(expr: &Expr) -> Span {
    fn start(expr: &Expr) -> usize {
        match expr {
            Expr::Field(base, _, _) | Expr::Index { base, .. } | Expr::Slice { base, .. } => {
                start(base)
            }
            other => other.span().start,
        }
    }
    let span = expr.span();
    Span::new(start(expr).min(span.start), span.end)
}

fn is_window_type(ty: &Type) -> bool {
    match ty {
        Type::Apply { name, .. } => matches!(name.as_str(), "ViewMut" | "ComputeViewMut"),
        Type::Tagged { inner, .. } => is_window_type(inner),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    /// Lift a written receiver mark off `receiver` and open the call's frame.
    /// Must be paired with [`Checker::finish_receiver_mark`].
    pub(crate) fn begin_receiver_mark(&mut self, receiver: &mut Box<Expr>) {
        let lift = matches!(
            receiver.as_ref(),
            Expr::Place(inner, PlaceAccess::Write | PlaceAccess::Take, _)
                if !matches!(inner.as_ref(), Expr::Slice { .. })
        );
        if lift {
            let placeholder = Expr::Absent(receiver.span());
            if let Expr::Place(inner, access, span) =
                std::mem::replace(receiver.as_mut(), placeholder)
            {
                let inner_span = inner.span();
                self.receiver_marks.insert(
                    (inner_span.start, inner_span.end),
                    ReceiverMark {
                        access,
                        mark_span: Span::new(span.start, span.start + 1),
                    },
                );
                **receiver = *inner;
            }
        }
        let span = receiver.span();
        self.receiver_mark_frames.push(ReceiverMarkFrame {
            receiver: (span.start, span.end),
            convention: None,
        });
    }

    /// Receiver hooks report the convention a resolved call uses for this
    /// receiver. The strongest report wins (take over write over read).
    pub(crate) fn note_receiver_convention(&mut self, receiver: &Expr, convention: AccessConvention) {
        let span = receiver.span();
        let key = (span.start, span.end);
        if let Some(frame) = self
            .receiver_mark_frames
            .iter_mut()
            .rev()
            .find(|frame| frame.receiver == key)
        {
            let stronger = match frame.convention {
                None => true,
                Some(seen) => convention_rank(convention) > convention_rank(seen),
            };
            if stronger {
                frame.convention = Some(convention);
            }
        }
    }

    /// Close the call's frame and compare the written mark with the resolved
    /// receiver convention: E0224 missing, E0225 extra, E0226 mismatched.
    pub(crate) fn finish_receiver_mark(&mut self, receiver: &Expr) {
        let Some(frame) = self.receiver_mark_frames.pop() else {
            return;
        };
        if self.compiler_generated {
            return;
        }
        let mark = self.receiver_marks.get(&frame.receiver).copied();
        let root = place_root(receiver).filter(|root| self.lookup(root).is_some());
        if let Some(mark) = mark {
            self.check_written_receiver_mark(receiver, root.is_some(), mark, frame.convention);
            return;
        }
        let Some(root) = root else {
            // A fresh receiver (a call result or literal) takes no mark.
            return;
        };
        let span = place_span(receiver);
        // Only a place the user wrote gets a missing-mark report; lowering
        // and desugaring passes synthesize receivers with borrowed spans.
        if !self
            .source
            .get(span.start..span.end)
            .is_some_and(|text| text.starts_with(root))
        {
            return;
        }
        // D-COPY-DEFAULT1=A: a taking call on a whole owned local at its last
        // use moves it with no mark; the compiler chooses and `^` stays
        // optional. A writing call still needs `&`, and a take on a field or
        // on a local read again later still needs `^`.
        let unmarked_last_use_take = matches!(receiver, Expr::Ident(name, _)
            if self.lookup(name).is_some_and(|info| {
                matches!(info.param_conv, None | Some(AccessConvention::Move))
            }) && !self.is_name_live_after(name));
        let needed = match frame.convention {
            Some(AccessConvention::Write) => Syntax::SIGIL_WRITE,
            Some(AccessConvention::Move)
                if !unmarked_last_use_take
                    && !self.place_expr_type(receiver).is_some_and(|ty| type_is_copy(&ty)) =>
            {
                Syntax::SIGIL_MOVE
            }
            _ => return,
        };
        let (verb, method_decl) = if needed == Syntax::SIGIL_WRITE {
            ("writes", "&self")
        } else {
            ("takes", "^self")
        };
        let place = self
            .source
            .get(span.start..span.end)
            .unwrap_or(root)
            .to_string();
        let diagnostic = Diagnostic::error(
            "E0224",
            format!("this call {verb} `{place}`, but `{place}` has no `{needed}` mark"),
            format!(
                "the method is declared with `{method_decl}`, so the call {verb} the named place it is called on; the mark sits on that place, the same way `edit(&buf)` and `consume(^text)` mark arguments"
            ),
            format!("write `{needed}{place}`"),
            Some(span),
        )
        .with_edit(TextEdit {
            span: Span::new(span.start, span.start),
            new_text: needed.to_string(),
        });
        self.push_receiver_mark_diagnostic(diagnostic);
    }

    fn check_written_receiver_mark(
        &mut self,
        receiver: &Expr,
        is_place: bool,
        mark: ReceiverMark,
        convention: Option<AccessConvention>,
    ) {
        let written = match mark.access {
            PlaceAccess::Take => Syntax::SIGIL_MOVE,
            _ => Syntax::SIGIL_WRITE,
        };
        let remove = |what: String, why: &str| {
            Diagnostic::error(
                "E0225",
                what,
                why.to_string(),
                format!("remove `{written}`"),
                Some(mark.mark_span),
            )
            .with_edit(TextEdit {
                span: mark.mark_span,
                new_text: String::new(),
            })
        };
        if !is_place {
            let diagnostic = remove(
                format!("`{written}` marks a fresh value, not a named place"),
                "a mark binds a name plus fields, indexes, or ranges; a call result or literal is new, so the call receives it without a mark",
            );
            self.push_receiver_mark_diagnostic(diagnostic);
            return;
        }
        let Some(convention) = convention else {
            return;
        };
        let replace = |needed: &str, verb: &str| {
            Diagnostic::error(
                "E0226",
                format!("this call {verb} its receiver, so the mark is `{needed}`, not `{written}`"),
                "`&` marks a place the call writes and `^` a place the call takes; the resolved method declares which one".to_string(),
                format!("replace `{written}` with `{needed}`"),
                Some(mark.mark_span),
            )
            .with_edit(TextEdit {
                span: mark.mark_span,
                new_text: needed.to_string(),
            })
        };
        let diagnostic = match (convention, mark.access) {
            (AccessConvention::Write, PlaceAccess::Take) => replace(Syntax::SIGIL_WRITE, "writes"),
            (AccessConvention::Move, PlaceAccess::Write) => replace(Syntax::SIGIL_MOVE, "takes"),
            (AccessConvention::Read, PlaceAccess::Write)
                if self.place_expr_type(receiver).is_some_and(|ty| is_window_type(&ty)) =>
            {
                // Calls through a stored write window mark the window name.
                return;
            }
            (AccessConvention::Read, _) => remove(
                format!("this call only reads its receiver, so it takes no `{written}` mark"),
                "a mark shows that a call writes (`&`) or takes (`^`) the named place; a read call leaves the place unmarked",
            ),
            _ => return,
        };
        self.push_receiver_mark_diagnostic(diagnostic);
    }

    /// Re-inference may revisit a call; report each finding once.
    fn push_receiver_mark_diagnostic(&mut self, diagnostic: Diagnostic) {
        let duplicate = self
            .diags
            .iter()
            .any(|seen| seen.code == diagnostic.code && seen.span == diagnostic.span);
        if !duplicate {
            self.diags.push(diagnostic);
        }
    }
}
