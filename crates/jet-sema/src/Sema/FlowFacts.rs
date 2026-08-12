//! One flow-fact store, one join rule (D-FACT-FLOW1, card #1621).
//!
//! Every fact the checker holds about one binding lives here, keyed by binding
//! and plane. A plane supplies **only** its join rule; this module owns every
//! merge point, so a branch or a loop can never keep whatever path the walker
//! happened to see last.
//!
//! Planes that are declared here as fact shapes:
//!
//! | plane | fact | where the walker lives |
//! |---|---|---|
//! | [`Binding`] | everything a declaration says about a name | `CheckerCore` |
//! | [`Narrow`] | a binding refined by a proven test (D-FLOWTYPE1) | `CheckerCore` |
//! | [`Moved`] | the use that gave a place away | `CheckerOwnership` |
//! | [`Uninit`] | a `Type.{ uninit }` place not yet written (D-UNINIT1) | `CheckerCore` |
//! | [`View`] | open borrow windows over a place (D-MEM1 S9) | `CheckerOwnership` |
//! | `Typestate` | the state a value is in (D-STATE1) | `Sema::State` |
//! | `Taint` | the fact tags a value carries (D-TAG-SURFACE1) | `Sema::Taint` |
//!
//! Typestate and taint declare their own planes next to their walkers; every
//! plane uses the [`Facts`] store and the merge rules below.

use std::collections::HashMap;
use std::marker::PhantomData;

/// One family of per-binding facts.
pub(crate) trait Plane {
    /// What this plane knows about one binding.
    type Fact: Clone;

    /// Join one binding's fact from two paths that meet.
    ///
    /// `None` on a side means that path holds no fact for the binding.
    /// `None` out drops the fact: the merge proves nothing.
    fn join(left: Option<&Self::Fact>, right: Option<&Self::Fact>) -> Option<Self::Fact>;

    /// True for a fact no branch can undo — a place given away, a window still
    /// open. The pre-branch store then counts as one of the paths that meet, so
    /// a branch can never forget what the code before it already knew.
    ///
    /// False when a branch *can* discharge the fact on every path: a place
    /// written in every arm is written, a value scrubbed in every arm is clean,
    /// a state every arm agrees on holds. Those planes see only the real paths;
    /// when one path skips the arms, the walker passes the pre-branch store as
    /// that path.
    const KEEPS_PRE_MERGE: bool = false;

    /// True when the walker reports the bindings this plane's join refused to
    /// keep because the two paths disagreed. Only typestate does today.
    const REPORTS_DIVERGENCE: bool = false;
}

/// One binding whose fact the plane refused to join because the paths disagreed.
#[derive(Debug, Clone)]
pub(crate) struct Divergence<F> {
    pub(crate) name: String,
    pub(crate) left: F,
    pub(crate) right: F,
}

#[derive(Debug, Clone)]
struct Row<F> {
    /// Number of open scopes when the fact was recorded. A row leaves when the
    /// scope that recorded it closes. Planes with no lexical nesting use 0.
    depth: usize,
    fact: F,
}

/// Per-binding facts of one plane. A name may carry one row per scope depth, so
/// an inner declaration shadows an outer one and the outer fact returns when
/// the inner scope closes.
pub(crate) struct Facts<P: Plane> {
    /// Rows per name, ordered by increasing depth (innermost last).
    rows: HashMap<String, Vec<Row<P::Fact>>>,
    plane: PhantomData<fn() -> P>,
}

impl<P: Plane> Default for Facts<P> {
    fn default() -> Self {
        Self {
            rows: HashMap::new(),
            plane: PhantomData,
        }
    }
}

impl<P: Plane> Clone for Facts<P> {
    fn clone(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            plane: PhantomData,
        }
    }
}

impl<P: Plane> std::fmt::Debug for Facts<P>
where
    P::Fact: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.rows.iter().map(|(name, rows)| (name, rows)))
            .finish()
    }
}

