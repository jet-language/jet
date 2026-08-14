// D-MEMO1=A / D-FIELDMEMO1=A: one Prelude cache substrate shared by pure
// function results and retained computed-field results. Function entries use
// an argument key and the ratified bound; field entries use one reserved slot.
// Engines only marshal keys, results, bounds, and invalidation calls here.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

#[derive(Clone)]
struct JetMemoState<K, V> {
    bound: Option<usize>,
    entries: Vec<(Option<K>, V)>,
    hits: i64,
    misses: i64,
}

impl<K, V> JetMemoState<K, V> {
    fn new(bound: Option<usize>) -> Self {
        Self {
            bound,
            entries: Vec::new(),
            hits: 0,
            misses: 0,
        }
    }
}

#[derive(Clone)]
pub struct JetMemoStats {
    pub hits: i64,
    pub misses: i64,
    pub size: i64,
    pub bound: String,
}

pub struct JetMemo<K, V = K> {
    state: Mutex<JetMemoState<K, V>>,
}

impl<K, V> JetMemo<K, V> {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(JetMemoState::new(None)),
        }
    }

    pub fn with_bound(bound: Option<usize>) -> Self {
        Self {
            state: Mutex::new(JetMemoState::new(bound)),
        }
    }

    pub fn get_or_insert_with(&self, build: impl FnOnce() -> V) -> V
    where
        V: Clone,
    {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, value)) = state.entries.iter().find(|(key, _)| key.is_none()) {
            return value.clone();
        }
        let value = build();
        state.entries.insert(0, (None, value.clone()));
        value
    }

    pub fn invalidate(&self) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .retain(|(key, _)| key.is_some());
    }
}

impl<K: PartialEq + Clone, V: Clone> JetMemo<K, V> {
    pub fn get(&self, key: &K) -> Option<V> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(index) = state.entries.iter().position(|(stored, _)| {
            stored
                .as_ref()
                .is_some_and(|stored_key| stored_key == key)
        }) else {
            state.misses += 1;
            return None;
        };
        let (stored, value) = state.entries.remove(index);
        let out = value.clone();
        state.entries.insert(0, (stored, value));
        state.hits += 1;
        Some(out)
    }

    pub fn put(&self, key: K, value: V) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.bound == Some(0) {
            return;
        }
        if let Some(index) = state.entries.iter().position(|(stored, _)| {
            stored
                .as_ref()
                .is_some_and(|stored_key| stored_key == &key)
        }) {
            state.entries.remove(index);
        }
        state.entries.insert(0, (Some(key), value));
        if state
            .bound
            .is_some_and(|bound| state.entries.len() > bound)
        {
            state.entries.pop();
        }
    }

    pub fn stats(&self) -> JetMemoStats {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        JetMemoStats {
            hits: state.hits,
            misses: state.misses,
            size: state.entries.len() as i64,
            bound: state
                .bound
                .map(|bound| bound.to_string())
                .unwrap_or_else(|| "none".to_string()),
        }
    }
}

impl<K, V> Default for JetMemo<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone, V: Clone> Clone for JetMemo<K, V> {
    fn clone(&self) -> Self {
        Self {
            state: Mutex::new(
                self.state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            ),
        }
    }
}

impl<K, V> fmt::Debug for JetMemo<K, V> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<memo>")
    }
}

impl<K, V> PartialEq for JetMemo<K, V> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<K, V> Eq for JetMemo<K, V> {}

impl<K, V> Hash for JetMemo<K, V> {
    fn hash<H: Hasher>(&self, _state: &mut H) {}
}

pub fn jet_memo_call<K, V, F>(
    store: &std::sync::Mutex<JetMemo<K, V>>,
    key: K,
    compute: F,
) -> V
where
    K: PartialEq + Clone,
    V: Clone,
    F: FnOnce() -> V,
{
    if let Some(value) = store
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
    {
        return value;
    }
    let value = compute();
    store
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .put(key, value.clone());
    value
}
