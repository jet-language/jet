// ── core.encoding: Encode / Decode traits + blanket impls (D-SERDE1/2/4) ──────
// The built-in `@[Codable]`/`@[Encode]`/`@[Decode]` derive (D-ENC1) lowers to
// these traits. `jet_encode`/`jet_decode` are codegen-internal method names the
// user never types (they write the verbs `encode`/`decode` only in a hand-impl,
// D-SERDE2 — a later increment). Pure safe std Rust, no proc-macros (I1/I6).
#[allow(non_camel_case_types)]
pub trait user_Encode {
    fn jet_encode(&self) -> jet_std::DataTree;
}
#[allow(non_camel_case_types)]
pub trait user_Decode: Sized {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError>;
    /// D-MIGRATE4: decode this value, reporting whether it arrived as an older
    /// `@PublishedSchema` shape and was walked forward through the migration
    /// chain. The default is the zero-cost identity: no migrations declared, so
    /// decode the current shape and report `fresh`. Codegen overrides this only
    /// for a `@PublishedSchema` type that has `migration { }` blocks and a
    /// runtime decode path — every other type keeps this default, so no
    /// per-type code is emitted and the decode path is byte-for-byte unchanged.
    fn jet_decode_traced(
        tree: &jet_std::DataTree,
    ) -> Result<(Self, jet_std::MigrationStatus), jet_std::DecodeError> {
        Ok((Self::jet_decode(tree)?, jet_std::MigrationStatus::fresh()))
    }
}

impl user_Encode for i64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Int(*self)
    }
}
impl user_Encode for f64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Float(*self)
    }
}
impl user_Encode for bool {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Bool(*self)
    }
}
impl user_Encode for String {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(self.clone())
    }
}
impl user_Encode for char {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(self.to_string())
    }
}
impl user_Encode for u8 {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Int(*self as i64) }
}
impl<T: user_Encode> user_Encode for Vec<T> {
    fn jet_encode(&self) -> jet_std::DataTree {
        // D-ENC-CBOR-SURFACE1: `[U8]` carries binary identity through the shared
        // Codable tree. Text codecs already render Bytes as a number list, while
        // CBOR emits major type 2. No downcast or raw-pointer code is needed.
        if std::any::type_name::<T>() == "u8" {
            let mut bytes = Vec::with_capacity(self.len());
            for value in self {
                let jet_std::DataTree::Int(n) = value.jet_encode() else { unreachable!() };
                bytes.push(n as u8);
            }
            return jet_std::DataTree::Bytes(bytes);
        }
        jet_std::DataTree::Array(self.iter().map(|x| x.jet_encode()).collect())
    }
}
impl<T: user_Encode> user_Encode for Option<T> {
    fn jet_encode(&self) -> jet_std::DataTree {
        match self {
            Some(x) => x.jet_encode(),
            None => jet_std::DataTree::Null,
        }
    }
}
impl<V: user_Encode> user_Encode for std::collections::BTreeMap<String, V> {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), v.jet_encode()))
                .collect(),
        )
    }
}
// D-ENC-CBOR-SURFACE1: DataTree itself is Codable. Whole-value codec
// composition must not fall through to rustc after front-end acceptance.
impl user_Encode for jet_std::DataTree {
    fn jet_encode(&self) -> jet_std::DataTree { self.clone() }
}