impl<P: Plane> Facts<P> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The innermost fact for this binding.
    pub(crate) fn get(&self, name: &str) -> Option<&P::Fact> {
        self.rows.get(name)?.last().map(|row| &row.fact)
    }

    /// The fact this binding carries at exactly one scope depth.
    pub(crate) fn get_at(&self, name: &str, depth: usize) -> Option<&P::Fact> {
        self.rows
            .get(name)?
            .iter()
            .find(|row| row.depth == depth)
            .map(|row| &row.fact)
    }

    /// The depth of this binding's innermost row.
    pub(crate) fn depth_of(&self, name: &str) -> Option<usize> {
        self.rows.get(name)?.last().map(|row| row.depth)
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.rows.get(name).is_some_and(|rows| !rows.is_empty())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.values().all(|rows| rows.is_empty())
    }

    /// Record a fact at one scope depth, replacing this binding's row there.
    pub(crate) fn set_at(&mut self, name: &str, depth: usize, fact: P::Fact) {
        let rows = self.rows.entry(name.to_string()).or_default();
        match rows.iter().position(|row| row.depth >= depth) {
            Some(index) if rows[index].depth == depth => rows[index].fact = fact,
            Some(index) => rows.insert(index, Row { depth, fact }),
            None => rows.push(Row { depth, fact }),
        }
    }

    /// Record a fact that has no lexical nesting of its own.
    pub(crate) fn set(&mut self, name: &str, fact: P::Fact) {
        self.set_at(name, 0, fact);
    }

    /// This binding's fact at one scope depth, created empty when absent.
    pub(crate) fn entry_at(&mut self, name: &str, depth: usize) -> &mut P::Fact
    where
        P::Fact: Default,
    {
        if self.get_at(name, depth).is_none() {
            self.set_at(name, depth, P::Fact::default());
        }
        let rows = self.rows.get_mut(name).expect("row just recorded");
        let index = rows
            .iter()
            .position(|row| row.depth == depth)
            .expect("row just recorded");
        &mut rows[index].fact
    }

    /// Forget this binding's innermost fact.
    pub(crate) fn remove(&mut self, name: &str) -> Option<P::Fact> {
        let rows = self.rows.get_mut(name)?;
        let fact = rows.pop().map(|row| row.fact);
        if rows.is_empty() {
            self.rows.remove(name);
        }
        fact
    }

    /// Forget this binding's fact at exactly one scope depth.
    pub(crate) fn remove_at(&mut self, name: &str, depth: usize) {
        let Some(rows) = self.rows.get_mut(name) else {
            return;
        };
        rows.retain(|row| row.depth != depth);
        if rows.is_empty() {
            self.rows.remove(name);
        }
    }

    /// This binding's facts at every depth, outermost first.
    pub(crate) fn facts_of(&self, name: &str) -> impl Iterator<Item = &P::Fact> {
        self.rows
            .get(name)
            .into_iter()
            .flatten()
            .map(|row| &row.fact)
    }

    /// Every fact in the store, at every depth.
    pub(crate) fn all_mut(&mut self) -> impl Iterator<Item = &mut P::Fact> {
        self.rows
            .values_mut()
            .flatten()
            .map(|row| &mut row.fact)
    }

    /// Every fact in the store, at every depth, with its binding.
    pub(crate) fn all(&self) -> impl Iterator<Item = (&str, &P::Fact)> {
        self.rows
            .iter()
            .flat_map(|(name, rows)| rows.iter().map(move |row| (name.as_str(), &row.fact)))
    }

    /// Innermost fact per binding.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &P::Fact)> {
        self.rows
            .iter()
            .filter_map(|(name, rows)| Some((name.as_str(), &rows.last()?.fact)))
    }

    /// Facts recorded at exactly one scope depth.
    pub(crate) fn iter_at(&self, depth: usize) -> impl Iterator<Item = (&str, &P::Fact)> {
        self.rows.iter().filter_map(move |(name, rows)| {
            let row = rows.iter().find(|row| row.depth == depth)?;
            Some((name.as_str(), &row.fact))
        })
    }

    /// Keep only the bindings whose innermost fact passes the test.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&str, &P::Fact) -> bool) {
        self.rows.retain(|name, rows| {
            rows.retain(|row| keep(name, &row.fact));
            !rows.is_empty()
        });
    }

    /// Drop every fact recorded at `depth` or deeper. This is scope exit.
    pub(crate) fn leave_depth(&mut self, depth: usize) {
        self.rows.retain(|_, rows| {
            rows.retain(|row| row.depth < depth);
            !rows.is_empty()
        });
    }

    /// The one two-path join. Every binding either side knows about is joined
    /// by the plane, depth by depth; a plane that proves nothing drops it.
    pub(crate) fn join_paths(
        &self,
        other: &Self,
        diverged: &mut Vec<Divergence<P::Fact>>,
    ) -> Self {
        let mut out = Self::new();
        let mut seen = std::collections::HashSet::new();
        for name in self.rows.keys().chain(other.rows.keys()) {
            if !seen.insert(name) {
                continue;
            }
            let mut depths: Vec<usize> = self
                .rows
                .get(name)
                .into_iter()
                .chain(other.rows.get(name))
                .flatten()
                .map(|row| row.depth)
                .collect();
            depths.sort_unstable();
            depths.dedup();
            let mut rows = Vec::new();
            for depth in depths {
                let left = self.get_at(name, depth);
                let right = other.get_at(name, depth);
                match P::join(left, right) {
                    Some(fact) => rows.push(Row { depth, fact }),
                    None => {
                        if P::REPORTS_DIVERGENCE {
                            if let (Some(left), Some(right)) = (left, right) {
                                diverged.push(Divergence {
                                    name: name.clone(),
                                    left: left.clone(),
                                    right: right.clone(),
                                });
                            }
                        }
                    }
                }
            }
            if !rows.is_empty() {
                out.rows.insert(name.clone(), rows);
            }
        }
        out
    }

    /// The one branch-merge rule. `paths` holds the store as each path through
    /// the branch left it, including the fall-through path when the branch has
    /// no `else`. A hazard plane also counts the pre-branch store as a path, so
    /// no branch can forget a hazard it already knew (`KEEPS_PRE_MERGE`).
    pub(crate) fn merge_paths(
        before: &Self,
        paths: &[Self],
        diverged: &mut Vec<Divergence<P::Fact>>,
    ) -> Self {
        let Some((first, rest)) = paths.split_first() else {
            return before.clone();
        };
        let mut out = first.clone();
        for path in rest {
            out = out.join_paths(path, diverged);
        }
        if P::KEEPS_PRE_MERGE {
            out = out.join_paths(before, diverged);
        }
        out
    }

    /// The one loop rule. A loop body runs zero times or many, so the facts
    /// after a loop are the join of the facts before it with the facts one walk
    /// of the body left behind — the zero-turn path and the at-least-one-turn
    /// path are the two paths that meet.
    ///
    /// One walk is the fixpoint: a proof plane's join only ever drops facts, so
    /// a second walk over the joined store re-derives the same rows, and a
    /// hazard plane's join only ever keeps them, so the first walk already found
    /// every hazard the body states.
    // ponytail: one walk. A hazard a later turn of the loop would introduce
    // *before* an earlier statement reads it needs a second walk; widen to a
    // real fixpoint loop here if a plane ever needs it.
    pub(crate) fn after_loop(
        before: &Self,
        after_body: &Self,
        diverged: &mut Vec<Divergence<P::Fact>>,
    ) -> Self {
        before.join_paths(after_body, diverged)
    }
}

