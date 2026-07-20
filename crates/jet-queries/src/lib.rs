//! Demand-driven query cache for Jet front-end clients.
//!
//! This crate is intentionally std-only. Compiler/LSP-specific work stays in
//! callers; this layer only owns inputs, memoization, dependency tracking, and
//! revision invalidation.

use std::any::Any;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FileKey(String);

impl FileKey {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum Subject {
    Named(String),
    File(FileKey),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InputKey(Subject);

impl InputKey {
    pub fn new(name: impl Into<String>) -> Self {
        Self(Subject::Named(name.into()))
    }

    pub fn file(file: FileKey) -> Self {
        Self(Subject::File(file))
    }

}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct QueryKey {
    kind: &'static str,
    subject: Subject,
}

impl QueryKey {
    pub fn new(kind: &'static str, name: impl Into<String>) -> Self {
        QueryKey {
            kind,
            subject: Subject::Named(name.into()),
        }
    }

    pub fn for_file(kind: &'static str, file: FileKey) -> Self {
        Self {
            kind,
            subject: Subject::File(file),
        }
    }

}

struct InputCell {
    revision: u64,
    text: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum DepKey {
    Input(InputKey),
    Query(QueryKey),
}

#[derive(Clone, Debug)]
enum Dep {
    Input(InputKey, Option<u64>),
    Query(QueryKey, u64),
}

struct MemoEntry {
    deps: Vec<Dep>,
    value: Box<dyn Any>,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QueryStats {
    pub hits: u64,
    pub recomputes: u64,
    pub live_inputs: usize,
    pub live_input_bytes: usize,
    pub live_memos: usize,
    pub live_query_counters: usize,
}

#[derive(Default)]
pub struct QueryEngine {
    revision: u64,
    inputs: HashMap<InputKey, InputCell>,
    memo: HashMap<QueryKey, MemoEntry>,
    recompute_by_key: HashMap<QueryKey, u64>,
    dep_stack: Vec<Vec<DepKey>>,
    hits: u64,
    recomputes: u64,
}

impl QueryEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_input(&mut self, key: InputKey, text: String) -> bool {
        if let Some(cell) = self.inputs.get_mut(&key) {
            if cell.text == text {
                return false;
            }
            self.revision += 1;
            cell.revision = self.revision;
            cell.text = text;
            self.prune_invalid_memos(false);
            return true;
        }

        self.revision += 1;
        let revision = self.revision;
        self.inputs.insert(key, InputCell { revision, text });
        self.prune_invalid_memos(false);
        true
    }

    pub fn remove_input(&mut self, key: &InputKey) -> bool {
        let removed = self.inputs.remove(key).is_some();
        if removed {
            self.revision += 1;
            self.prune_invalid_memos(true);
            self.recompute_by_key
                .retain(|query, _| query.subject != key.0);
        }
        removed
    }

    pub fn input_text(&mut self, key: &InputKey) -> Option<String> {
        self.record_dep(key);
        self.inputs.get(key).map(|cell| cell.text.clone())
    }

    pub fn query<T, F>(&mut self, key: QueryKey, compute: F) -> T
    where
        T: Clone + 'static,
        F: FnOnce(&mut QueryEngine) -> T,
    {
        self.record_query_dep(&key);
        let cached = self.memo.get(&key).and_then(|entry| {
            (self.memo_entry_valid(entry, &mut HashSet::new()))
                .then(|| entry.value.downcast_ref::<T>().cloned())
                .flatten()
        });
        if let Some(value) = cached {
            self.hits += 1;
            return value;
        }

        self.memo.remove(&key);
        self.dep_stack.push(Vec::new());
        let value = compute(self);
        let deps = self.finish_deps();
        let generation = self.recompute_by_key.get(&key).copied().unwrap_or(0) + 1;
        self.recompute_by_key.insert(key.clone(), generation);
        self.recomputes += 1;
        self.memo.insert(
            key,
            MemoEntry {
                deps,
                value: Box::new(value.clone()),
                generation,
            },
        );
        value
    }

