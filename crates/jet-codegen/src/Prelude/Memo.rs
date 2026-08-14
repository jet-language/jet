// D-MEMO1=A: one function-result cache substrate shared by Cache, AOT, and the
// TIR interpreter. Engines only marshal a key, a result, and the bound into
// this implementation.

#[derive(Clone)]
struct JetLru<K, V> {
    bound: Option<usize>,
    entries: Vec<(K, V)>,
}

impl<K: PartialEq + Clone, V: Clone> JetLru<K, V> {
    fn new(bound: Option<usize>) -> Self {
        Self {
            bound,
            entries: Vec::new(),
        }
    }

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

    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Clone)]
pub struct JetMemoStats {
    pub hits: i64,
    pub misses: i64,
    pub size: i64,
    pub bound: String,
}

#[derive(Clone)]
pub struct JetMemo<K, V> {
    cache: JetLru<K, V>,
    hits: i64,
    misses: i64,
}

impl<K: PartialEq + Clone, V: Clone> JetMemo<K, V> {
    pub fn new(bound: Option<usize>) -> Self {
        Self {
            cache: JetLru::new(bound),
            hits: 0,
            misses: 0,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<V> {
        let Some(value) = self.cache.get(key) else {
            self.misses += 1;
            return None;
        };
        self.hits += 1;
        Some(value)
    }

    pub fn put(&mut self, key: K, value: V) {
        self.cache.put(key, value);
    }

    pub fn stats(&self) -> JetMemoStats {
        JetMemoStats {
            hits: self.hits,
            misses: self.misses,
            size: self.cache.len() as i64,
            bound: self
                .cache
                .bound
                .map(|bound| bound.to_string())
                .unwrap_or_else(|| "none".to_string()),
        }
    }
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