/// An else-less pattern-arm table CheckerCore's own coverage check
/// (`switches.rs::check_pattern_coverage_complete`) already proved covers
/// every case — no E0307 was reported at this span. Mirrors
/// `can_skip_every_arm` there: the satellite walkers (`Sema::State`,
/// `Sema::Taint`) don't type-check, so they read that verdict off the
/// diagnostics CheckerCore already produced instead of re-deriving it, and
/// only treat "skip every arm" as a real path when CheckerCore could not
/// prove the table exhaustive.
pub(crate) fn switch_proven_exhaustive(
    arms: &[crate::AST::SwitchArm],
    existing_diags: &[crate::Diagnostics::Diagnostic],
    span: crate::Diagnostics::Span,
) -> bool {
    !arms.is_empty()
        && arms
            .iter()
            .all(|a| matches!(a.cond, crate::AST::Expr::PatternTest { .. }))
        && !existing_diags
            .iter()
            .any(|d| d.code == "E0307" && d.span == Some(span))
}

/// A binding fact both paths carry survives the merge; scope exit, not the
/// join, is what ends a binding's life.
fn keep_left<F: Clone>(left: Option<&F>, right: Option<&F>) -> Option<F> {
    left.or(right).cloned()
}

/// A refinement holds after the merge only when every path proved it.
fn keep_if_both<F: Clone>(left: Option<&F>, right: Option<&F>) -> Option<F> {
    match (left, right) {
        (Some(left), Some(_)) => Some(left.clone()),
        _ => None,
    }
}