    pub fn recompute_count(&self, key: &QueryKey) -> u64 {
        self.recompute_by_key.get(key).copied().unwrap_or(0)
    }

    pub fn stats(&self) -> QueryStats {
        QueryStats {
            hits: self.hits,
            recomputes: self.recomputes,
            live_inputs: self.inputs.len(),
            live_input_bytes: self.inputs.values().map(|cell| cell.text.len()).sum(),
            live_memos: self.memo.len(),
            live_query_counters: self.recompute_by_key.len(),
        }
    }

    pub fn invalidate_kind(&mut self, kind: &'static str) {
        let dead = self
            .memo
            .keys()
            .filter(|key| key.kind == kind)
            .cloned()
            .collect::<Vec<_>>();
        for key in dead {
            self.memo.remove(&key);
        }
    }

    pub fn invalidate(&mut self, key: &QueryKey) {
        if self.memo.remove(key).is_some() {
            self.prune_invalid_memos(false);
        }
    }

    fn record_dep(&mut self, key: &InputKey) {
        if let Some(deps) = self.dep_stack.last_mut() {
            deps.push(DepKey::Input(key.clone()));
        }
    }

    fn record_query_dep(&mut self, key: &QueryKey) {
        if let Some(deps) = self.dep_stack.last_mut() {
            deps.push(DepKey::Query(key.clone()));
        }
    }

    fn finish_deps(&mut self) -> Vec<Dep> {
        let deps = self.dep_stack.pop().unwrap_or_default();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for dep in deps {
            if !seen.insert(dep.clone()) {
                continue;
            }
            match dep {
                DepKey::Input(input) => {
                    let revision = self.inputs.get(&input).map(|cell| cell.revision);
                    out.push(Dep::Input(input, revision));
                }
                DepKey::Query(query) => {
                    if let Some(entry) = self.memo.get(&query) {
                        out.push(Dep::Query(query, entry.generation));
                    }
                }
            }
        }
        out
    }

    fn memo_entry_valid(&self, entry: &MemoEntry, visiting: &mut HashSet<QueryKey>) -> bool {
        entry.deps.iter().all(|dep| match dep {
            Dep::Input(input, revision) => {
                self.inputs.get(input).map(|cell| cell.revision) == *revision
            }
            Dep::Query(query, recomputes) => self
                .memo
                .get(query)
                .map(|entry| {
                    entry.generation == *recomputes && self.memo_key_valid(query, visiting)
                })
                .unwrap_or(false),
        })
    }

    fn memo_key_valid(&self, key: &QueryKey, visiting: &mut HashSet<QueryKey>) -> bool {
        if !visiting.insert(key.clone()) {
            return true;
        }
        let valid = self
            .memo
            .get(key)
            .map(|entry| self.memo_entry_valid(entry, visiting))
            .unwrap_or(false);
        visiting.remove(key);
        valid
    }

