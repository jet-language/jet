//! D-COMPILE-SPEED1 (#3661): one walk shape for structural questions about
//! named types: can a value of this type be cloned, does it own heap data,
//! can it cross a task boundary, does it hold a view, a Cell guard or a
//! `Shared` handle.
//!
//! Each question recurses through struct fields and enum payloads. A walk
//! that only guards the current path re-walks a type once for every path
//! that reaches it, which is exponential in the depth of sharing (every node
//! of a compiler AST reaches `Expr`, `Type` and `Span`), and each binding
//! asked again from scratch. Checking the self-hosted compiler's parser
//! slice spent minutes in those walks.
//!
//! Every such question stops at its first non-neutral answer (a problem
//! found, a field that cannot be cloned), so a named instantiation whose
//! walk finished while the query is still running finished neutral, and a
//! revisit adds nothing: `finished` makes one query linear in the type
//! graph. A type still on the path is assumed neutral (coinduction), as
//! before. When a whole query ends neutral, no problem is reachable from any
//! instantiation it entered, so those instantiations are neutral outright;
//! while body checking runs, the registry is final and its memo keeps them
//! for every later query. Parallel body checkers share one memo: a row only
//! records a proven answer, so the order in which checkers add rows cannot
//! change any answer.

use std::sync::{PoisonError, RwLock};
use std::collections::{HashMap, HashSet};

use super::TypeRegistry;
use crate::AST::Type;

/// Which structural question a walk answers. Answers of different questions
/// never share memo rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NominalQuery {
    Cloneable,
    OwnsHeap,
    Sendable {
        strict_callable: bool,
        cell_only: bool,
    },
    ViewBoundary,
    CellGuard,
    SharedHandle,
    /// Can a value hold an observable `Clock`. Keyed by nominal name in the
    /// checked module's own registry (see `type_contains_observable_clock`).
    ObservableClock,
}

/// Named instantiations proven neutral for each question while the
/// registry is final.
#[derive(Default)]
struct NominalMemo {
    neutral: HashMap<NominalQuery, HashSet<String>>,
}

/// The memo field of `TypeRegistry`: closed (`None`) while registration can
/// still change the registered types, so no answer can go stale. A copy of a
/// registry may be changed afterwards, so it starts closed too.
#[derive(Default)]
pub(crate) struct NominalMemoCell(RwLock<Option<NominalMemo>>);

impl Clone for NominalMemoCell {
    fn clone(&self) -> Self {
        Self::default()
    }
}

pub(crate) struct NominalWalk<'r> {
    query: NominalQuery,
    registry: &'r TypeRegistry,
    /// Named types on the current path, by name alone, so a generic type
    /// that recurses with growing arguments still terminates.
    active: HashSet<String>,
    /// Instantiations this query already walked to completion.
    finished: HashSet<String>,
}

impl<'r> NominalWalk<'r> {
    pub(crate) fn new(query: NominalQuery, registry: &'r TypeRegistry) -> Self {
        Self {
            query,
            registry,
            active: HashSet::new(),
            finished: HashSet::new(),
        }
    }

    /// Enter the named instantiation `name<args>`. `None` means the caller
    /// answers neutral without walking it: it is on the current path, this
    /// query already finished it, or the registry memo holds it. `Some(key)`
    /// must be handed back to `leave` once its fields are walked.
    pub(crate) fn enter(&mut self, name: &str, args: &[Type]) -> Option<String> {
        if self.active.contains(name) {
            return None;
        }
        let key = instantiation_key(name, args);
        if self.finished.contains(&key) || self.memo_holds(&key) {
            return None;
        }
        self.active.insert(name.to_string());
        Some(key)
    }

    pub(crate) fn leave(&mut self, name: &str, key: String) {
        self.active.remove(name);
        self.finished.insert(key);
    }

    /// End the query. A neutral answer proves every entered instantiation
    /// neutral, so the open registry memo keeps them.
    pub(crate) fn finish(self, neutral: bool) {
        if !neutral || self.finished.is_empty() {
            return;
        }
        self.registry.record_neutral_nominals(self.query, self.finished);
    }

    fn memo_holds(&self, key: &str) -> bool {
        self.registry.nominal_memo_holds(self.query, key)
    }
}

fn instantiation_key(name: &str, args: &[Type]) -> String {
    if args.is_empty() {
        return name.to_string();
    }
    let mut key = String::with_capacity(name.len() + 16 * args.len());
    key.push_str(name);
    key.push('<');
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            key.push_str(", ");
        }
        key.push_str(&arg.name());
    }
    key.push('>');
    key
}

/// Keeps the registry memo open for one body-checking pass and closes it
/// when dropped. A nested scope over an already open memo leaves it to the
/// outer one.
pub(crate) struct NominalMemoScope<'r> {
    registry: &'r TypeRegistry,
    opened: bool,
}

impl TypeRegistry {
    /// Open the memo once the registered types are final: the caller holds
    /// the registry shared for the whole scope, so no type can change
    /// underneath a remembered answer.
    pub(crate) fn open_nominal_memo(&self) -> NominalMemoScope<'_> {
        let mut memo = self
            .nominal_memo
            .0
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let opened = memo.is_none();
        if opened {
            *memo = Some(NominalMemo::default());
        }
        NominalMemoScope {
            registry: self,
            opened,
        }
    }

    /// Whether the open memo already proved `key` neutral for `query`.
    pub(crate) fn nominal_memo_holds(&self, query: NominalQuery, key: &str) -> bool {
        self.nominal_memo
            .0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|memo| memo.neutral.get(&query))
            .is_some_and(|neutral| neutral.contains(key))
    }

    /// Record `keys` as neutral for `query` while the memo is open; a closed
    /// memo keeps nothing.
    pub(crate) fn record_neutral_nominals(
        &self,
        query: NominalQuery,
        keys: impl IntoIterator<Item = String>,
    ) {
        if let Some(memo) = self
            .nominal_memo
            .0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            memo.neutral.entry(query).or_default().extend(keys);
        }
    }
}

impl Drop for NominalMemoScope<'_> {
    fn drop(&mut self) {
        if self.opened {
            *self
                .registry
                .nominal_memo
                .0
                .write()
                .unwrap_or_else(PoisonError::into_inner) = None;
        }
    }
}