/// Everything a declaration says about one name.
pub(crate) enum Binding {}

impl Plane for Binding {
    type Fact = super::LocalInfo;

    fn join(left: Option<&Self::Fact>, right: Option<&Self::Fact>) -> Option<Self::Fact> {
        keep_left(left, right)
    }
}

/// D-FLOWTYPE1: a binding a proven test refined for the rest of a path
/// (`x != None` refines `T?` to `T`). The refined row shadows the declaration
/// at the same depth and leaves with it.
pub(crate) enum Narrow {}

impl Plane for Narrow {
    type Fact = super::LocalInfo;

    fn join(left: Option<&Self::Fact>, right: Option<&Self::Fact>) -> Option<Self::Fact> {
        keep_if_both(left, right)
    }
}

/// The use that gave a place away, keyed by place name (`order`, `order.line`).
pub(crate) enum Moved {}

impl Plane for Moved {
    type Fact = super::Span;

    fn join(left: Option<&Self::Fact>, right: Option<&Self::Fact>) -> Option<Self::Fact> {
        keep_left(left, right)
    }

    const KEEPS_PRE_MERGE: bool = true;
}

/// D-UNINIT1: a `Type.{ uninit }` place that is not yet definitely written.
/// The fact is the hazard, so a place written on one path only stays here, with
/// the initialised parts intersected.
pub(crate) enum Uninit {}

impl Plane for Uninit {
    type Fact = super::UninitState;

    fn join(left: Option<&Self::Fact>, right: Option<&Self::Fact>) -> Option<Self::Fact> {
        match (left, right) {
            (Some(left), Some(right)) => {
                let mut merged = left.clone();
                merged.merge_paths(right);
                Some(merged)
            }
            (Some(one), None) | (None, Some(one)) => Some(one.clone()),
            (None, None) => None,
        }
    }
}

/// D-MEM1 S9: the borrow windows open over one binding. Storage lives here;
/// invalidation stays with the ownership prover (D-FACT-OWN1).
pub(crate) enum View {}

impl Plane for View {
    type Fact = Vec<super::ViewFact>;

    fn join(left: Option<&Self::Fact>, right: Option<&Self::Fact>) -> Option<Self::Fact> {
        match (left, right) {
            (Some(left), Some(right)) => {
                let mut merged = left.clone();
                for fact in right {
                    let same = merged.iter_mut().find(|held| {
                        held.binding_span == fact.binding_span
                            && held.output_path == fact.output_path
                    });
                    match same {
                        // One window, seen down both paths. A path that killed
                        // the storage is the one that decides: an invalidated
                        // window never comes back to life at a merge.
                        Some(held) => {
                            if held.invalidated.is_none() {
                                held.invalidated = fact.invalidated.clone();
                            }
                        }
                        None => merged.push(fact.clone()),
                    }
                }
                Some(merged)
            }
            (Some(one), None) | (None, Some(one)) => Some(one.clone()),
            (None, None) => None,
        }
    }

