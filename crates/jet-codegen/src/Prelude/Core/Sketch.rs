// D-APPROX1=A: one sketch kernel for every execution tier.

fn fnv1a(data: &[u8]) -> u64 {
    let mut hash = 14695981039346656037u64;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

fn fnv1a_h2(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64.wrapping_add(0xdeadbeef);
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

#[derive(Clone)]
pub(crate) struct JetHyperLogLog(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl JetHyperLogLog {
    pub(crate) fn new() -> Self {
        Self::from_registers(vec![0; 256])
    }

    pub(crate) fn from_registers(registers: Vec<u8>) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(registers)))
    }

    pub(crate) fn registers(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }

    pub(crate) fn add(&self, item: &str) {
        let hash = fnv1a(item.as_bytes());
        let index = (hash & 0xff) as usize;
        let rest = hash >> 8;
        let zeros = if rest == 0 { 57 } else { rest.leading_zeros() as u8 + 1 };
        let mut registers = self.0.lock().unwrap();
        if zeros > registers[index] {
            registers[index] = zeros;
        }
    }

    pub(crate) fn count(&self) -> i64 {
        let registers = self.0.lock().unwrap();
        let width = registers.len() as f64;
        let empty = registers.iter().filter(|&&value| value == 0).count();
        if empty > 0 {
            return (width * (width / empty as f64).ln()).round() as i64;
        }
        let sum: f64 = registers.iter().map(|&value| 2f64.powi(-(value as i32))).sum();
        let alpha = 0.7213 / (1.0 + 1.079 / width);
        (alpha * width * width / sum).round() as i64
    }
}

#[derive(Clone)]
pub(crate) struct JetTDigest(std::sync::Arc<std::sync::Mutex<Vec<(f64, f64)>>>);

impl JetTDigest {
    const DELTA: f64 = 100.0;

    pub(crate) fn new() -> Self {
        Self::from_centroids(Vec::new())
    }

    pub(crate) fn from_centroids(centroids: Vec<(f64, f64)>) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(centroids)))
    }

    pub(crate) fn centroids(&self) -> Vec<(f64, f64)> {
        self.0.lock().unwrap().clone()
    }

    pub(crate) fn add(&self, value: f64) {
        let mut centroids = self.0.lock().unwrap();
        let index = centroids.partition_point(|&(mean, _)| mean < value);
        centroids.insert(index, (value, 1.0));
        let total: f64 = centroids.iter().map(|(_, weight)| weight).sum();
        let mut merged: Vec<(f64, f64)> = Vec::with_capacity(centroids.len());
        let mut cumulative = 0.0;
        for &(mean, weight) in centroids.iter() {
            if merged.is_empty() {
                merged.push((mean, weight));
                cumulative += weight;
                continue;
            }
            let last = merged.last_mut().unwrap();
            let quantile = cumulative / total;
            let limit = 4.0 * total * quantile * (1.0 - quantile) / Self::DELTA;
            if last.1 + weight <= limit.max(1.0) {
                let new_weight = last.1 + weight;
                last.0 = (last.0 * last.1 + mean * weight) / new_weight;
                last.1 = new_weight;
            } else {
                merged.push((mean, weight));
                cumulative += weight;
            }
        }
        *centroids = merged;
    }

    pub(crate) fn quantile(&self, quantile: f64) -> f64 {
        let centroids = self.0.lock().unwrap();
        if centroids.is_empty() {
            return 0.0;
        }
        let total: f64 = centroids.iter().map(|(_, weight)| weight).sum();
        let target = quantile * total;
        let mut cumulative = 0.0;
        for &(mean, weight) in centroids.iter() {
            cumulative += weight;
            if cumulative >= target {
                return mean;
            }
        }
        centroids.last().unwrap().0
    }
}

pub(crate) const JET_CMS_COLS: usize = 256;

#[derive(Clone)]
pub(crate) struct JetCountMinSketch(
    std::sync::Arc<std::sync::Mutex<[[u32; JET_CMS_COLS]; 4]>>,
);

impl JetCountMinSketch {
    pub(crate) fn new() -> Self {
        Self::from_rows([[0; JET_CMS_COLS]; 4])
    }

    pub(crate) fn from_rows(rows: [[u32; JET_CMS_COLS]; 4]) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(rows)))
    }

    pub(crate) fn rows(&self) -> [[u32; JET_CMS_COLS]; 4] {
        *self.0.lock().unwrap()
    }

    pub(crate) fn add(&self, key: &str) {
        let bytes = key.as_bytes();
        let first = fnv1a(bytes);
        let second = fnv1a_h2(bytes);
        let mut rows = self.0.lock().unwrap();
        for row in 0..4 {
            let column = ((first.wrapping_add(second.wrapping_mul(row as u64 + 1))) & 0xff) as usize;
            rows[row][column] = rows[row][column].saturating_add(1);
        }
    }

    pub(crate) fn count(&self, key: &str) -> i64 {
        let bytes = key.as_bytes();
        let first = fnv1a(bytes);
        let second = fnv1a_h2(bytes);
        let rows = self.0.lock().unwrap();
        (0..4)
            .map(|row| {
                let column = ((first.wrapping_add(second.wrapping_mul(row as u64 + 1))) & 0xff) as usize;
                rows[row][column]
            })
            .min()
            .unwrap() as i64
    }
}

#[derive(Clone)]
pub(crate) struct JetReservoirSampler(std::sync::Arc<std::sync::Mutex<JetReservoirInner>>);

#[derive(Clone)]
struct JetReservoirInner {
    capacity: usize,
    reservoir: Vec<String>,
    count: u64,
    rng: u64,
}

impl JetReservoirSampler {
    pub(crate) fn new(capacity: i64) -> Self {
        Self::from_parts(capacity.max(1) as usize, Vec::new(), 0, 0xdeadbeef_cafebabe)
    }

    pub(crate) fn from_parts(
        capacity: usize,
        reservoir: Vec<String>,
        count: u64,
        rng: u64,
    ) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(JetReservoirInner {
            capacity,
            reservoir,
            count,
            rng,
        })))
    }

    pub(crate) fn parts(&self) -> (usize, Vec<String>, u64, u64) {
        let inner = self.0.lock().unwrap();
        (inner.capacity, inner.reservoir.clone(), inner.count, inner.rng)
    }

    pub(crate) fn add(&self, item: String) {
        let mut inner = self.0.lock().unwrap();
        inner.count += 1;
        if inner.reservoir.len() < inner.capacity {
            inner.reservoir.push(item);
            return;
        }
        let mut state = inner.rng;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        inner.rng = state;
        let index = (state % inner.count) as usize;
        if index < inner.capacity {
            inner.reservoir[index] = item;
        }
    }

    pub(crate) fn sample(&self) -> Vec<String> {
        self.0.lock().unwrap().reservoir.clone()
    }
}
