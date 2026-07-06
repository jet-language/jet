//! Demand-driven query cache for Jet front-end clients.
//!
//! This crate is intentionally std-only. Compiler/LSP-specific work stays in
//! callers; this layer only owns inputs, memoization, dependency tracking, and
//! revision invalidation.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InputKey(String);

impl InputKey {
    pub fn new(name: impl Into<String>) -> Self {
        InputKey(name.into())
    }

    pub fn name(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct QueryKey {
    kind: &'static str,
    name: String,
}

impl QueryKey {
    pub fn new(kind: &'static str, name: impl Into<String>) -> Self {
        QueryKey {
            kind,
            name: name.into(),
        }
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputSnapshot {
    pub revision: u64,
    pub text_hash: u64,
    pub len: usize,
}

struct InputCell {
    revision: u64,
    text: String,
    text_hash: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum DepKey {
    Input(InputKey),
    Query(QueryKey),
}

#[derive(Clone, Debug)]
enum Dep {
    Input(InputKey, u64),
    Query(QueryKey, u64),
}

struct MemoEntry {
    deps: Vec<Dep>,
    value: Box<dyn Any>,
    recomputes: u64,
}

#[derive(Default)]
pub struct QueryEngine {
    revision: u64,
    inputs: HashMap<InputKey, InputCell>,
    memo: HashMap<QueryKey, MemoEntry>,
    dep_stack: Vec<Vec<DepKey>>,
}

impl QueryEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn global_revision(&self) -> u64 {
        self.revision
    }

    pub fn set_input(&mut self, key: InputKey, text: String) -> InputSnapshot {
        let text_hash = hash_text(&text);
        let len = text.len();
        if let Some(cell) = self.inputs.get_mut(&key) {
            if cell.text_hash == text_hash && cell.text == text {
                return InputSnapshot {
                    revision: cell.revision,
                    text_hash: cell.text_hash,
                    len: cell.text.len(),
                };
            }
            self.revision += 1;
            cell.revision = self.revision;
            cell.text = text;
            cell.text_hash = text_hash;
            return InputSnapshot {
                revision: cell.revision,
                text_hash,
                len,
            };
        }

        self.revision += 1;
        let revision = self.revision;
        self.inputs.insert(
            key,
            InputCell {
                revision,
                text,
                text_hash,
            },
        );
        InputSnapshot {
            revision,
            text_hash,
            len,
        }
    }

    pub fn remove_input(&mut self, key: &InputKey) -> bool {
        let removed = self.inputs.remove(key).is_some();
        if removed {
            self.revision += 1;
        }
        removed
    }

    pub fn input_text(&mut self, key: &InputKey) -> Option<String> {
        self.record_dep(key);
        self.inputs.get(key).map(|cell| cell.text.clone())
    }

    pub fn input_snapshot(&self, key: &InputKey) -> Option<InputSnapshot> {
        self.inputs.get(key).map(|cell| InputSnapshot {
            revision: cell.revision,
            text_hash: cell.text_hash,
            len: cell.text.len(),
        })
    }

    pub fn query<T, F>(&mut self, key: QueryKey, compute: F) -> T
    where
        T: Clone + 'static,
        F: FnOnce(&mut QueryEngine) -> T,
    {
        self.record_query_dep(&key);
        if let Some(entry) = self.memo.get(&key) {
            if self.memo_entry_valid(entry, &mut HashSet::new()) {
                if let Some(value) = entry.value.downcast_ref::<T>() {
                    return value.clone();
                }
            }
        }

        let previous_recomputes = self
            .memo
            .remove(&key)
            .map(|entry| entry.recomputes)
            .unwrap_or(0);
        self.dep_stack.push(Vec::new());
        let value = compute(self);
        let deps = self.finish_deps();
        self.memo.insert(
            key,
            MemoEntry {
                deps,
                value: Box::new(value.clone()),
                recomputes: previous_recomputes + 1,
            },
        );
        value
    }

    pub fn recompute_count(&self, key: &QueryKey) -> u64 {
        self.memo
            .get(key)
            .map(|entry| entry.recomputes)
            .unwrap_or(0)
    }

    pub fn clear(&mut self) {
        self.memo.clear();
        self.dep_stack.clear();
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
                    if let Some(cell) = self.inputs.get(&input) {
                        out.push(Dep::Input(input, cell.revision));
                    }
                }
                DepKey::Query(query) => {
                    if let Some(entry) = self.memo.get(&query) {
                        out.push(Dep::Query(query, entry.recomputes));
                    }
                }
            }
        }
        out
    }

    fn memo_entry_valid(&self, entry: &MemoEntry, visiting: &mut HashSet<QueryKey>) -> bool {
        entry.deps.iter().all(|dep| match dep {
            Dep::Input(input, rev) => self
                .inputs
                .get(input)
                .map(|cell| cell.revision == *rev)
                .unwrap_or(false),
            Dep::Query(query, recomputes) => self
                .memo
                .get(query)
                .map(|entry| {
                    entry.recomputes == *recomputes && self.memo_key_valid(query, visiting)
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
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
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
}