    const KEEPS_PRE_MERGE: bool = true;
}

/// Every per-binding fact the checker holds, in one store.
#[derive(Debug, Clone)]
pub(crate) struct FlowFacts {
    /// Open scopes. A row recorded at depth `n` leaves when scope `n` closes.
    pub(crate) depth: usize,
    /// Whether this path can reach the next statement in its enclosing block.
    /// Exit statements clear it; branch and loop joins discard unreachable
    /// paths before asking the individual fact planes to join.
    pub(crate) reachable: bool,
    pub(crate) bindings: Facts<Binding>,
    pub(crate) narrow: Facts<Narrow>,
    pub(crate) moved: Facts<Moved>,
    pub(crate) uninit: Facts<Uninit>,
    pub(crate) views: Facts<View>,
}

impl Default for FlowFacts {
    fn default() -> Self {
        Self {
            depth: 0,
            reachable: true,
            bindings: Facts::default(),
            narrow: Facts::default(),
            moved: Facts::default(),
            uninit: Facts::default(),
            views: Facts::default(),
        }
    }
}

impl FlowFacts {
    pub(crate) fn enter_scope(&mut self) {
        self.depth += 1;
    }

    /// Scope exit: every plane drops the rows that scope recorded.
    pub(crate) fn leave_scope(&mut self) {
        let depth = self.depth;
        self.bindings.leave_depth(depth);
        self.narrow.leave_depth(depth);
        self.views.leave_depth(depth);
        self.depth = depth.saturating_sub(1);
    }

    /// The one merge point for the checker's planes. `paths` holds the store as
    /// each path through the branch left it.
    pub(crate) fn merge_paths(before: &Self, paths: &[Self]) -> Self {
        if !before.reachable {
            return before.clone();
        }
        let paths: Vec<Self> = paths
            .iter()
            .filter(|path| path.reachable)
            .cloned()
            .collect();
        if paths.is_empty() {
            let mut exited = before.clone();
            exited.reachable = false;
            return exited;
        }
        let mut sink = Vec::new();
        let mut view_sink = Vec::new();
        Self {
            depth: before.depth,
            reachable: true,
            bindings: Facts::merge_paths(
                &before.bindings,
                &Self::plane(&paths, |facts| &facts.bindings),
                &mut sink,
            ),
            narrow: Facts::merge_paths(
                &before.narrow,
                &Self::plane(&paths, |facts| &facts.narrow),
                &mut sink,
            ),
            moved: Facts::merge_paths(
                &before.moved,
                &Self::plane(&paths, |facts| &facts.moved),
                &mut Vec::new(),
            ),
            uninit: Facts::merge_paths(
                &before.uninit,
                &Self::plane(&paths, |facts| &facts.uninit),
                &mut Vec::new(),
            ),
            views: Facts::merge_paths(
                &before.views,
                &Self::plane(&paths, |facts| &facts.views),
                &mut view_sink,
            ),
        }
    }

    /// The one loop rule ([`Facts::after_loop`]), applied to every plane.
    pub(crate) fn after_loop(before: &Self, after_body: &Self) -> Self {
        if !before.reachable || !after_body.reachable {
            return before.clone();
        }
        Self {
            depth: before.depth,
            reachable: true,
            bindings: Facts::after_loop(&before.bindings, &after_body.bindings, &mut Vec::new()),
            narrow: Facts::after_loop(&before.narrow, &after_body.narrow, &mut Vec::new()),
            moved: Facts::after_loop(&before.moved, &after_body.moved, &mut Vec::new()),
            uninit: Facts::after_loop(&before.uninit, &after_body.uninit, &mut Vec::new()),
            views: Facts::after_loop(&before.views, &after_body.views, &mut Vec::new()),
        }
    }