    fn prune_invalid_memos(&mut self, purge_counters: bool) {
        let dead = self
            .memo
            .keys()
            .filter(|key| !self.memo_key_valid(key, &mut HashSet::new()))
            .cloned()
            .collect::<Vec<_>>();
        for key in dead {
            self.memo.remove(&key);
            if purge_counters {
                self.recompute_by_key.remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word_count(engine: &mut QueryEngine, path: &str) -> usize {
        let input = InputKey::new(path);
        engine.query(QueryKey::new("tokens", path), |db| {
            db.input_text(&input)
                .unwrap_or_default()
                .split_whitespace()
                .count()
        })
    }

    #[test]
    fn unchanged_input_reuses_memo() {
        let mut db = QueryEngine::new();
        db.set_input(InputKey::new("a.jet"), "one two".to_string());

        assert_eq!(word_count(&mut db, "a.jet"), 2);
        assert_eq!(word_count(&mut db, "a.jet"), 2);

        let key = QueryKey::new("tokens", "a.jet");
        assert_eq!(db.recompute_count(&key), 1);
    }

    #[test]
    fn local_edit_only_recomputes_dependent_query() {
        let mut db = QueryEngine::new();
        for (path, text) in [("a.jet", "fn a"), ("b.jet", "fn b"), ("c.jet", "fn c")] {
            db.set_input(InputKey::new(path), text.to_string());
            assert_eq!(word_count(&mut db, path), 2);
        }

        db.set_input(InputKey::new("b.jet"), "fn b changed".to_string());
        assert_eq!(word_count(&mut db, "a.jet"), 2);
        assert_eq!(word_count(&mut db, "b.jet"), 3);
        assert_eq!(word_count(&mut db, "c.jet"), 2);

        assert_eq!(db.recompute_count(&QueryKey::new("tokens", "a.jet")), 1);
        assert_eq!(db.recompute_count(&QueryKey::new("tokens", "b.jet")), 2);
        assert_eq!(db.recompute_count(&QueryKey::new("tokens", "c.jet")), 1);
    }

    #[test]
    fn multi_file_query_recomputes_when_any_input_changes() {
        let mut db = QueryEngine::new();
        db.set_input(InputKey::new("a.jet"), "alpha".to_string());
        db.set_input(InputKey::new("b.jet"), "beta".to_string());
        let key = QueryKey::new("module_graph", "root");

        let first = db.query(key.clone(), |q| {
            let a = q.input_text(&InputKey::new("a.jet")).unwrap_or_default();
            let b = q.input_text(&InputKey::new("b.jet")).unwrap_or_default();
            format!("{}+{}", a, b)
        });
        assert_eq!(first, "alpha+beta");

        db.set_input(InputKey::new("a.jet"), "alpha2".to_string());
        let second = db.query(key.clone(), |q| {
            let a = q.input_text(&InputKey::new("a.jet")).unwrap_or_default();
            let b = q.input_text(&InputKey::new("b.jet")).unwrap_or_default();
            format!("{}+{}", a, b)
        });
        assert_eq!(second, "alpha2+beta");
        assert_eq!(db.recompute_count(&key), 2);
    }

    #[test]
    fn query_dependencies_invalidate_transitively() {
        let mut db = QueryEngine::new();
        db.set_input(InputKey::new("a.jet"), "alpha beta".to_string());
        let tokens = QueryKey::new("tokens", "a.jet");
        let symbols = QueryKey::new("symbols", "a.jet");

        let first = db.query(symbols.clone(), |q| {
            let n = q.query(tokens.clone(), |inner| {
                inner
                    .input_text(&InputKey::new("a.jet"))
                    .unwrap_or_default()
                    .split_whitespace()
                    .count()
            });
            format!("symbols:{n}")
        });
        assert_eq!(first, "symbols:2");

        db.set_input(InputKey::new("a.jet"), "alpha beta gamma".to_string());
        let second = db.query(symbols.clone(), |q| {
            let n = q.query(tokens.clone(), |inner| {
                inner
                    .input_text(&InputKey::new("a.jet"))
                    .unwrap_or_default()
                    .split_whitespace()
                    .count()
            });
            format!("symbols:{n}")
        });
        assert_eq!(second, "symbols:3");
        assert_eq!(db.recompute_count(&tokens), 2);
        assert_eq!(db.recompute_count(&symbols), 2);
    }

    #[test]
    fn explicit_invalidation_preserves_unrelated_queries() {
        let mut db = QueryEngine::new();
        db.set_input(InputKey::new("a.jet"), "alpha".into());
        db.set_input(InputKey::new("b.jet"), "beta".into());
        assert_eq!(word_count(&mut db, "a.jet"), 1);
        assert_eq!(word_count(&mut db, "b.jet"), 1);

        db.invalidate(&QueryKey::new("tokens", "a.jet"));
        assert_eq!(word_count(&mut db, "a.jet"), 1);
        assert_eq!(word_count(&mut db, "b.jet"), 1);
        assert_eq!(db.recompute_count(&QueryKey::new("tokens", "a.jet")), 2);
        assert_eq!(db.recompute_count(&QueryKey::new("tokens", "b.jet")), 1);
    }

    #[test]
    fn counters_and_live_memory_are_deterministic() {
        let file = FileKey::new("a.jet");
        let input = InputKey::file(file.clone());
        let query = QueryKey::for_file("tokens", file);
        let mut db = QueryEngine::new();
        db.set_input(input.clone(), "one two".into());
        assert_eq!(
            db.query(query.clone(), |q| q.input_text(&input).unwrap()),
            "one two"
        );
        let hit: String = db.query(query, |_| unreachable!("memo hit"));
        assert_eq!(hit, "one two");

        assert_eq!(
            db.stats(),
            QueryStats {
                hits: 1,
                recomputes: 1,
                live_inputs: 1,
                live_input_bytes: 7,
                live_memos: 1,
                live_query_counters: 1,
            }
        );
    }

    #[test]
    fn removing_input_reclaims_dead_memos_and_key_counters() {
        let input = InputKey::new("gone.jet");
        let query = QueryKey::new("tokens", "gone.jet");
        let mut db = QueryEngine::new();
        db.set_input(input.clone(), "gone".into());
        let _: String = db.query(query.clone(), |q| q.input_text(&input).unwrap());

        assert!(db.remove_input(&input));
        assert_eq!(db.stats().live_inputs, 0);
        assert_eq!(db.stats().live_input_bytes, 0);
        assert_eq!(db.stats().live_memos, 0);
        assert_eq!(db.stats().live_query_counters, 0);
        assert_eq!(db.stats().recomputes, 1);
        assert_eq!(db.recompute_count(&query), 0);
    }

    #[test]
    fn removing_input_reclaims_counter_after_explicit_invalidation() {
        let input = InputKey::new("gone.jet");
        let query = QueryKey::new("check", "gone.jet");
        let mut db = QueryEngine::new();
        db.set_input(input.clone(), "gone".into());
        let _: String = db.query(query.clone(), |q| q.input_text(&input).unwrap());
        db.invalidate_kind("check");

        assert_eq!(db.stats().live_memos, 0);
        assert_eq!(db.stats().live_query_counters, 1);
        assert!(db.remove_input(&input));
        assert_eq!(db.stats().live_query_counters, 0);
        assert_eq!(db.recompute_count(&query), 0);
    }

    #[test]
    fn adding_previously_missing_input_invalidates_reader() {
        let input = InputKey::new("late.jet");
        let query = QueryKey::new("tokens", "late.jet");
        let mut db = QueryEngine::new();
        let first: String = db.query(query.clone(), |q| {
            q.input_text(&input).unwrap_or_default()
        });
        assert!(first.is_empty());

        db.set_input(input.clone(), "now present".into());
        let second: String = db.query(query.clone(), |q| {
            q.input_text(&input).unwrap_or_default()
        });
        assert_eq!(second, "now present");
        assert_eq!(db.recompute_count(&query), 2);
    }

    #[test]
    fn document_churn_does_not_retain_query_history() {
        let mut db = QueryEngine::new();
        for index in 0..64 {
            let file = FileKey::new(format!("closed-{index}.jet"));
            let input = InputKey::file(file.clone());
            db.set_input(input.clone(), "fn run() {}".into());
            let _: String = db.query(QueryKey::for_file("check", file), |q| {
                q.input_text(&input).unwrap()
            });
            assert!(db.remove_input(&input));
        }
        assert_eq!(db.stats().recomputes, 64);
        assert_eq!(db.stats().live_inputs, 0);
        assert_eq!(db.stats().live_memos, 0);
        assert_eq!(db.stats().live_query_counters, 0);
    }
}