impl user_Decode for i64 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Int(n) => Ok(*n),
            jet_std::DataTree::Float(f) if f.fract() == 0.0 => Ok(*f as i64),
            jet_std::DataTree::Text(s) => s.trim().parse::<i64>().map_err(|_| {
                jet_std::DecodeError::new(format!("expected Int, found text {:?}", s))
            }),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Int, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for f64 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Float(f) => Ok(*f),
            jet_std::DataTree::Int(n) => Ok(*n as f64),
            jet_std::DataTree::Text(s) => s.trim().parse::<f64>().map_err(|_| {
                jet_std::DecodeError::new(format!("expected Float, found text {:?}", s))
            }),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Float, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for bool {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Bool(b) => Ok(*b),
            jet_std::DataTree::Text(s) => match s.trim() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(jet_std::DecodeError::new(format!(
                    "expected Bool, found text {:?}",
                    s
                ))),
            },
            other => Err(jet_std::DecodeError::new(format!(
                "expected Bool, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for String {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Text(s) => Ok(s.clone()),
            jet_std::DataTree::Int(n) => Ok(n.to_string()),
            jet_std::DataTree::Float(f) => Ok(format!("{:?}", f)),
            jet_std::DataTree::Bool(b) => Ok(b.to_string()),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Text, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for char {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        let s = String::jet_decode(t)?;
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(jet_std::DecodeError::new(format!(
                "expected a single Char, found {:?}",
                s
            ))),
        }
    }
}
impl user_Decode for u8 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
            other => Err(jet_std::DecodeError::new(format!("expected U8, found {}", jet_std::datatree_kind(other)))),
        }
    }
}
impl<T: user_Decode> user_Decode for Vec<T> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Bytes(bytes) if std::any::type_name::<T>() == "u8" => {
                let mut out = Vec::with_capacity(bytes.len());
                for byte in bytes { out.push(T::jet_decode(&jet_std::DataTree::Int(*byte as i64))?); }
                Ok(out)
            }
            jet_std::DataTree::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    out.push(
                        T::jet_decode(item)
                            .map_err(|e| jet_std::DecodeError::under(&format!("[{}]", i), e))?,
                    );
                }
                Ok(out)
            }
            other => Err(jet_std::DecodeError::new(format!(
                "expected a list, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for jet_std::DataTree {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> { Ok(t.clone()) }
}
impl<T: user_Decode> user_Decode for Option<T> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Null => Ok(None),
            other => Ok(Some(T::jet_decode(other)?)),
        }
    }
}
impl<V: user_Decode> user_Decode for std::collections::BTreeMap<String, V> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Object(entries) => {
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in entries {
                    out.insert(
                        k.clone(),
                        V::jet_decode(v).map_err(|e| jet_std::DecodeError::under(k, e))?,
                    );
                }
                Ok(out)
            }
            other => Err(jet_std::DecodeError::new(format!(
                "expected an object, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}

// ── core.encoding: typed format verbs over Encode/Decode (D-ENC1, D-SERDE6) ────
// `to_string`/`to_string_pretty` (D-JSONVERB1) and the typed `decode<T>` route
// every format through the one DataTree model.
fn jet_enc_json_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::render_datatree_json(&v.jet_encode(), false, 0)
}
fn jet_enc_json_to_string_pretty<T: user_Encode>(v: &T) -> String {
    jet_std::render_datatree_json(&v.jet_encode(), true, 0)
}
fn jet_enc_json_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let j = jet_std::parse_json(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid JSON (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the same migration chain, silently — the
    // status is dropped. Types without migrations hit the trait default, which
    // is exactly `jet_decode` (zero cost).
    Ok(T::jet_decode_traced(&jet_std::datatree_from_json(&j))?.0)
}

// D-MIGRATE3=A: `decode_traced<T>` — same decode, wrapped in `DecodeResult` so the
// caller can ask whether/how it migrated, without `decode` itself paying for it.
fn jet_enc_json_decode_traced<T: user_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let j = jet_std::parse_json(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid JSON (line {}): {}", e.line, e.message))
    })?;
    let (value, migration) = T::jet_decode_traced(&jet_std::datatree_from_json(&j))?;
    Ok(jet_std::DecodeResult { value, migration })
}

// CSV typed decode: header row maps columns to fields by name; each data row
// becomes a DataTree::Object of Text cells, then decodes to `T`. A short row or a
// per-row decode failure is a typed `DecodeError` naming the 1-based row.
fn jet_enc_csv_decode<T: user_Decode>(text: &String) -> Result<Vec<T>, jet_std::DecodeError> {
    let rows = jet_ring_csv_parse(text).map_err(jet_std::DecodeError::new)?;
    let mut it = rows.into_iter();
    let Some(header) = it.next() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (i, row) in it.enumerate() {
        let obj: Vec<(String, jet_std::DataTree)> = header
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let cell = row.get(c).cloned().unwrap_or_default();
                (name.clone(), jet_std::DataTree::Text(cell))
            })
            .collect();
        let tree = jet_std::DataTree::Object(obj);
        // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
        out.push(
            T::jet_decode_traced(&tree)
                .map(|(v, _)| v)
                .map_err(|e| jet_std::DecodeError::under(&format!("row {}", i + 1), e))?,
        );
    }
    Ok(out)
}