    fn plane<P: Plane>(paths: &[Self], pick: impl Fn(&Self) -> &Facts<P>) -> Vec<Facts<P>> {
        paths.iter().map(|facts| pick(facts).clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    enum Hazard {}

    impl Plane for Hazard {
        type Fact = u32;

        fn join(left: Option<&u32>, right: Option<&u32>) -> Option<u32> {
            keep_left(left, right)
        }

        const KEEPS_PRE_MERGE: bool = true;
    }

    enum Proof {}

    impl Plane for Proof {
        type Fact = u32;

        fn join(left: Option<&u32>, right: Option<&u32>) -> Option<u32> {
            match (left, right) {
                (Some(left), Some(right)) if left == right => Some(*left),
                _ => None,
            }
        }

        const REPORTS_DIVERGENCE: bool = true;
    }

    fn facts<P: Plane<Fact = u32>>(rows: &[(&str, u32)]) -> Facts<P> {
        let mut out = Facts::new();
        for (name, fact) in rows {
            out.set(name, *fact);
        }
        out
    }

    #[test]
    fn hazard_on_one_path_survives_the_merge() {
        let before: Facts<Hazard> = facts(&[]);
        let then = facts(&[("order", 7)]);
        let otherwise = facts(&[]);
        let merged = Facts::merge_paths(&before, &[then, otherwise], &mut Vec::new());
        assert_eq!(merged.get("order"), Some(&7));
    }

    #[test]
    fn hazard_before_the_branch_is_never_forgotten() {
        let before: Facts<Hazard> = facts(&[("order", 1)]);
        let then = facts(&[]);
        let merged = Facts::merge_paths(&before, &[then], &mut Vec::new());
        assert_eq!(merged.get("order"), Some(&1));
    }

    #[test]
    fn disagreeing_paths_drop_the_proof_and_report_it() {
        let before: Facts<Proof> = facts(&[("order", 1)]);
        let then = facts(&[("order", 2)]);
        let otherwise = facts(&[("order", 3)]);
        let mut diverged = Vec::new();
        let merged = Facts::merge_paths(&before, &[then, otherwise], &mut diverged);
        assert_eq!(merged.get("order"), None);
        assert_eq!(diverged.len(), 1);
        assert_eq!(diverged[0].name, "order");
    }

    #[test]
    fn agreeing_paths_keep_the_proof() {
        let before: Facts<Proof> = facts(&[("order", 1)]);
        let then = facts(&[("order", 2)]);
        let otherwise = facts(&[("order", 2)]);
        let merged = Facts::merge_paths(&before, &[then, otherwise], &mut Vec::new());
        assert_eq!(merged.get("order"), Some(&2));
    }

    #[test]
    fn a_loop_body_that_may_not_run_keeps_the_pre_loop_proof() {
        let before: Facts<Proof> = facts(&[("order", 1)]);
        let after_body = facts(&[("order", 1)]);
        let merged = Facts::after_loop(&before, &after_body, &mut Vec::new());
        assert_eq!(merged.get("order"), Some(&1));
    }

    #[test]
    fn a_loop_body_that_changes_a_proof_drops_it() {
        let before: Facts<Proof> = facts(&[("order", 1)]);
        let after_body = facts(&[("order", 2)]);
        let merged = Facts::after_loop(&before, &after_body, &mut Vec::new());
        assert_eq!(merged.get("order"), None);
    }

    #[test]
    fn an_inner_row_shadows_an_outer_one_and_leaves_with_its_scope() {
        let mut store: Facts<Hazard> = Facts::new();
        store.set_at("window", 1, 10);
        store.set_at("window", 2, 20);
        assert_eq!(store.get("window"), Some(&20));
        store.leave_depth(2);
        assert_eq!(store.get("window"), Some(&10));
    }
}
