// D-MEMO1=A / D-FIELDMEMO1=A: one Prelude cache substrate shared by pure
// function results and retained computed-field results. Function entries use
// an argument key and the ratified bound; field entries use one reserved slot.
// Engines only marshal keys, results, bounds, and invalidation calls here.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Mutex as JetMemoMutex;

#[derive(Clone)]
struct JetLru<K, V> {
    bound: Option<usize>,
    entries: Vec<(K, V)>,
}

impl<K, V> JetLru<K, V> {
    fn new(bound: Option<usize>) -> Self {
        Self {
            bound,
            entries: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

impl<K: PartialEq + Clone, V: Clone> JetLru<K, V> {
    fn put(&mut self, key: K, value: V) -> Option<V> {
        if self.bound == Some(0) {
            return None;
        }
        let displaced = self
            .entries
            .iter()
            .position(|(stored, _)| stored == &key)
            .map(|index| self.entries.remove(index).1);
        self.entries.insert(0, (key, value));
        if self
            .bound
            .is_some_and(|bound| self.entries.len() > bound)
        {
            self.entries.pop();
        }
        displaced
    }

    fn get(&mut self, key: &K) -> Option<V> {
        let index = self.entries.iter().position(|(stored, _)| stored == key)?;
        let (stored, value) = self.entries.remove(index);
        let out = value.clone();
        self.entries.insert(0, (stored, value));
        Some(out)
    }
}

#[derive(Clone)]
struct JetMemoState<K, V> {
    cache: JetLru<Option<K>, V>,
    hits: i64,
    misses: i64,
}

impl<K, V> JetMemoState<K, V> {
    fn new(bound: Option<usize>) -> Self {
        Self {
            cache: JetLru::new(bound),
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
    state: JetMemoMutex<JetMemoState<K, V>>,
}

impl<K, V> JetMemo<K, V> {
    pub fn new() -> Self {
        Self {
            state: JetMemoMutex::new(JetMemoState::new(None)),
        }
    }

    pub fn with_bound(bound: Option<usize>) -> Self {
        Self {
            state: JetMemoMutex::new(JetMemoState::new(bound)),
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
        if let Some((_, value)) = state
            .cache
            .entries
            .iter()
            .find(|(key, _)| key.is_none())
        {
            return value.clone();
        }
        let value = build();
        state.cache.entries.insert(0, (None, value.clone()));
        value
    }

    pub fn invalidate(&self) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .cache
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
        let Some(value) = state.cache.get(&Some(key.clone())) else {
            state.misses += 1;
            return None;
        };
        state.hits += 1;
        Some(value)
    }

    pub fn put(&self, key: K, value: V) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.cache.put(Some(key), value);
    }

    pub fn stats(&self) -> JetMemoStats {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        JetMemoStats {
            hits: state.hits,
            misses: state.misses,
            size: state.cache.len() as i64,
            bound: state
                .cache
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
            state: JetMemoMutex::new(
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