trait JetDataCount {
    fn jet_data_count(&self) -> i64;
}

impl<T> JetDataCount for Vec<T> {
    fn jet_data_count(&self) -> i64 {
        self.len() as i64
    }
}

impl<T> JetDataCount for jet_std::DataTable<T> {
    fn jet_data_count(&self) -> i64 {
        self.rows.len() as i64
    }
}

impl<T> JetDataCount for jet_std::DataSeries<T> {
    fn jet_data_count(&self) -> i64 {
        self.values.len() as i64
    }
}

impl<T: Clone> JetDataCount for jet_std::DataLazyFrame<T> {
    fn jet_data_count(&self) -> i64 {
        jet_data_materialize(self).len() as i64
    }
}

fn jet_data_count<T: JetDataCount + ?Sized>(rows: &T) -> i64 {
    rows.jet_data_count()
}

fn jet_data_table<T: Clone>(rows: &Vec<T>) -> jet_std::DataTable<T> {
    jet_std::DataTable {
        rows: rows.clone(),
        missing: 0,
        plan: vec!["table".to_string()],
    }
}

fn jet_data_rows<T: Clone>(table: &jet_std::DataTable<T>) -> Vec<T> {
    table.rows.clone()
}

fn jet_data_series<T: Clone>(values: &Vec<T>) -> jet_std::DataSeries<T> {
    jet_std::DataSeries {
        values: values.clone(),
        missing: 0,
    }
}

fn jet_data_series_values<T: Clone>(series: &jet_std::DataSeries<T>) -> Vec<T> {
    series.values.clone()
}

fn jet_data_missing_count<T>(series: &jet_std::DataSeries<Option<T>>) -> i64 {
    series.missing + series.values.iter().filter(|v| v.is_none()).count() as i64
}

fn jet_data_lazy<T: Clone>(table: &jet_std::DataTable<T>) -> jet_std::DataLazyFrame<T> {
    jet_std::DataLazyFrame {
        rows: table.rows.clone(),
        missing: table.missing,
        plan: table.plan.clone(),
        operations: Vec::new(),
    }
}

fn jet_data_materialize<T: Clone>(frame: &jet_std::DataLazyFrame<T>) -> Vec<T> {
    let mut rows = frame.rows.clone();
    for operation in &frame.operations {
        match operation {
            jet_std::DataLazyOperation::Filter(pred) => {
                rows.retain(|row| pred(row.clone()));
            }
            jet_std::DataLazyOperation::SortBy(key) => {
                rows.sort_by_key(|row| key(row.clone()));
            }
        }
    }
    rows
}

fn jet_data_lazy_filter<T, F>(
    frame: &jet_std::DataLazyFrame<T>,
    pred: F,
) -> jet_std::DataLazyFrame<T>
where
    T: Clone + 'static,
    F: Fn(T) -> bool + 'static,
{
    let mut plan = frame.plan.clone();
    plan.push("filter".to_string());
    let mut operations = frame.operations.clone();
    operations.push(jet_std::DataLazyOperation::Filter(std::sync::Arc::new(pred)));
    jet_std::DataLazyFrame {
        rows: frame.rows.clone(),
        missing: frame.missing,
        plan,
        operations,
    }
}

fn jet_data_lazy_sort_by<T, F>(
    frame: &jet_std::DataLazyFrame<T>,
    key: F,
) -> jet_std::DataLazyFrame<T>
where
    T: Clone + 'static,
    F: Fn(T) -> String + 'static,
{
    let mut plan = frame.plan.clone();
    plan.push("sort_by".to_string());
    let mut operations = frame.operations.clone();
    operations.push(jet_std::DataLazyOperation::SortBy(std::sync::Arc::new(key)));
    jet_std::DataLazyFrame {
        rows: frame.rows.clone(),
        missing: frame.missing,
        plan,
        operations,
    }
}

fn jet_data_collect<T: Clone>(frame: &jet_std::DataLazyFrame<T>) -> jet_std::DataTable<T> {
    let mut plan = frame.plan.clone();
    plan.push("collect".to_string());
    jet_std::DataTable {
        rows: jet_data_materialize(frame),
        missing: frame.missing,
        plan,
    }
}

fn jet_data_plan<T>(frame: &jet_std::DataLazyFrame<T>) -> Vec<String> {
    frame.plan.clone()
}

fn jet_data_filter<T, F>(rows: &Vec<T>, pred: F) -> Vec<T>
where
    T: Clone,
    F: Fn(T) -> bool,
{
    rows.iter().cloned().filter(|row| pred(row.clone())).collect()
}

fn jet_data_sort_by<T, F>(rows: &Vec<T>, key: F) -> Vec<T>
where
    T: Clone,
    F: Fn(T) -> String,
{
    let mut out = rows.clone();
    out.sort_by_key(|row| key(row.clone()));
    out
}

fn jet_data_sum(values: &Vec<f64>) -> f64 {
    values.iter().copied().sum()
}

fn jet_data_mean(values: &Vec<f64>) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        jet_data_sum(values) / values.len() as f64
    }
}

fn jet_data_min(values: &Vec<f64>) -> f64 {
    values.iter().copied().reduce(f64::min).unwrap_or(0.0)
}

fn jet_data_max(values: &Vec<f64>) -> f64 {
    values.iter().copied().reduce(f64::max).unwrap_or(0.0)
}

fn jet_data_median(values: &Vec<f64>) -> f64 {
    jet_data_quantile(values, 0.5)
}

fn jet_data_quantile(values: &Vec<f64>, q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = q.clamp(0.0, 1.0);
    let pos = q * (sorted.len().saturating_sub(1)) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let t = pos - lo as f64;
        sorted[lo] * (1.0 - t) + sorted[hi] * t
    }
}

fn jet_data_variance(values: &Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = jet_data_mean(values);
    values
        .iter()
        .map(|v| {
            let d = *v - mean;
            d * d
        })
        .sum::<f64>()
        / values.len() as f64
}

fn jet_data_stddev(values: &Vec<f64>) -> f64 {
    jet_data_variance(values).sqrt()
}

fn jet_data_describe(values: &Vec<f64>) -> jet_std::DataSummary {
    jet_std::DataSummary {
        count: values.len() as i64,
        sum: jet_data_sum(values),
        mean: jet_data_mean(values),
        min: jet_data_min(values),
        max: jet_data_max(values),
        median: jet_data_median(values),
        variance: jet_data_variance(values),
        stddev: jet_data_stddev(values),
    }
}

fn jet_data_group_count<T, F>(rows: &Vec<T>, key: F) -> Vec<jet_std::DataGroup>
where
    T: Clone,
    F: Fn(T) -> String,
{
    let mut groups: std::collections::BTreeMap<String, (i64, f64)> =
        std::collections::BTreeMap::new();
    for row in rows.iter().cloned() {
        let k = key(row);
        let entry = groups.entry(k).or_insert((0, 0.0));
        entry.0 += 1;
    }
    groups
        .into_iter()
        .map(|(key, (count, sum))| jet_std::DataGroup {
            key,
            count,
            sum,
            mean: 0.0,
        })
        .collect()
}

fn jet_data_group_sum<T, FK, FV>(rows: &Vec<T>, key: FK, value: FV) -> Vec<jet_std::DataGroup>
where
    T: Clone,
    FK: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    let mut groups: std::collections::BTreeMap<String, (i64, f64)> =
        std::collections::BTreeMap::new();
    for row in rows.iter().cloned() {
        let k = key(row.clone());
        let v = value(row);
        let entry = groups.entry(k).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += v;
    }
    groups
        .into_iter()
        .map(|(key, (count, sum))| jet_std::DataGroup {
            key,
            count,
            sum,
            mean: if count == 0 { 0.0 } else { sum / count as f64 },
        })
        .collect()
}

fn jet_data_group_mean<T, FK, FV>(rows: &Vec<T>, key: FK, value: FV) -> Vec<jet_std::DataGroup>
where
    T: Clone,
    FK: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    jet_data_group_sum(rows, key, value)
}
