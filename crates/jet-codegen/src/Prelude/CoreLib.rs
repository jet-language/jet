mod jet_std {
    #[derive(Clone, Debug, PartialEq)]
    pub enum IoError {
        NotFound { path: String },
        PermissionDenied { path: String },
        Other { message: String },
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Utf8Error {
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct ProcessResult {
        pub code: i64,
        pub output: String,
        pub errors: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DirEntry {
        pub name: String,
        pub path: String,
        pub is_dir: bool,
    }

    #[derive(Clone, Debug)]
    pub struct Stopwatch {
        pub start: std::time::Instant,
    }

    // D-DET1: deterministic injected Clock capability. `now` is the current value
    // in ms (starts at the caller's seed); `tick(ms)` advances it. No wall-clock
    // read — reproducible by construction.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Clock {
        pub now: i64,
    }

    // D-DET1: deterministic injected Rng capability. A SplitMix64 state stream
    // (std-only, no external crate — I6). The same seed yields the same draws on
    // every machine.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Rng {
        pub state: u64,
    }

    // D-DET-CAPAPI: a deterministic span of milliseconds. Minted by `time.ms(n)` /
    // `time.secs(n)` (pure value constructors). The injected `Clock` advances by one
    // with `clock.wait(d)`; read it back with `duration.millis()`. std-only (I6).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Duration {
        pub ms: i64,
    }

    // D-BIGINT1: arbitrary-precision integer (std-only limb arithmetic).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetBigInt {
        negative: bool,
        limbs: Vec<u32>, // little-endian base 10^9
    }

    const BI_BASE: u64 = 1_000_000_000;

    impl JetBigInt {
        pub fn from_int(n: i64) -> Self {
            if n == 0 {
                return JetBigInt { negative: false, limbs: vec![0] };
            }
            let negative = n < 0;
            let mut v = if negative { (n as i128).wrapping_neg() as u64 } else { n as u64 };
            let mut limbs = Vec::new();
            while v > 0 {
                limbs.push((v % BI_BASE) as u32);
                v /= BI_BASE;
            }
            JetBigInt { negative, limbs }
        }

        pub fn from_str(s: &str) -> Result<Self, String> {
            let t = s.trim();
            if t.is_empty() {
                return Err("empty BigInt string".to_string());
            }
            let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = t.strip_prefix('+') {
                (false, rest)
            } else {
                (false, t)
            };
            if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("invalid BigInt string `{s}`"));
            }
            let mut acc = JetBigInt { negative: false, limbs: vec![0] };
            for ch in body.chars() {
                let digit = ch.to_digit(10).unwrap() as u32;
                acc = acc.mul_small(10).add_small(digit);
            }
            acc.negative = negative && !(acc.limbs.len() == 1 && acc.limbs[0] == 0);
            Ok(acc)
        }

        fn normalize(mut self) -> Self {
            while self.limbs.len() > 1 && *self.limbs.last().unwrap() == 0 {
                self.limbs.pop();
            }
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                self.negative = false;
            }
            self
        }

        fn mul_small(&self, m: u32) -> Self {
            let mut carry = 0u64;
            let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
            for &limb in &self.limbs {
                let prod = limb as u64 * m as u64 + carry;
                limbs.push((prod % BI_BASE) as u32);
                carry = prod / BI_BASE;
            }
            if carry > 0 {
                limbs.push(carry as u32);
            }
            JetBigInt { negative: self.negative, limbs }.normalize()
        }

        fn add_small(&self, n: u32) -> Self {
            self.add(&JetBigInt::from_int(n as i64))
        }

        pub fn add(&self, other: &JetBigInt) -> JetBigInt {
            if self.negative == other.negative {
                let mut carry = 0u64;
                let len = self.limbs.len().max(other.limbs.len());
                let mut limbs = Vec::with_capacity(len + 1);
                for i in 0..len {
                    let a = *self.limbs.get(i).unwrap_or(&0) as u64;
                    let b = *other.limbs.get(i).unwrap_or(&0) as u64;
                    let sum = a + b + carry;
                    limbs.push((sum % BI_BASE) as u32);
                    carry = sum / BI_BASE;
                }
                if carry > 0 {
                    limbs.push(carry as u32);
                }
                JetBigInt { negative: self.negative, limbs }.normalize()
            } else {
                let cmp = self.cmp_abs(other);
                if cmp == 0 {
                    JetBigInt::from_int(0)
                } else if cmp > 0 {
                    self.sub_abs(other).with_sign(self.negative)
                } else {
                    other.sub_abs(self).with_sign(other.negative)
                }
            }
        }

        fn with_sign(self, negative: bool) -> Self {
            JetBigInt { negative, limbs: self.limbs }
        }

        pub fn sub(&self, other: &JetBigInt) -> JetBigInt {
            let mut neg_other = other.clone();
            neg_other.negative = !neg_other.negative;
            self.add(&neg_other)
        }

        fn sub_abs(&self, other: &JetBigInt) -> JetBigInt {
            let mut borrow = 0i64;
            let len = self.limbs.len();
            let mut limbs = Vec::with_capacity(len);
            for i in 0..len {
                let a = self.limbs[i] as i64;
                let b = *other.limbs.get(i).unwrap_or(&0) as i64;
                let mut cur = a - b - borrow;
                if cur < 0 {
                    cur += BI_BASE as i64;
                    borrow = 1;
                } else {
                    borrow = 0;
                }
                limbs.push(cur as u32);
            }
            JetBigInt { negative: false, limbs }.normalize()
        }

        fn cmp_abs(&self, other: &JetBigInt) -> i8 {
            match self.limbs.len().cmp(&other.limbs.len()) {
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => {
                    for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
                        match a.cmp(b) {
                            std::cmp::Ordering::Greater => return 1,
                            std::cmp::Ordering::Less => return -1,
                            std::cmp::Ordering::Equal => {}
                        }
                    }
                    0
                }
            }
        }

        pub fn mul(&self, other: &JetBigInt) -> JetBigInt {
            let mut out = JetBigInt::from_int(0);
            for (i, &limb) in other.limbs.iter().enumerate() {
                if limb == 0 {
                    continue;
                }
                let mut part = self.mul_small(limb);
                for _ in 0..i {
                    part = part.mul_small(BI_BASE as u32);
                }
                out = out.add(&part);
            }
            JetBigInt {
                negative: self.negative != other.negative,
                limbs: out.limbs,
            }
            .normalize()
        }

        pub fn neg(&self) -> JetBigInt {
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                self.clone()
            } else {
                JetBigInt { negative: !self.negative, limbs: self.limbs.clone() }
            }
        }

        pub fn to_string_rep(&self) -> String {
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                return "0".to_string();
            }
            let mut s = String::new();
            let top = *self.limbs.last().unwrap();
            s.push_str(&top.to_string());
            for &limb in self.limbs.iter().rev().skip(1) {
                s.push_str(&format!("{:09}", limb));
            }
            if self.negative {
                format!("-{s}")
            } else {
                s
            }
        }
    }

    impl super::JetShow for JetBigInt {
        fn jet_show(&self) -> String { self.to_string_rep() }
    }

    // D-DECIMAL1: exact base-10 decimal (scaled integer + scale).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetDecimal {
        negative: bool,
        digits: Vec<u8>, // big-endian mantissa digits 0-9, no dot
        scale: u32,
    }

    impl JetDecimal {
        pub fn from_str(s: &str) -> Result<Self, String> {
            let t = s.trim();
            if t.is_empty() {
                return Err("empty Decimal string".to_string());
            }
            let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = t.strip_prefix('+') {
                (false, rest)
            } else {
                (false, t)
            };
            let parts: Vec<&str> = body.split('.').collect();
            if parts.len() > 2 {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            let (int_part, frac_part) = (parts[0], parts.get(1).copied().unwrap_or(""));
            if int_part.is_empty() && frac_part.is_empty() {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            if !int_part.chars().all(|c| c.is_ascii_digit())
                || !frac_part.chars().all(|c| c.is_ascii_digit())
            {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            let mut digits: Vec<u8> = int_part
                .chars()
                .chain(frac_part.chars())
                .map(|c| (c as u8 - b'0'))
                .collect();
            while digits.len() > 1 && digits.first() == Some(&0) {
                digits.remove(0);
            }
            if digits.is_empty() {
                digits.push(0);
            }
            let scale = frac_part.len() as u32;
            Ok(JetDecimal { negative, digits, scale }.normalize())
        }

        fn normalize(mut self) -> Self {
            while self.digits.len() > 1 && self.digits.last() == Some(&0) {
                self.digits.pop();
            }
            if self.digits == [0] {
                self.negative = false;
            }
            self
        }

        fn align_scales(a: &JetDecimal, b: &JetDecimal) -> (JetDecimal, JetDecimal) {
            let scale = a.scale.max(b.scale);
            let mut left = a.clone();
            let mut right = b.clone();
            while left.scale < scale {
                left.digits.push(0);
                left.scale += 1;
            }
            while right.scale < scale {
                right.digits.push(0);
                right.scale += 1;
            }
            (left, right)
        }

        fn to_bigint(&self) -> JetBigInt {
            let mut s = String::new();
            for &d in &self.digits {
                s.push((b'0' + d) as char);
            }
            JetBigInt::from_str(&s).unwrap()
        }

        fn from_bigint(v: JetBigInt, scale: u32, negative: bool) -> JetDecimal {
            let s = v.to_string_rep();
            let body = if s.starts_with('-') { &s[1..] } else { &s };
            let digits: Vec<u8> = body.bytes().map(|b| b - b'0').collect();
            JetDecimal { negative, digits, scale }.normalize()
        }

        pub fn add(&self, other: &JetDecimal) -> JetDecimal {
            let (a, b) = JetDecimal::align_scales(self, other);
            let sum = a.to_bigint().add(&b.to_bigint());
            let negative = if a.negative == b.negative {
                a.negative
            } else if a.to_bigint().cmp_abs(&b.to_bigint()) >= 0 {
                a.negative
            } else {
                b.negative
            };
            if a.negative == b.negative {
                JetDecimal::from_bigint(sum, a.scale, negative)
            } else {
                let diff = if a.to_bigint().cmp_abs(&b.to_bigint()) >= 0 {
                    a.to_bigint().sub_abs(&b.to_bigint())
                } else {
                    b.to_bigint().sub_abs(&a.to_bigint())
                };
                JetDecimal::from_bigint(diff, a.scale, negative)
            }
        }

        pub fn sub(&self, other: &JetDecimal) -> JetDecimal {
            let mut neg = other.clone();
            neg.negative = !neg.negative;
            self.add(&neg)
        }

        pub fn mul(&self, other: &JetDecimal) -> JetDecimal {
            let prod = self.to_bigint().mul(&other.to_bigint());
            JetDecimal::from_bigint(
                prod,
                self.scale + other.scale,
                self.negative != other.negative,
            )
        }

        pub fn to_string_rep(&self) -> String {
            if self.digits == [0] {
                return if self.scale == 0 {
                    "0".to_string()
                } else {
                    format!("0.{}", "0".repeat(self.scale as usize))
                };
            }
            let mut int_digits = self.digits.clone();
            let frac_len = self.scale as usize;
            let sign = if self.negative { "-" } else { "" };
            if frac_len == 0 {
                let s: String = int_digits.iter().map(|d| (b'0' + *d) as char).collect();
                return format!("{sign}{s}");
            }
            if int_digits.len() <= frac_len {
                let pad = frac_len - int_digits.len() + 1;
                int_digits.splice(0..0, vec![0; pad]);
            }
            let split = int_digits.len() - frac_len;
            let (whole, frac) = int_digits.split_at(split);
            let w: String = whole.iter().map(|d| (b'0' + *d) as char).collect();
            let f: String = frac.iter().map(|d| (b'0' + *d) as char).collect();
            format!("{sign}{w}.{f}")
        }
    }

    impl super::JetShow for JetDecimal {
        fn jet_show(&self) -> String { self.to_string_rep() }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JsonError {
        pub line: i64,
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Json {
        Null,
        Boolean(bool),
        Number(f64),
        Text(String),
        Array(Vec<Json>),
        Object(std::collections::BTreeMap<String, Json>),
    }

    impl super::JetShow for IoError {
        fn jet_show(&self) -> String { format!("{:?}", self) }
    }
    impl super::JetShow for Utf8Error {
        fn jet_show(&self) -> String { self.message.clone() }
    }
    impl super::JetShow for ProcessResult {
        fn jet_show(&self) -> String { format!("{:?}", self) }
    }
    impl super::JetShow for DirEntry {
        fn jet_show(&self) -> String {
            format!("DirEntry {{ name: {:?}, path: {:?}, is_dir: {} }}", self.name, self.path, self.is_dir)
        }
    }
    impl super::JetShow for Stopwatch {
        fn jet_show(&self) -> String { format!("{:?}", self.start) }
    }
    impl super::JetShow for Clock {
        fn jet_show(&self) -> String { format!("Clock {{ now: {} }}", self.now) }
    }
    impl super::JetShow for Rng {
        fn jet_show(&self) -> String { format!("Rng {{ .. }}") }
    }
    impl super::JetShow for Duration {
        fn jet_show(&self) -> String { format!("{}ms", self.ms) }
    }
    impl super::JetShow for JsonError {
        fn jet_show(&self) -> String { format!("line {}: {}", self.line, self.message) }
    }
    impl super::JetShow for Json {
        fn jet_show(&self) -> String { render_json(self, false, 0) }
    }

    // D-SERDE-ACCESS=B: accessor methods on Json (= Data).
    impl Json {
        pub fn field(&self, name: &str) -> Result<Json, String> {
            match self {
                Json::Object(map) => map
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("field `{}` not found", name)),
                _ => Err(format!("expected object, got {}", render_json(self, false, 0))),
            }
        }
        pub fn at(&self, i: i64) -> Result<Json, String> {
            match self {
                Json::Array(items) => {
                    let idx = if i < 0 {
                        items.len().wrapping_sub((-i) as usize)
                    } else {
                        i as usize
                    };
                    items.get(idx).cloned().ok_or_else(|| format!("index {} out of bounds (len {})", i, items.len()))
                }
                _ => Err(format!("expected array, got {}", render_json(self, false, 0))),
            }
        }
        pub fn int(&self) -> Result<i64, String> {
            match self {
                Json::Number(f) => {
                    let n = *f as i64;
                    if (n as f64 - f).abs() < 0.5 { Ok(n) } else { Err(format!("{} is not an integer", f)) }
                }
                _ => Err(format!("expected number, got {}", render_json(self, false, 0))),
            }
        }
        pub fn text(&self) -> Result<String, String> {
            match self {
                Json::Text(s) => Ok(s.clone()),
                _ => Err(format!("expected text, got {}", render_json(self, false, 0))),
            }
        }
        pub fn bool(&self) -> Result<bool, String> {
            match self {
                Json::Boolean(b) => Ok(*b),
                _ => Err(format!("expected bool, got {}", render_json(self, false, 0))),
            }
        }
        pub fn float(&self) -> Result<f64, String> {
            match self {
                Json::Number(f) => Ok(*f),
                _ => Err(format!("expected number, got {}", render_json(self, false, 0))),
            }
        }
    }

    // ── core.db: the tagged SQL parameter/column value (D-DBDRIVER1) ───────────
    // `DbValue` mirrors `Json`'s dynamic-value construction mechanism
    // (`DbValue.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)` / `.Null`) but is
    // SQL-shaped: `Int` keeps the full 64-bit width SQLite integers carry (never
    // routed through `f64`, which would lose precision above 2^53). A `Row` is
    // `Map<String, DbValue>` — the built-in `Map` type already gives `.get`/
    // `.keys`/`.values`, so no separate nominal `Row` type is needed (I8).
    #[derive(Clone, Debug, PartialEq)]
    pub enum DbValue {
        Null,
        Int(i64),
        Float(f64),
        Text(String),
        Bool(bool),
    }

    impl super::JetShow for DbValue {
        fn jet_show(&self) -> String {
            render_db_value(self)
        }
    }

    fn render_db_value(v: &DbValue) -> String {
        match v {
            DbValue::Null => "null".to_string(),
            DbValue::Int(n) => n.to_string(),
            DbValue::Float(f) => f.to_string(),
            DbValue::Text(s) => s.clone(),
            DbValue::Bool(b) => b.to_string(),
        }
    }

    impl DbValue {
        pub fn is_null(&self) -> bool {
            matches!(self, DbValue::Null)
        }
        pub fn int(&self) -> Result<i64, String> {
            match self {
                DbValue::Int(n) => Ok(*n),
                _ => Err(format!("expected an int, got {}", render_db_value(self))),
            }
        }
        pub fn float(&self) -> Result<f64, String> {
            match self {
                DbValue::Float(f) => Ok(*f),
                DbValue::Int(n) => Ok(*n as f64),
                _ => Err(format!("expected a float, got {}", render_db_value(self))),
            }
        }
        pub fn text(&self) -> Result<String, String> {
            match self {
                DbValue::Text(s) => Ok(s.clone()),
                _ => Err(format!("expected text, got {}", render_db_value(self))),
            }
        }
        pub fn bool(&self) -> Result<bool, String> {
            match self {
                DbValue::Bool(b) => Ok(*b),
                _ => Err(format!("expected a bool, got {}", render_db_value(self))),
            }
        }
    }

    /// D-DBDRIVER1: `.query`/`.query_one`/`.execute` fail with a `DbError`
    /// carrying the driver's message (SQLite's error text) — never the raw SQL.
    #[derive(Clone, Debug, PartialEq)]
    pub struct DbError {
        pub message: String,
    }

    impl super::JetShow for DbError {
        fn jet_show(&self) -> String {
            self.message.clone()
        }
    }

    // ── core.db wire codec ──────────────────────────────────────────────────────
    // The FFI bridge crate (built only when a program uses `jet.db`, Source/FFI.rs)
    // and this always-compiled prelude are two independently built Rust crates —
    // they can't share types, so bind params and result rows cross that boundary as
    // plain `String`s in a small tagged-length wire format (mirrored byte-for-byte
    // in Source/Prelude/Db.rs). A value is `<tag><decimal-length>:<payload-bytes>`;
    // a list is a decimal item count + `:` + that many back-to-back items. Every
    // length is a byte count, so arbitrary text — including an "injection-looking"
    // literal — round-trips exactly with no escaping.
    fn db_encode_tagged(tag: char, payload: &str) -> String {
        format!("{tag}{}:{payload}", payload.len())
    }

    pub fn jet_db_encode_params(params: &Vec<DbValue>) -> String {
        let mut out = String::new();
        out.push_str(&params.len().to_string());
        out.push(':');
        for p in params {
            out.push_str(&match p {
                DbValue::Null => db_encode_tagged('N', ""),
                DbValue::Int(n) => db_encode_tagged('I', &n.to_string()),
                DbValue::Float(f) => db_encode_tagged('F', &f.to_string()),
                DbValue::Text(s) => db_encode_tagged('T', s),
                DbValue::Bool(b) => db_encode_tagged('B', if *b { "1" } else { "0" }),
            });
        }
        out
    }

    fn db_read_tagged(bytes: &[u8], pos: &mut usize) -> Option<(char, String)> {
        let tag = *bytes.get(*pos)? as char;
        *pos += 1;
        let len_start = *pos;
        while *bytes.get(*pos)? != b':' {
            *pos += 1;
        }
        let len: usize = std::str::from_utf8(&bytes[len_start..*pos]).ok()?.parse().ok()?;
        *pos += 1; // skip ':'
        let payload = std::str::from_utf8(bytes.get(*pos..*pos + len)?).ok()?.to_string();
        *pos += len;
        Some((tag, payload))
    }

    fn db_decode_value(tag: char, payload: &str) -> DbValue {
        match tag {
            'I' => DbValue::Int(payload.parse().unwrap_or(0)),
            'F' => DbValue::Float(payload.parse().unwrap_or(0.0)),
            'T' => DbValue::Text(payload.to_string()),
            'B' => DbValue::Bool(payload == "1"),
            _ => DbValue::Null,
        }
    }

    /// Decode the `"O:" + rows`/`"E:" + message` wire produced by `jet_db_query`.
    pub fn jet_db_decode_query_result(
        wire: &str,
    ) -> Result<Vec<std::collections::BTreeMap<String, DbValue>>, DbError> {
        let Some(body) = wire.strip_prefix("O:") else {
            let msg = wire.strip_prefix("E:").unwrap_or(wire);
            return Err(DbError { message: msg.to_string() });
        };
        let bytes = body.as_bytes();
        let mut pos = 0usize;
        let Some(colon) = bytes.iter().position(|b| *b == b':') else {
            return Ok(Vec::new());
        };
        let row_count: usize = std::str::from_utf8(&bytes[..colon]).unwrap_or("0").parse().unwrap_or(0);
        pos = colon + 1;
        let mut rows = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            let Some(col_colon) = bytes[pos..].iter().position(|b| *b == b':') else { break };
            let col_count: usize =
                std::str::from_utf8(&bytes[pos..pos + col_colon]).unwrap_or("0").parse().unwrap_or(0);
            pos += col_colon + 1;
            let mut row = std::collections::BTreeMap::new();
            for _ in 0..col_count {
                let Some((_, name)) = db_read_tagged(bytes, &mut pos) else { break };
                let Some((vtag, vpayload)) = db_read_tagged(bytes, &mut pos) else { break };
                row.insert(name, db_decode_value(vtag, &vpayload));
            }
            rows.push(row);
        }
        Ok(rows)
    }

    /// Decode the `"O:" + count`/`"E:" + message` wire produced by `jet_db_execute`.
    pub fn jet_db_decode_execute_result(wire: &str) -> Result<i64, DbError> {
        if let Some(n) = wire.strip_prefix("O:") {
            return Ok(n.parse().unwrap_or(0));
        }
        let msg = wire.strip_prefix("E:").unwrap_or(wire);
        Err(DbError { message: msg.to_string() })
    }

    // ── core.encoding: format-agnostic value tree (D-SERDE2 = A) ───────────────
    // The one tree every format adapter speaks. The built-in `@[Codable]` derive
    // (D-ENC1) lowers `encode`/`decode` to walks over this; each adapter turns it
    // into / parses it from wire text. Distinct from the dynamic `Json` enum:
    // `DataTree` preserves field order (ordered `Object`) and keeps Int vs Float.
    #[derive(Clone, Debug, PartialEq)]
    pub enum DataTree {
        Null,
        Bool(bool),
        Int(i64),
        Float(f64),
        Text(String),
        Bytes(Vec<u8>),
        Array(Vec<DataTree>),
        Object(Vec<(String, DataTree)>),
    }

    // D-SERDE2 = A: the decode-side error carries a field path (`order.items[2]`)
    // and a plain reason. Encode is infallible, so no `EncodeError` is minted (I8).
    #[derive(Clone, Debug, PartialEq)]
    pub struct DecodeError {
        pub path: String,
        pub reason: String,
    }

    impl DecodeError {
        pub fn new(reason: impl Into<String>) -> DecodeError {
            DecodeError { path: String::new(), reason: reason.into() }
        }
        // Prefix a child error with the field/index segment it occurred under.
        pub fn under(seg: &str, mut e: DecodeError) -> DecodeError {
            e.path = if e.path.is_empty() {
                seg.to_string()
            } else if e.path.starts_with('[') {
                format!("{}{}", seg, e.path)
            } else {
                format!("{}.{}", seg, e.path)
            };
            e
        }
    }

    impl super::JetShow for DataTree {
        fn jet_show(&self) -> String { render_datatree_json(self, false, 0) }
    }

    // D-MIGRATE3=A: decode-time migration transparency. `decode_traced<T>` sits
    // beside `decode<T>` on every codec that shares this decode machinery;
    // `decode` itself is byte-for-byte unchanged (I8, zero cost for callers not
    // asking). `.migrated` is false and `.from`/`.steps` are empty both for
    // fresh data and for any non-`@PublishedSchema` type; today that is the
    // only case there is to report — `@PublishedSchema` + `migration { }` is a
    // sema-only intent check (E0910) with no runtime data conversion yet (see
    // `docs/spec/spec.md`'s migrations section / SchemaMigration.rs), so a real
    // record read through an old shape still needs the shape to satisfy the
    // current struct exactly. `MigrationStatus` reports what actually happened
    // at decode time, honestly, and starts reporting migrations the moment
    // that runtime engine lands, with no call-site change.
    #[derive(Clone, Debug, PartialEq)]
    pub struct MigrationStatus {
        pub migrated: bool,
        pub from: String,
        pub steps: Vec<String>,
    }

    impl MigrationStatus {
        pub fn fresh() -> MigrationStatus {
            MigrationStatus { migrated: false, from: String::new(), steps: Vec::new() }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DecodeResult<T> {
        pub value: T,
        pub migration: MigrationStatus,
    }

    // D-SERDE-ACCESS=B: dynamic accessor methods on DataTree.
    impl DataTree {
        pub fn field(&self, name: &str) -> Result<DataTree, String> {
            match self {
                DataTree::Object(pairs) => pairs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| format!("field `{}` not found", name)),
                _ => Err(format!("expected object, got {}", render_datatree_json(self, false, 0))),
            }
        }
        pub fn at(&self, i: i64) -> Result<DataTree, String> {
            match self {
                DataTree::Array(items) => {
                    let idx = if i < 0 {
                        items.len().wrapping_sub((-i) as usize)
                    } else {
                        i as usize
                    };
                    items.get(idx).cloned().ok_or_else(|| format!("index {} out of bounds (len {})", i, items.len()))
                }
                _ => Err(format!("expected array, got {}", render_datatree_json(self, false, 0))),
            }
        }
        pub fn int(&self) -> Result<i64, String> {
            match self {
                DataTree::Int(n) => Ok(*n),
                _ => Err(format!("expected int, got {}", render_datatree_json(self, false, 0))),
            }
        }
        pub fn text(&self) -> Result<String, String> {
            match self {
                DataTree::Text(s) => Ok(s.clone()),
                _ => Err(format!("expected text, got {}", render_datatree_json(self, false, 0))),
            }
        }
        pub fn bool(&self) -> Result<bool, String> {
            match self {
                DataTree::Bool(b) => Ok(*b),
                _ => Err(format!("expected bool, got {}", render_datatree_json(self, false, 0))),
            }
        }
        pub fn float(&self) -> Result<f64, String> {
            match self {
                DataTree::Float(f) => Ok(*f),
                DataTree::Int(n) => Ok(*n as f64),
                _ => Err(format!("expected float, got {}", render_datatree_json(self, false, 0))),
            }
        }
    }

    impl super::JetShow for DecodeError {
        fn jet_show(&self) -> String {
            if self.path.is_empty() {
                self.reason.clone()
            } else {
                format!("at `{}`: {}", self.path, self.reason)
            }
        }
    }

    // ── D-SIMD2 / D-LINALG1: built-in math value types ───────────────────────────
    // SIMD lanes + linear-algebra vectors/matrices. The pinned stable rustc has no
    // `std::simd` (portable_simd is unstable), so lane types are a SCALAR-ARRAY
    // fallback: a `[f32; 4]` / `[f64; 2]` newtype with element-wise ops. This is
    // correct and memory-safe by construction (I1) — no intrinsics, no feature gate,
    // no `un`+`safe`. A `std::simd` backend can replace these structs later behind
    // the same surface without touching generated code. Linalg types are column-major
    // F64 arrays. All ops return fresh values (value semantics); `Copy` for ergonomics.

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct F32x4(pub [f32; 4]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct F64x2(pub [f64; 2]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Vec2(pub [f64; 2]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Vec3(pub [f64; 3]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Vec4(pub [f64; 4]);
    // Column-major: element (row r, col c) is `.0[c * N + r]`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Mat3(pub [f64; 9]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Mat4(pub [f64; 16]);

    macro_rules! jet_lane_ops {
        ($T:ident, $E:ty, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..$N { r[i] = self.0[i] + o.0[i]; } $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..$N { r[i] = self.0[i] - o.0[i]; } $T(r)
                }
            }
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..$N { r[i] = self.0[i] * o.0[i]; } $T(r)
                }
            }
            impl std::ops::Div for $T {
                type Output = $T;
                fn div(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..$N { r[i] = self.0[i] / o.0[i]; } $T(r)
                }
            }
        };
    }
    jet_lane_ops!(F32x4, f32, 4);
    jet_lane_ops!(F64x2, f64, 2);

    macro_rules! jet_vec_ops {
        ($T:ident, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..$N { r[i] = self.0[i] + o.0[i]; } $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..$N { r[i] = self.0[i] - o.0[i]; } $T(r)
                }
            }
            // `v * w` is element-wise (Hadamard); the dot/cross products are methods.
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..$N { r[i] = self.0[i] * o.0[i]; } $T(r)
                }
            }
        };
    }
    jet_vec_ops!(Vec2, 2);
    jet_vec_ops!(Vec3, 3);
    jet_vec_ops!(Vec4, 4);

    macro_rules! jet_mat_ops {
        ($T:ident, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..($N * $N) { r[i] = self.0[i] + o.0[i]; } $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0; for i in 0..($N * $N) { r[i] = self.0[i] - o.0[i]; } $T(r)
                }
            }
            // `m * n` is matrix multiply (column-major).
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = [0.0f64; $N * $N];
                    for c in 0..$N { for row in 0..$N {
                        let mut acc = 0.0f64;
                        for k in 0..$N { acc += self.0[k * $N + row] * o.0[c * $N + k]; }
                        r[c * $N + row] = acc;
                    } }
                    $T(r)
                }
            }
        };
    }
    jet_mat_ops!(Mat3, 3);
    jet_mat_ops!(Mat4, 4);

    // `Mat * Vec` transforms the vector (column-major).
    impl std::ops::Mul<Vec3> for Mat3 {
        type Output = Vec3;
        fn mul(self, v: Vec3) -> Vec3 {
            let mut r = [0.0f64; 3];
            for row in 0..3 { let mut a = 0.0f64; for k in 0..3 { a += self.0[k * 3 + row] * v.0[k]; } r[row] = a; }
            Vec3(r)
        }
    }
    impl std::ops::Mul<Vec4> for Mat4 {
        type Output = Vec4;
        fn mul(self, v: Vec4) -> Vec4 {
            let mut r = [0.0f64; 4];
            for row in 0..4 { let mut a = 0.0f64; for k in 0..4 { a += self.0[k * 4 + row] * v.0[k]; } r[row] = a; }
            Vec4(r)
        }
    }

    impl super::JetShow for F32x4 { fn jet_show(&self) -> String { format!("F32x4({:?})", self.0) } }
    impl super::JetShow for F64x2 { fn jet_show(&self) -> String { format!("F64x2({:?})", self.0) } }
    impl super::JetShow for Vec2 { fn jet_show(&self) -> String { format!("Vec2({:?})", self.0) } }
    impl super::JetShow for Vec3 { fn jet_show(&self) -> String { format!("Vec3({:?})", self.0) } }
    impl super::JetShow for Vec4 { fn jet_show(&self) -> String { format!("Vec4({:?})", self.0) } }
    impl super::JetShow for Mat3 { fn jet_show(&self) -> String { format!("Mat3({:?})", self.0) } }
    impl super::JetShow for Mat4 { fn jet_show(&self) -> String { format!("Mat4({:?})", self.0) } }

    pub struct JetTask<T: Send + 'static> {
        handle: Option<super::JetSchedulerJoin<T>>,
        control: std::sync::Arc<super::JetTaskControl>,
    }
    impl<T: Send + 'static> Default for JetTask<T> {
        fn default() -> Self {
            JetTask {
                handle: None,
                control: super::JetTaskControl::new(),
            }
        }
    }
    impl<T: Send + 'static> JetTask<T> {
        pub fn spawn<F: FnOnce() -> T + Send + 'static>(f: F) -> JetTask<T> {
            let inherited_deadline = super::jet_ctx_deadline_ms();
            let control = super::JetTaskControl::new();
            JetTask {
                handle: Some(super::jet_scheduler_spawn_with_control(
                    move || {
                        let _deadline_guard =
                            inherited_deadline.map(super::jet_ctx_push_deadline);
                        f()
                    },
                    control.clone(),
                )),
                control,
            }
        }
        // D-COROUTINE1=A: control-plane hooks on the M:N scheduler substrate.
        pub fn pause(&self) {
            self.control.pause();
        }
        pub fn resume(&self) {
            self.control.resume();
        }
        pub fn cancel(&self) {
            self.control.cancel();
        }
        pub fn trace(&self) -> String {
            let paused = self.control.paused.load(std::sync::atomic::Ordering::Relaxed);
            let cancel = self
                .control
                .cancelled
                .load(std::sync::atomic::Ordering::Relaxed);
            format!("paused={},cancel={}", paused, cancel)
        }
        pub fn join(mut self) -> T {
            super::jet_deadline_check("task join");
            let v = self.handle.take().unwrap().join();
            super::jet_deadline_check("task join");
            v
        }
    }

    /// D-CONCCOMB1=A: join every handle; fail fast and cancel siblings on error.
    pub fn jet_task_all<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> Vec<T> {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|mut t| {
                (
                    t.handle.take().expect("all: task already joined"),
                    t.control,
                )
            })
            .collect();
        super::jet_scheduler_all(entries)
    }

    /// D-CONCCOMB1=A + D-RACEWIN1: first successful result; cancel siblings via scheduler.
    pub fn jet_task_race<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> T {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|mut t| {
                (
                    t.handle.take().expect("race: task already joined"),
                    t.control,
                )
            })
            .collect();
        super::jet_scheduler_race(entries)
    }

    /// D-CONCCOMB1=A: first completed result (success or failure path — v1 propagates panic).
    pub fn jet_task_any<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> T {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|mut t| {
                (
                    t.handle.take().expect("any: task already joined"),
                    t.control,
                )
            })
            .collect();
        super::jet_scheduler_any(entries)
    }

    /// D-CONCSELECT1=A: fluent select builder accumulated at compile time, executed at `.wait()`.
    pub struct JetSelectBuilder<T: Send + 'static> {
        recvs: Vec<JetChannel<T>>,
        after_ms: Vec<i64>,
    }

    impl<T: Send + 'static> JetSelectBuilder<T> {
        pub fn start() -> JetSelectBuilder<T> {
            JetSelectBuilder {
                recvs: Vec::new(),
                after_ms: Vec::new(),
            }
        }
        pub fn recv(mut self, ch: JetChannel<T>) -> JetSelectBuilder<T> {
            self.recvs.push(ch);
            self
        }
        pub fn after(mut self, ms: i64) -> JetSelectBuilder<T> {
            self.after_ms.push(ms);
            self
        }
        pub fn read(self, _stream: super::JetTcpStream) -> JetSelectBuilder<T> {
            self
        }
        pub fn wait(self) -> T {
            let recv_refs: Vec<&JetChannel<T>> = self.recvs.iter().collect();
            jet_select_wait(&recv_refs, &self.after_ms)
        }
    }

    /// D-CONCSELECT1=A: multiplex channel/timer arms registered by `g.select()`.
    pub fn jet_select_wait<T: Send + 'static>(recvs: &[&JetChannel<T>], after_ms: &[i64]) -> T {
        let inners: Vec<_> = recvs.iter().map(|c| c.inner.select_inner()).collect();
        let timers: Vec<u64> = after_ms.iter().map(|ms| (*ms).max(0) as u64).collect();
        match super::jet_scheduler_select(inners, timers) {
            super::JetSelectOutcome::Recv { value, .. } => value,
            super::JetSelectOutcome::After { .. } => {
                eprintln!("panic: select timer arm has no receive value");
                std::process::exit(70);
            }
            super::JetSelectOutcome::Closed => {
                eprintln!("panic: select closed");
                std::process::exit(70);
            }
        }
    }

    pub struct JetChannel<T> {
        inner: super::JetSchedulerChannel<T>,
    }
    impl<T: Send> JetChannel<T> {
        pub fn new() -> JetChannel<T> {
            JetChannel {
                inner: super::JetSchedulerChannel::new(),
            }
        }
        pub fn sender(&self) -> JetSender<T> {
            JetSender {
                tx: self.inner.sender(),
            }
        }
        pub fn receive(&self) -> Result<T, Closed> {
            if super::jet_scheduler_task_cancelled() {
                return Err(Closed::Closed);
            }
            if let Some(remaining) = super::jet_deadline_remaining_ms() {
                if remaining <= 0 {
                    super::jet_deadline_exceeded("channel receive");
                }
            }
            match self.inner.receive() {
                Some(v) => {
                    super::jet_deadline_check("channel receive");
                    Ok(v)
                }
                None => Err(Closed::Closed),
            }
        }
    }

    pub struct JetSender<T> {
        tx: super::JetSchedulerSender<T>,
    }
    impl<T: Send> JetSender<T> {
        pub fn send(&self, value: T) {
            self.tx.send(value);
        }
    }
    impl<T> Clone for JetSender<T> {
        fn clone(&self) -> Self {
            JetSender {
                tx: self.tx.clone(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Closed { Closed }

    // ── D-REACT1=B: opt-in reactive runtime (signals / derived / effects) ──────
    // Reactivity is a LIBRARY, not core semantics (option B): ordinary bindings are
    // unchanged; these types are the explicit, opt-in surface. Pure std — no external
    // crate (I6) and no raw-memory tier (interior mutability via Rc/RefCell). Dependency
    // tracking is explicit-by-read: a `.get()` evaluated while an observer (a derived
    // recompute or an effect run) is on the thread-local stack subscribes that
    // observer to the signal. A `.set(v)` re-runs every subscribed observer.
    use std::cell::RefCell;
    use std::rc::Rc;

    type Observer = Rc<dyn Fn()>;

    thread_local! {
        // The stack of observers currently (re)computing. The top is the active one.
        static JET_REACTIVE_OBSERVERS: RefCell<Vec<Observer>> = const { RefCell::new(Vec::new()) };
    }

    fn jet_reactive_active_observer() -> Option<Observer> {
        JET_REACTIVE_OBSERVERS.with(|s| s.borrow().last().cloned())
    }

    fn jet_reactive_run_observed(obs: &Observer, body: &dyn Fn()) {
        JET_REACTIVE_OBSERVERS.with(|s| s.borrow_mut().push(obs.clone()));
        body();
        JET_REACTIVE_OBSERVERS.with(|s| { s.borrow_mut().pop(); });
    }

    struct SignalCell<T> {
        value: T,
        // Subscribers are re-run on set. Held as weak-free Rc closures; an effect or
        // derived keeps its own observer alive, so these stay valid for the run.
        subs: Vec<Observer>,
    }

    pub struct JetSignal<T> {
        cell: Rc<RefCell<SignalCell<T>>>,
    }

    impl<T> Clone for JetSignal<T> {
        fn clone(&self) -> Self { JetSignal { cell: self.cell.clone() } }
    }

    impl<T: Clone> JetSignal<T> {
        pub fn new(initial: T) -> JetSignal<T> {
            JetSignal { cell: Rc::new(RefCell::new(SignalCell { value: initial, subs: Vec::new() })) }
        }
        pub fn get(&self) -> T {
            if let Some(obs) = jet_reactive_active_observer() {
                let mut c = self.cell.borrow_mut();
                if !c.subs.iter().any(|s| Rc::ptr_eq(s, &obs)) {
                    c.subs.push(obs);
                }
            }
            self.cell.borrow().value.clone()
        }
        pub fn set(&self, value: T) {
            let subs = {
                let mut c = self.cell.borrow_mut();
                c.value = value;
                c.subs.clone()
            };
            for s in subs {
                s();
            }
        }
    }

    // A derived value is itself observable: it holds a current value plus its own
    // subscriber list, so effects (and other deriveds) that read it re-run when it
    // recomputes. The `_observer` it registers with its source signals recomputes the
    // value and then notifies the derived's own subscribers.
    pub struct JetDerived<T> {
        cell: Rc<RefCell<SignalCell<T>>>,
        _observer: Observer,
    }

    impl<T> Clone for JetDerived<T> {
        fn clone(&self) -> Self {
            JetDerived { cell: self.cell.clone(), _observer: self._observer.clone() }
        }
    }

    impl<T: Clone + 'static> JetDerived<T> {
        pub fn new<F: Fn() -> T + 'static>(compute: F) -> JetDerived<T> {
            let compute = Rc::new(compute);
            let cell: Rc<RefCell<SignalCell<T>>> =
                Rc::new(RefCell::new(SignalCell { value: (compute)(), subs: Vec::new() }));
            // The observer recomputes the value, then notifies the derived's own subs.
            let cell_for_obs = cell.clone();
            let compute_for_obs = compute.clone();
            let observer: Observer = Rc::new(move || {
                let v = (compute_for_obs)();
                let subs = {
                    let mut c = cell_for_obs.borrow_mut();
                    c.value = v;
                    c.subs.clone()
                };
                for s in subs {
                    s();
                }
            });
            // Run once under observation to record the source-signal dependency set.
            jet_reactive_run_observed(&observer, &{
                let cell = cell.clone();
                let compute = compute.clone();
                move || {
                    let v = (compute)();
                    cell.borrow_mut().value = v;
                }
            });
            JetDerived { cell, _observer: observer }
        }
        pub fn get(&self) -> T {
            // Reading a derived inside an observer subscribes that observer to it.
            if let Some(obs) = jet_reactive_active_observer() {
                let mut c = self.cell.borrow_mut();
                if !c.subs.iter().any(|s| Rc::ptr_eq(s, &obs)) {
                    c.subs.push(obs);
                }
            }
            self.cell.borrow().value.clone()
        }
    }

    /// `reactive.effect(body)` — run `body` now, and again whenever a signal it read
    /// changes. The first run records the effect's dependencies; each subscribed
    /// signal then holds an `Rc` to the observer, keeping the effect alive for as long
    /// as a signal it reads is alive (a long-lived reactive sink). An effect that reads
    /// no signal simply runs once.
    pub fn jet_reactive_effect<F: Fn() + 'static>(body: F) {
        let observer: Observer = Rc::new(body);
        let run = observer.clone();
        jet_reactive_run_observed(&observer, &move || { run(); });
    }

    /// D-REACTCORE1: `#Reactive` scope marker — alias for `jet_reactive_effect`.
    pub fn jet_reactive_scope<F: Fn() + 'static>(body: F) {
        jet_reactive_effect(body);
    }

    impl super::JetShow for Closed {
        fn jet_show(&self) -> String { "Closed".to_string() }
    }

    // D-HONESTNUM1=A: Measurement<T> — a value paired with its standard uncertainty.
    // Arithmetic propagates uncertainty using the standard quadrature rules.
    // Only `JetMeasurement<f64>` (Float) is exposed to Jet programs.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct JetMeasurement<T: Copy> {
        value: T,
        uncertainty: T,
    }

    impl JetMeasurement<f64> {
        pub fn new(value: f64, uncertainty: f64) -> Self {
            JetMeasurement { value, uncertainty }
        }
        pub fn value(&self) -> f64 { self.value }
        pub fn uncertainty(&self) -> f64 { self.uncertainty }
        // Addition / subtraction: σ_z = sqrt(σ_a² + σ_b²)
        pub fn add(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value + other.value,
                uncertainty: (self.uncertainty * self.uncertainty
                    + other.uncertainty * other.uncertainty).sqrt(),
            }
        }
        pub fn sub(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value - other.value,
                uncertainty: (self.uncertainty * self.uncertainty
                    + other.uncertainty * other.uncertainty).sqrt(),
            }
        }
        // Multiplication: σ_z = sqrt((b·σ_a)² + (a·σ_b)²)
        pub fn mul(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value * other.value,
                uncertainty: ((other.value * self.uncertainty).powi(2)
                    + (self.value * other.uncertainty).powi(2)).sqrt(),
            }
        }
        // Division: σ_z = sqrt((σ_a/b)² + (a·σ_b/b²)²)
        pub fn div(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value / other.value,
                uncertainty: ((self.uncertainty / other.value).powi(2)
                    + (self.value * other.uncertainty / (other.value * other.value)).powi(2))
                .sqrt(),
            }
        }
    }

    impl super::JetShow for JetMeasurement<f64> {
        fn jet_show(&self) -> String {
            format!("{:?} \u{00b1} {:?}", self.value, self.uncertainty)
        }
    }

    pub fn io_error(path: &str, e: std::io::Error) -> IoError {
        match e.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound { path: path.to_string() },
            std::io::ErrorKind::PermissionDenied => {
                IoError::PermissionDenied { path: path.to_string() }
            }
            _ => IoError::Other { message: e.to_string() },
        }
    }

    pub fn parse_json(text: &str) -> Result<Json, JsonError> {
        let mut p = JsonParser { chars: text.chars().collect(), pos: 0 };
        let v = p.value()?;
        p.ws();
        if p.pos != p.chars.len() {
            return Err(p.err("extra text after JSON value"));
        }
        Ok(v)
    }

    pub fn render_json(j: &Json, pretty: bool, depth: usize) -> String {
        match j {
            Json::Null => "null".to_string(),
            Json::Boolean(b) => b.to_string(),
            Json::Number(n) => format!("{:?}", n),
            Json::Text(s) => quote_json(s),
            Json::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = items.iter().map(|x| render_json(x, false, depth)).collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            Json::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{}:{}", quote_json(k), render_json(v, false, depth)))
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}{}: {}", pad, quote_json(k), render_json(v, true, depth + 1)))
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    fn quote_json(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\u{0008}' => out.push_str("\\b"),
                '\u{000c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    // Render a DataTree as JSON, preserving Object field order. Int prints with no
    // decimal (`5`), Float keeps its decimal (`5.0`); Bytes render as a number array.
    pub fn render_datatree_json(t: &DataTree, pretty: bool, depth: usize) -> String {
        match t {
            DataTree::Null => "null".to_string(),
            DataTree::Bool(b) => b.to_string(),
            DataTree::Int(n) => format!("{}", n),
            DataTree::Float(f) => format!("{:?}", f),
            DataTree::Text(s) => quote_json(s),
            DataTree::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(","))
            }
            DataTree::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> =
                        items.iter().map(|x| render_datatree_json(x, false, depth)).collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_datatree_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            DataTree::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{}:{}", quote_json(k), render_datatree_json(v, false, depth)))
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}{}: {}", pad, quote_json(k), render_datatree_json(v, true, depth + 1)))
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    // Json (dynamic, BTreeMap-keyed) → DataTree. Numbers that are integral collapse
    // to `Int`, so a round-trip through JSON keeps `5` an Int.
    pub fn datatree_from_json(j: &Json) -> DataTree {
        match j {
            Json::Null => DataTree::Null,
            Json::Boolean(b) => DataTree::Bool(*b),
            Json::Number(n) => {
                if n.fract() == 0.0 && n.is_finite() && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                    DataTree::Int(*n as i64)
                } else {
                    DataTree::Float(*n)
                }
            }
            Json::Text(s) => DataTree::Text(s.clone()),
            Json::Array(items) => DataTree::Array(items.iter().map(datatree_from_json).collect()),
            Json::Object(m) => {
                DataTree::Object(m.iter().map(|(k, v)| (k.clone(), datatree_from_json(v))).collect())
            }
        }
    }

    // A short kind name for decode error messages.
    pub fn datatree_kind(t: &DataTree) -> &'static str {
        match t {
            DataTree::Null => "null",
            DataTree::Bool(_) => "Bool",
            DataTree::Int(_) => "Int",
            DataTree::Float(_) => "Float",
            DataTree::Text(_) => "Text",
            DataTree::Bytes(_) => "Bytes",
            DataTree::Array(_) => "a list",
            DataTree::Object(_) => "an object",
        }
    }

    // Look up a key in an ordered Object.
    pub fn datatree_get<'a>(t: &'a DataTree, key: &str) -> Option<&'a DataTree> {
        match t {
            DataTree::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    struct JsonParser {
        chars: Vec<char>,
        pos: usize,
    }

    impl JsonParser {
        fn err(&self, msg: &str) -> JsonError {
            let line = self.chars[..self.pos.min(self.chars.len())]
                .iter()
                .filter(|c| **c == '\n')
                .count() as i64
                + 1;
            JsonError { line, message: msg.to_string() }
        }

        fn ws(&mut self) {
            while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
                self.pos += 1;
            }
        }

        fn value(&mut self) -> Result<Json, JsonError> {
            self.ws();
            match self.peek() {
                Some('n') => self.word("null", Json::Null),
                Some('t') => self.word("true", Json::Boolean(true)),
                Some('f') => self.word("false", Json::Boolean(false)),
                Some('"') => Ok(Json::Text(self.string()?)),
                Some('[') => self.array(),
                Some('{') => self.object(),
                Some('-') | Some('0'..='9') => self.number(),
                _ => Err(self.err("expected a JSON value")),
            }
        }

        fn peek(&self) -> Option<char> { self.chars.get(self.pos).copied() }

        fn word(&mut self, w: &str, v: Json) -> Result<Json, JsonError> {
            for ch in w.chars() {
                if self.peek() != Some(ch) {
                    return Err(self.err("expected a JSON word"));
                }
                self.pos += 1;
            }
            Ok(v)
        }

        fn string(&mut self) -> Result<String, JsonError> {
            if self.peek() != Some('"') {
                return Err(self.err("expected quoted text"));
            }
            self.pos += 1;
            let mut out = String::new();
            while let Some(c) = self.peek() {
                self.pos += 1;
                match c {
                    '"' => return Ok(out),
                    '\\' => {
                        let Some(e) = self.peek() else { return Err(self.err("unfinished escape")); };
                        self.pos += 1;
                        match e {
                            '"' => out.push('"'),
                            '\\' => out.push('\\'),
                            '/' => out.push('/'),
                            'b' => out.push('\u{0008}'),
                            'f' => out.push('\u{000c}'),
                            'n' => out.push('\n'),
                            'r' => out.push('\r'),
                            't' => out.push('\t'),
                            'u' => self.unicode_escape(&mut out)?,
                            _ => return Err(self.err("invalid escape in string")),
                        }
                    }
                    c if (c as u32) < 0x20 => return Err(self.err("control character in string")),
                    other => out.push(other),
                }
            }
            Err(self.err("missing closing quote"))
        }

        // A `\uXXXX` escape, already past the `u`. Combines a high+low surrogate
        // pair into one code point; rejects a lone or malformed surrogate.
        fn unicode_escape(&mut self, out: &mut String) -> Result<(), JsonError> {
            let cp = self.hex4()?;
            if (0xD800..=0xDBFF).contains(&cp) {
                if self.peek() != Some('\\') {
                    return Err(self.err("unpaired surrogate in string"));
                }
                self.pos += 1;
                if self.peek() != Some('u') {
                    return Err(self.err("unpaired surrogate in string"));
                }
                self.pos += 1;
                let lo = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    return Err(self.err("unpaired surrogate in string"));
                }
                let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                match char::from_u32(combined) {
                    Some(ch) => out.push(ch),
                    None => return Err(self.err("invalid unicode escape")),
                }
            } else if (0xDC00..=0xDFFF).contains(&cp) {
                return Err(self.err("unpaired surrogate in string"));
            } else {
                match char::from_u32(cp) {
                    Some(ch) => out.push(ch),
                    None => return Err(self.err("invalid unicode escape")),
                }
            }
            Ok(())
        }

        fn hex4(&mut self) -> Result<u32, JsonError> {
            let mut v = 0u32;
            for _ in 0..4 {
                let Some(c) = self.peek() else { return Err(self.err("truncated unicode escape")); };
                let d = c.to_digit(16).ok_or_else(|| self.err("invalid unicode escape"))?;
                v = v * 16 + d;
                self.pos += 1;
            }
            Ok(v)
        }

        fn number(&mut self) -> Result<Json, JsonError> {
            let start = self.pos;
            if self.peek() == Some('-') {
                self.pos += 1;
            }
            // Integer part: `0` alone, or a non-zero digit then more digits.
            match self.peek() {
                Some('0') => self.pos += 1,
                Some('1'..='9') => {
                    self.pos += 1;
                    while matches!(self.peek(), Some('0'..='9')) {
                        self.pos += 1;
                    }
                }
                _ => return Err(self.err("bad number")),
            }
            // Fraction: a `.` must be followed by at least one digit.
            if self.peek() == Some('.') {
                self.pos += 1;
                if !matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.err("bad number"));
                }
                while matches!(self.peek(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            // Exponent: `e`/`E`, optional sign, at least one digit.
            if matches!(self.peek(), Some('e') | Some('E')) {
                self.pos += 1;
                if matches!(self.peek(), Some('+') | Some('-')) {
                    self.pos += 1;
                }
                if !matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.err("bad number"));
                }
                while matches!(self.peek(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            let s: String = self.chars[start..self.pos].iter().collect();
            match s.parse::<f64>() {
                Ok(n) => Ok(Json::Number(n)),
                Err(_) => Err(self.err("bad number")),
            }
        }

        fn array(&mut self) -> Result<Json, JsonError> {
            self.pos += 1;
            let mut out = Vec::new();
            loop {
                self.ws();
                if self.peek() == Some(']') {
                    self.pos += 1;
                    return Ok(Json::Array(out));
                }
                out.push(self.value()?);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some(']') => {}
                    _ => return Err(self.err("expected `,` or `]`")),
                }
            }
        }

        fn object(&mut self) -> Result<Json, JsonError> {
            self.pos += 1;
            let mut out = std::collections::BTreeMap::new();
            loop {
                self.ws();
                if self.peek() == Some('}') {
                    self.pos += 1;
                    return Ok(Json::Object(out));
                }
                let key = self.string()?;
                self.ws();
                if self.peek() != Some(':') {
                    return Err(self.err("expected `:` after object key"));
                }
                self.pos += 1;
                let value = self.value()?;
                out.insert(key, value);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some('}') => {}
                    _ => return Err(self.err("expected `,` or `}`")),
                }
            }
        }
    }

    // ── core.encoding.toml: full TOML 1.0 → DataTree (D-ENC-DYN1=A+, c152) ────────
    // A complete std-only TOML 1.0 parser (ported from the compiler's
    // Source/Jetpack/TOML.rs, which the emitted prelude cannot reach) that lowers a
    // document onto the one rich `DataTree`. Strings (every escape + multi-line),
    // integers in every base, floats incl. inf/nan, booleans, datetimes (kept raw),
    // arrays, inline tables, dotted keys, `[table]` headers, and `[[array-of-tables]]`.
    pub mod toml {
        use super::DataTree;

        #[derive(Clone, Debug, PartialEq)]
        pub enum Value {
            String(String),
            Integer(i64),
            Float(f64),
            Boolean(bool),
            Datetime(String),
            Array(Vec<Value>),
            InlineTable(Vec<(String, Value)>),
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum Item {
            Header { path: Vec<String>, array: bool },
            KeyVal { path: Vec<String>, value: Value },
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ParseError {
            pub line: usize,
            pub message: String,
        }

        pub fn parse_to_tree(raw: &str) -> Result<DataTree, ParseError> {
            let mut p = Parser { chars: raw.chars().collect(), pos: 0, line: 1 };
            let mut items = Vec::new();
            loop {
                p.skip_between_statements();
                if p.peek().is_none() {
                    break;
                }
                match p.statement()? {
                    Some(item) => items.push(item),
                    None => {}
                }
            }
            Ok(assemble(items))
        }

        // ── Assembly: a flat Item list → a nested ordered `DataTree::Object` ──────
        fn assemble(items: Vec<Item>) -> DataTree {
            let mut root = DataTree::Object(Vec::new());
            let mut current: Vec<String> = Vec::new();
            for item in items {
                match item {
                    Item::Header { path, array } => {
                        if array {
                            push_array_table(&mut root, &path);
                        } else {
                            // Ensure the table exists.
                            let _ = table_at(&mut root, &path);
                        }
                        current = path;
                    }
                    Item::KeyVal { path, value } => {
                        set_key(&mut root, &current, &path, value_to_tree(value));
                    }
                }
            }
            root
        }

        // Navigate to (creating along the way) the table at `path`. When a segment is
        // an array-of-tables, descend into its LAST element.
        fn table_at<'a>(mut node: &'a mut DataTree, path: &[String]) -> &'a mut DataTree {
            for seg in path {
                node = child_table_mut(node, seg);
            }
            node
        }

        fn child_table_mut<'a>(node: &'a mut DataTree, seg: &str) -> &'a mut DataTree {
            let entries = match node {
                DataTree::Object(entries) => entries,
                other => return other,
            };
            let idx = match entries.iter().position(|(k, _)| k == seg) {
                Some(i) => i,
                None => {
                    entries.push((seg.to_string(), DataTree::Object(Vec::new())));
                    entries.len() - 1
                }
            };
            // An existing array-of-tables: descend into its last element. Decide the
            // target index immutably first, then take exactly one mutable borrow per
            // branch of a match on the (non-borrowing) `Option` — sidesteps the NLL snag.
            let arr_last: Option<usize> = match &entries[idx].1 {
                DataTree::Array(arr) if !arr.is_empty() => Some(arr.len() - 1),
                _ => None,
            };
            match arr_last {
                Some(n) => match &mut entries[idx].1 {
                    DataTree::Array(arr) => &mut arr[n],
                    other => other,
                },
                None => &mut entries[idx].1,
            }
        }

        fn push_array_table(root: &mut DataTree, path: &[String]) {
            let (parent_path, last) = path.split_at(path.len() - 1);
            let parent = table_at(root, parent_path);
            if let DataTree::Object(entries) = parent {
                let idx = match entries.iter().position(|(k, _)| k == &last[0]) {
                    Some(i) => i,
                    None => {
                        entries.push((last[0].clone(), DataTree::Array(Vec::new())));
                        entries.len() - 1
                    }
                };
                if let DataTree::Array(arr) = &mut entries[idx].1 {
                    arr.push(DataTree::Object(Vec::new()));
                }
            }
        }

        fn set_key(root: &mut DataTree, current: &[String], key_path: &[String], value: DataTree) {
            let mut full: Vec<String> = current.to_vec();
            full.extend_from_slice(&key_path[..key_path.len() - 1]);
            let table = table_at(root, &full);
            let fk = &key_path[key_path.len() - 1];
            if let DataTree::Object(entries) = table {
                if let Some(slot) = entries.iter_mut().find(|(k, _)| k == fk) {
                    slot.1 = value;
                } else {
                    entries.push((fk.clone(), value));
                }
            }
        }

        fn value_to_tree(v: Value) -> DataTree {
            match v {
                Value::String(s) => DataTree::Text(s),
                Value::Integer(n) => DataTree::Int(n),
                Value::Float(f) => DataTree::Float(f),
                Value::Boolean(b) => DataTree::Bool(b),
                Value::Datetime(s) => DataTree::Text(s),
                Value::Array(xs) => DataTree::Array(xs.into_iter().map(value_to_tree).collect()),
                Value::InlineTable(es) => {
                    DataTree::Object(es.into_iter().map(|(k, v)| (k, value_to_tree(v))).collect())
                }
            }
        }

        // ── Render: a `DataTree` → TOML text (nested headers, arrays-of-tables) ───
        pub fn render(t: &DataTree) -> String {
            let mut out = String::new();
            render_table(t, &[], &mut out);
            out.trim_end().to_string()
        }

        fn is_table(v: &DataTree) -> bool {
            matches!(v, DataTree::Object(_))
        }
        fn is_array_of_tables(v: &DataTree) -> bool {
            matches!(v, DataTree::Array(arr)
                if !arr.is_empty() && arr.iter().all(|e| matches!(e, DataTree::Object(_))))
        }

        fn render_table(t: &DataTree, path: &[String], out: &mut String) {
            if let DataTree::Object(entries) = t {
                for (k, v) in entries {
                    if !is_table(v) && !is_array_of_tables(v) {
                        out.push_str(&format!("{} = {}\n", k, render_value(v)));
                    }
                }
                for (k, v) in entries {
                    if is_table(v) {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        out.push_str(&format!("\n[{}]\n", p.join(".")));
                        render_table(v, &p, out);
                    } else if is_array_of_tables(v) {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        if let DataTree::Array(arr) = v {
                            for elem in arr {
                                out.push_str(&format!("\n[[{}]]\n", p.join(".")));
                                render_table(elem, &p, out);
                            }
                        }
                    }
                }
            }
        }

        fn render_value(v: &DataTree) -> String {
            match v {
                DataTree::Null => "\"\"".to_string(),
                DataTree::Bool(b) => b.to_string(),
                DataTree::Int(n) => n.to_string(),
                DataTree::Float(f) => format!("{:?}", f),
                DataTree::Text(s) => super::quote_json(s),
                DataTree::Bytes(bs) => {
                    let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                    format!("[{}]", parts.join(", "))
                }
                DataTree::Array(items) => {
                    let parts: Vec<String> = items.iter().map(render_value).collect();
                    format!("[{}]", parts.join(", "))
                }
                // An inline (non-header) object renders as a TOML inline table.
                DataTree::Object(entries) => {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, val)| format!("{} = {}", k, render_value(val)))
                        .collect();
                    format!("{{ {} }}", parts.join(", "))
                }
            }
        }

        struct Parser {
            chars: Vec<char>,
            pos: usize,
            line: usize,
        }

        impl Parser {
            fn peek(&self) -> Option<char> { self.chars.get(self.pos).copied() }
            fn peek_at(&self, n: usize) -> Option<char> { self.chars.get(self.pos + n).copied() }
            fn bump(&mut self) -> Option<char> {
                let c = self.peek()?;
                self.pos += 1;
                if c == '\n' { self.line += 1; }
                Some(c)
            }
            fn err(&self, message: impl Into<String>) -> ParseError {
                ParseError { line: self.line, message: message.into() }
            }
            fn skip_inline_ws(&mut self) {
                while matches!(self.peek(), Some(' ' | '\t')) { self.pos += 1; }
            }
            fn skip_between_statements(&mut self) {
                loop {
                    match self.peek() {
                        Some(' ' | '\t' | '\r' | '\n') => { self.bump(); }
                        Some('#') => self.skip_comment(),
                        _ => break,
                    }
                }
            }
            fn skip_comment(&mut self) {
                while let Some(c) = self.peek() {
                    if c == '\n' { break; }
                    self.pos += 1;
                }
            }
            fn finish_line(&mut self) -> Result<(), ParseError> {
                self.skip_inline_ws();
                if self.peek() == Some('#') { self.skip_comment(); }
                match self.peek() {
                    None | Some('\n') | Some('\r') => Ok(()),
                    Some(c) => Err(self.err(format!("unexpected `{c}` after value"))),
                }
            }
            fn statement(&mut self) -> Result<Option<Item>, ParseError> {
                match self.peek() {
                    Some('[') => self.header().map(Some),
                    _ => self.key_value().map(Some),
                }
            }
            fn header(&mut self) -> Result<Item, ParseError> {
                self.bump(); // first '['
                let array = self.peek() == Some('[');
                if array { self.bump(); }
                self.skip_inline_ws();
                let path = self.key_path()?;
                self.skip_inline_ws();
                if self.peek() != Some(']') {
                    return Err(self.err("expected `]` to close a table header"));
                }
                self.bump();
                if array {
                    if self.peek() != Some(']') {
                        return Err(self.err("expected `]]` to close an array-of-tables header"));
                    }
                    self.bump();
                }
                if path.is_empty() {
                    return Err(self.err("a table header must name a table"));
                }
                self.finish_line()?;
                Ok(Item::Header { path, array })
            }
            fn key_value(&mut self) -> Result<Item, ParseError> {
                let path = self.key_path()?;
                if path.is_empty() {
                    return Err(self.err("expected a key"));
                }
                self.skip_inline_ws();
                if self.peek() != Some('=') {
                    return Err(self.err(format!("expected `=` after key `{}`", path.join("."))));
                }
                self.bump();
                self.skip_inline_ws();
                let value = self.value()?;
                self.finish_line()?;
                Ok(Item::KeyVal { path, value })
            }
            fn key_path(&mut self) -> Result<Vec<String>, ParseError> {
                let mut path = Vec::new();
                loop {
                    self.skip_inline_ws();
                    path.push(self.simple_key()?);
                    self.skip_inline_ws();
                    if self.peek() == Some('.') { self.bump(); } else { break; }
                }
                Ok(path)
            }
            fn simple_key(&mut self) -> Result<String, ParseError> {
                match self.peek() {
                    Some('"') => self.basic_string(),
                    Some('\'') => self.literal_string(),
                    Some(c) if is_bare_key_char(c) => {
                        let mut s = String::new();
                        while let Some(c) = self.peek() {
                            if is_bare_key_char(c) { s.push(c); self.pos += 1; } else { break; }
                        }
                        Ok(s)
                    }
                    Some(c) => Err(self.err(format!("`{c}` is not a valid key character"))),
                    None => Err(self.err("expected a key")),
                }
            }
            fn value(&mut self) -> Result<Value, ParseError> {
                match self.peek() {
                    Some('"') => Ok(Value::String(self.basic_string()?)),
                    Some('\'') => Ok(Value::String(self.literal_string()?)),
                    Some('[') => self.array(),
                    Some('{') => self.inline_table(),
                    Some('t') | Some('f') => self.boolean(),
                    Some('+') | Some('-') | Some('0'..='9') | Some('i') | Some('n') => self.number_or_datetime(),
                    Some(c) => Err(self.err(format!("`{c}` does not start a valid value"))),
                    None => Err(self.err("expected a value")),
                }
            }
            fn boolean(&mut self) -> Result<Value, ParseError> {
                if self.try_keyword("true") { Ok(Value::Boolean(true)) }
                else if self.try_keyword("false") { Ok(Value::Boolean(false)) }
                else { Err(self.err("expected `true` or `false`")) }
            }
            fn try_keyword(&mut self, kw: &str) -> bool {
                let chars: Vec<char> = kw.chars().collect();
                for (i, c) in chars.iter().enumerate() {
                    if self.peek_at(i) != Some(*c) { return false; }
                }
                if let Some(after) = self.peek_at(chars.len()) {
                    if is_bare_key_char(after) || after == '.' { return false; }
                }
                for _ in 0..chars.len() { self.bump(); }
                true
            }
            fn basic_string(&mut self) -> Result<String, ParseError> {
                if self.peek() == Some('"') && self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"') {
                    return self.multiline_basic_string();
                }
                self.bump();
                let mut out = String::new();
                loop {
                    match self.bump() {
                        None | Some('\n') => return Err(self.err("unterminated string")),
                        Some('"') => return Ok(out),
                        Some('\\') => out.push(self.string_escape()?),
                        Some(c) if (c as u32) < 0x20 => return Err(self.err("control character in string")),
                        Some(c) => out.push(c),
                    }
                }
            }
            fn multiline_basic_string(&mut self) -> Result<String, ParseError> {
                self.bump(); self.bump(); self.bump();
                if self.peek() == Some('\r') { self.bump(); }
                if self.peek() == Some('\n') { self.bump(); }
                let mut out = String::new();
                loop {
                    if self.peek() == Some('"') && self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"') {
                        self.bump(); self.bump(); self.bump();
                        return Ok(out);
                    }
                    match self.bump() {
                        None => return Err(self.err("unterminated multi-line string")),
                        Some('\\') => {
                            if matches!(self.peek(), Some('\n') | Some('\r') | Some(' ') | Some('\t')) {
                                let mut sawline = false;
                                let save = self.pos;
                                let saveline = self.line;
                                while matches!(self.peek(), Some(' ') | Some('\t') | Some('\r') | Some('\n')) {
                                    if self.peek() == Some('\n') { sawline = true; }
                                    self.bump();
                                }
                                if !sawline {
                                    self.pos = save;
                                    self.line = saveline;
                                    out.push(self.string_escape()?);
                                }
                            } else {
                                out.push(self.string_escape()?);
                            }
                        }
                        Some(c) => out.push(c),
                    }
                }
            }
            fn string_escape(&mut self) -> Result<char, ParseError> {
                match self.bump() {
                    Some('"') => Ok('"'),
                    Some('\\') => Ok('\\'),
                    Some('b') => Ok('\u{0008}'),
                    Some('f') => Ok('\u{000c}'),
                    Some('n') => Ok('\n'),
                    Some('r') => Ok('\r'),
                    Some('t') => Ok('\t'),
                    Some('u') => self.unicode_escape(4),
                    Some('U') => self.unicode_escape(8),
                    Some(c) => Err(self.err(format!("invalid escape `\\{c}`"))),
                    None => Err(self.err("unterminated escape")),
                }
            }
            fn unicode_escape(&mut self, n: usize) -> Result<char, ParseError> {
                let mut v = 0u32;
                for _ in 0..n {
                    let Some(c) = self.peek() else { return Err(self.err("truncated unicode escape")); };
                    let Some(d) = c.to_digit(16) else { return Err(self.err("invalid unicode escape")); };
                    v = v * 16 + d;
                    self.pos += 1;
                }
                char::from_u32(v).ok_or_else(|| self.err("invalid unicode scalar value"))
            }
            fn literal_string(&mut self) -> Result<String, ParseError> {
                if self.peek() == Some('\'') && self.peek_at(1) == Some('\'') && self.peek_at(2) == Some('\'') {
                    return self.multiline_literal_string();
                }
                self.bump();
                let mut out = String::new();
                loop {
                    match self.bump() {
                        None | Some('\n') => return Err(self.err("unterminated literal string")),
                        Some('\'') => return Ok(out),
                        Some(c) => out.push(c),
                    }
                }
            }
            fn multiline_literal_string(&mut self) -> Result<String, ParseError> {
                self.bump(); self.bump(); self.bump();
                if self.peek() == Some('\r') { self.bump(); }
                if self.peek() == Some('\n') { self.bump(); }
                let mut out = String::new();
                loop {
                    if self.peek() == Some('\'') && self.peek_at(1) == Some('\'') && self.peek_at(2) == Some('\'') {
                        self.bump(); self.bump(); self.bump();
                        return Ok(out);
                    }
                    match self.bump() {
                        None => return Err(self.err("unterminated multi-line literal string")),
                        Some(c) => out.push(c),
                    }
                }
            }
            fn array(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_ws_newlines_comments();
                    match self.peek() {
                        Some(']') => { self.bump(); return Ok(Value::Array(items)); }
                        None => return Err(self.err("unterminated array")),
                        _ => {}
                    }
                    items.push(self.value()?);
                    self.skip_ws_newlines_comments();
                    match self.peek() {
                        Some(',') => { self.bump(); }
                        Some(']') => { self.bump(); return Ok(Value::Array(items)); }
                        Some(c) => return Err(self.err(format!("expected `,` or `]` in array, found `{c}`"))),
                        None => return Err(self.err("unterminated array")),
                    }
                }
            }
            fn skip_ws_newlines_comments(&mut self) {
                loop {
                    match self.peek() {
                        Some(' ' | '\t' | '\r' | '\n') => { self.bump(); }
                        Some('#') => self.skip_comment(),
                        _ => break,
                    }
                }
            }
            fn inline_table(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let mut entries = Vec::new();
                self.skip_inline_ws();
                if self.peek() == Some('}') { self.bump(); return Ok(Value::InlineTable(entries)); }
                loop {
                    self.skip_inline_ws();
                    let path = self.key_path()?;
                    self.skip_inline_ws();
                    if self.bump() != Some('=') {
                        return Err(self.err("expected `=` in inline table"));
                    }
                    self.skip_inline_ws();
                    let value = self.value()?;
                    entries.push((path.join("."), value));
                    self.skip_inline_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some('}') => return Ok(Value::InlineTable(entries)),
                        Some(c) => return Err(self.err(format!("expected `,` or `}}` in inline table, found `{c}`"))),
                        None => return Err(self.err("unterminated inline table")),
                    }
                }
            }
            fn number_or_datetime(&mut self) -> Result<Value, ParseError> {
                if self.looks_like_date() || self.looks_like_time() {
                    return self.datetime();
                }
                self.number()
            }
            fn looks_like_date(&self) -> bool {
                let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
                d(0) && d(1) && d(2) && d(3)
                    && self.peek_at(4) == Some('-')
                    && d(5) && d(6)
                    && self.peek_at(7) == Some('-')
                    && d(8) && d(9)
            }
            fn looks_like_time(&self) -> bool {
                let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
                d(0) && d(1) && self.peek_at(2) == Some(':') && d(3) && d(4)
            }
            fn datetime(&mut self) -> Result<Value, ParseError> {
                let mut s = String::new();
                let is_dt = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '.' | '+');
                while let Some(c) = self.peek() {
                    if is_dt(c) { s.push(c); self.pos += 1; }
                    else if c == ' '
                        && self.peek_at(1).map_or(false, |n| n.is_ascii_digit())
                        && self.peek_at(3) == Some(':')
                    { s.push(' '); self.pos += 1; }
                    else { break; }
                }
                Ok(Value::Datetime(s))
            }
            fn number(&mut self) -> Result<Value, ParseError> {
                if self.try_keyword("inf") { return Ok(Value::Float(f64::INFINITY)); }
                if self.try_keyword("nan") { return Ok(Value::Float(f64::NAN)); }
                let mut tok = String::new();
                if matches!(self.peek(), Some('+') | Some('-')) {
                    let sign = self.bump().unwrap();
                    tok.push(sign);
                    if self.try_keyword("inf") {
                        return Ok(Value::Float(if sign == '-' { f64::NEG_INFINITY } else { f64::INFINITY }));
                    }
                    if self.try_keyword("nan") { return Ok(Value::Float(f64::NAN)); }
                }
                if self.peek() == Some('0') {
                    if let Some(r) = self.peek_at(1) {
                        if matches!(r, 'x' | 'o' | 'b') && tok.is_empty() {
                            return self.radix_integer();
                        }
                    }
                }
                let mut is_float = false;
                while let Some(c) = self.peek() {
                    match c {
                        '0'..='9' | '_' => { tok.push(c); self.pos += 1; }
                        '.' | 'e' | 'E' | '+' | '-' => { is_float = true; tok.push(c); self.pos += 1; }
                        _ => break,
                    }
                }
                let clean: String = tok.chars().filter(|c| *c != '_').collect();
                if is_float {
                    clean.parse::<f64>().map(Value::Float).map_err(|_| self.err(format!("invalid number `{tok}`")))
                } else {
                    clean.parse::<i64>().map(Value::Integer).map_err(|_| self.err(format!("invalid number `{tok}`")))
                }
            }
            fn radix_integer(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let prefix = self.bump().unwrap();
                let radix = match prefix { 'x' => 16, 'o' => 8, 'b' => 2, _ => 16 };
                let mut tok = String::new();
                while let Some(c) = self.peek() {
                    if c == '_' { self.pos += 1; }
                    else if c.is_digit(radix) { tok.push(c); self.pos += 1; }
                    else { break; }
                }
                if tok.is_empty() {
                    return Err(self.err("expected digits after numeric base prefix"));
                }
                i64::from_str_radix(&tok, radix)
                    .map(Value::Integer)
                    .map_err(|_| self.err(format!("invalid base-{radix} integer `{tok}`")))
            }
        }

        fn is_bare_key_char(c: char) -> bool {
            c.is_ascii_alphanumeric() || c == '_' || c == '-'
        }
    }

    // ── core.encoding.yaml: full std-only YAML 1.2 core → DataTree (c152) ─────────
    // D-ENC-YAML1 = A: block mappings + sequences (indentation-driven), flow `{}`/`[]`,
    // core-schema typed scalars (null/~, bool, int, float, str), single/double-quoted
    // + plain + block scalars (`|` literal, `>` folded with chomping), comments,
    // `---`/`...` document markers, and anchors/aliases (`&a`/`*a`). Explicit/custom
    // tags (`!!str`, `!MyType`) are deferred to c153 (frozen). No external crates (I6).
    pub mod yaml {
        use super::DataTree;
        use std::collections::BTreeMap;

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ParseError {
            pub line: usize,
            pub message: String,
        }

        pub fn parse_to_tree(raw: &str) -> Result<DataTree, ParseError> {
            let lines: Vec<String> = raw.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
            let mut p = Parser { lines, pos: 0, anchors: BTreeMap::new() };
            p.skip_ignorable();
            // Leading document marker(s).
            while p.at_doc_marker() {
                p.pos += 1;
                p.skip_ignorable();
            }
            if p.pos >= p.lines.len() || p.at_doc_end() {
                return Ok(DataTree::Null);
            }
            let base = p.indent(p.pos);
            p.parse_node(base)
        }

        struct Parser {
            lines: Vec<String>,
            pos: usize,
            anchors: BTreeMap<String, DataTree>,
        }

        impl Parser {
            fn indent(&self, i: usize) -> usize {
                self.lines[i].chars().take_while(|c| *c == ' ').count()
            }
            // The line's content with leading indent removed and any trailing comment
            // stripped (a ` #` outside quotes, or a leading `#`).
            fn content(&self, i: usize) -> String {
                let after = &self.lines[i][self.indent(i)..];
                strip_comment(after)
            }
            fn is_ignorable(&self, i: usize) -> bool {
                let t = self.lines[i].trim();
                t.is_empty() || t.starts_with('#')
            }
            fn skip_ignorable(&mut self) {
                while self.pos < self.lines.len() && self.is_ignorable(self.pos) {
                    self.pos += 1;
                }
            }
            fn at_doc_marker(&self) -> bool {
                self.pos < self.lines.len() && self.lines[self.pos].trim_start().starts_with("---")
            }
            fn at_doc_end(&self) -> bool {
                self.pos < self.lines.len() && self.lines[self.pos].trim() == "..."
            }

            fn parse_node(&mut self, min_indent: usize) -> Result<DataTree, ParseError> {
                self.skip_ignorable();
                if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                    return Ok(DataTree::Null);
                }
                let ind = self.indent(self.pos);
                if ind < min_indent {
                    return Ok(DataTree::Null);
                }
                let content = self.content(self.pos);
                if content == "-" || content.starts_with("- ") {
                    self.parse_block_seq(ind)
                } else if is_map_entry(&content) {
                    self.parse_block_map(ind)
                } else {
                    // A bare scalar node (possibly anchored/aliased/flow/quoted).
                    self.pos += 1;
                    self.parse_inline_value(&content)
                }
            }

            fn parse_block_seq(&mut self, indent: usize) -> Result<DataTree, ParseError> {
                let mut items = Vec::new();
                loop {
                    self.skip_ignorable();
                    if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                        break;
                    }
                    let ind = self.indent(self.pos);
                    if ind != indent {
                        break;
                    }
                    let content = self.content(self.pos);
                    if content != "-" && !content.starts_with("- ") {
                        break;
                    }
                    // Dash trick: blank out `-` so the item content aligns as a normal
                    // block at indent+1, then parse it uniformly (scalar/map/seq).
                    let line = &mut self.lines[self.pos];
                    let bytes: Vec<char> = line.chars().collect();
                    // The dash sits at char index == indent (indentation is spaces only).
                    let mut rebuilt: String = bytes.iter().enumerate()
                        .map(|(i, c)| if i == indent { ' ' } else { *c })
                        .collect();
                    // If nothing follows the dash, leave a blank line.
                    if rebuilt.trim().is_empty() {
                        rebuilt = String::new();
                    }
                    *line = rebuilt;
                    let item = self.parse_node(indent + 1)?;
                    items.push(item);
                }
                Ok(DataTree::Array(items))
            }

            fn parse_block_map(&mut self, indent: usize) -> Result<DataTree, ParseError> {
                let mut entries: Vec<(String, DataTree)> = Vec::new();
                loop {
                    self.skip_ignorable();
                    if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                        break;
                    }
                    let ind = self.indent(self.pos);
                    if ind != indent {
                        break;
                    }
                    let content = self.content(self.pos);
                    if content.starts_with("- ") || content == "-" || !is_map_entry(&content) {
                        break;
                    }
                    let line_no = self.pos + 1;
                    let (key, rest) = split_key(&content)
                        .ok_or_else(|| ParseError { line: line_no, message: "expected `key: value`".into() })?;
                    self.pos += 1;
                    let rest = rest.trim();
                    let value = if rest.is_empty() {
                        // Nested block (deeper indent) or empty → Null.
                        self.skip_ignorable();
                        if self.pos < self.lines.len()
                            && self.indent(self.pos) > indent
                            && !self.at_doc_marker()
                            && !self.at_doc_end()
                        {
                            self.parse_node(indent + 1)?
                        } else {
                            DataTree::Null
                        }
                    } else if rest.starts_with('|') || rest.starts_with('>') {
                        self.parse_block_scalar(indent, rest)
                    } else {
                        self.parse_inline_value(rest)?
                    };
                    entries.push((key, value));
                }
                Ok(DataTree::Object(entries))
            }

            // A `|`/`>` block scalar. Following lines more-indented than the key form the
            // body; dedent by the first body line's indent. `>` folds line breaks to spaces.
            fn parse_block_scalar(&mut self, parent_indent: usize, header: &str) -> DataTree {
                let folded = header.starts_with('>');
                let chomp = if header.contains('-') { 'S' } else if header.contains('+') { 'K' } else { 'C' };
                let mut body_lines: Vec<String> = Vec::new();
                let mut block_indent: Option<usize> = None;
                while self.pos < self.lines.len() {
                    let raw = &self.lines[self.pos];
                    if raw.trim().is_empty() {
                        body_lines.push(String::new());
                        self.pos += 1;
                        continue;
                    }
                    let ind = self.indent(self.pos);
                    if ind <= parent_indent {
                        break;
                    }
                    let bi = *block_indent.get_or_insert(ind);
                    let chars: Vec<char> = raw.chars().collect();
                    let start = bi.min(chars.len());
                    let dedented: String = chars[start..].iter().collect();
                    body_lines.push(dedented);
                    self.pos += 1;
                }
                // Drop trailing blank lines for chomping decisions.
                let mut text = if folded {
                    fold_lines(&body_lines)
                } else {
                    body_lines.join("\n")
                };
                let trimmed = text.trim_end_matches('\n').to_string();
                text = match chomp {
                    'S' => trimmed,                       // strip: no trailing newline
                    'K' => text.trim_end_matches('\n').to_string() + "\n", // keep (simplified to one)
                    _ => trimmed + "\n",                  // clip: single trailing newline
                };
                DataTree::Text(text)
            }

            fn parse_inline_value(&mut self, s: &str) -> Result<DataTree, ParseError> {
                let s = s.trim();
                // Anchor: `&name <value?>` — register the parsed value under `name`.
                if let Some(rest) = s.strip_prefix('&') {
                    let mut it = rest.splitn(2, char::is_whitespace);
                    let name = it.next().unwrap_or("").to_string();
                    let val_str = it.next().unwrap_or("").trim();
                    let value = if val_str.is_empty() {
                        // The value is a nested block following this line.
                        self.parse_node(0)?
                    } else {
                        self.parse_inline_value(val_str)?
                    };
                    self.anchors.insert(name, value.clone());
                    return Ok(value);
                }
                // Alias: `*name`.
                if let Some(name) = s.strip_prefix('*') {
                    return Ok(self.anchors.get(name.trim()).cloned().unwrap_or(DataTree::Null));
                }
                if s.starts_with('[') {
                    return Ok(parse_flow(s).0);
                }
                if s.starts_with('{') {
                    return Ok(parse_flow(s).0);
                }
                Ok(scalar_value(s))
            }
        }

        // ── Flow `[...]` / `{...}` (single-line) ─────────────────────────────────
        fn parse_flow(s: &str) -> (DataTree, usize) {
            let chars: Vec<char> = s.chars().collect();
            parse_flow_at(&chars, 0)
        }
        fn parse_flow_at(chars: &[char], mut i: usize) -> (DataTree, usize) {
            while i < chars.len() && chars[i].is_whitespace() { i += 1; }
            if i >= chars.len() {
                return (DataTree::Null, i);
            }
            match chars[i] {
                '[' => {
                    i += 1;
                    let mut items = Vec::new();
                    loop {
                        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') { i += 1; }
                        if i >= chars.len() || chars[i] == ']' { i += 1; break; }
                        let (v, ni) = parse_flow_at(chars, i);
                        items.push(v);
                        i = ni;
                        while i < chars.len() && chars[i].is_whitespace() { i += 1; }
                        if i < chars.len() && chars[i] == ',' { i += 1; }
                        else if i < chars.len() && chars[i] == ']' { i += 1; break; }
                    }
                    (DataTree::Array(items), i)
                }
                '{' => {
                    i += 1;
                    let mut entries = Vec::new();
                    loop {
                        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') { i += 1; }
                        if i >= chars.len() || chars[i] == '}' { i += 1; break; }
                        // key up to ':'
                        let (key, ni) = scan_flow_scalar(chars, i, true);
                        i = ni;
                        while i < chars.len() && chars[i].is_whitespace() { i += 1; }
                        if i < chars.len() && chars[i] == ':' { i += 1; }
                        let (v, nj) = parse_flow_at(chars, i);
                        i = nj;
                        entries.push((key.trim().to_string(), v));
                        while i < chars.len() && chars[i].is_whitespace() { i += 1; }
                        if i < chars.len() && chars[i] == ',' { i += 1; }
                        else if i < chars.len() && chars[i] == '}' { i += 1; break; }
                    }
                    (DataTree::Object(entries), i)
                }
                _ => {
                    let (raw, ni) = scan_flow_scalar(chars, i, false);
                    (scalar_value(raw.trim()), ni)
                }
            }
        }
        // Read a flow scalar (until `,`/`]`/`}`/`:` when key) honoring quotes.
        fn scan_flow_scalar(chars: &[char], mut i: usize, as_key: bool) -> (String, usize) {
            while i < chars.len() && chars[i].is_whitespace() { i += 1; }
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let q = chars[i];
                let mut out = String::new();
                i += 1;
                while i < chars.len() {
                    if chars[i] == q {
                        if q == '\'' && i + 1 < chars.len() && chars[i + 1] == '\'' {
                            out.push('\''); i += 2; continue;
                        }
                        i += 1; break;
                    }
                    if chars[i] == '\\' && q == '"' && i + 1 < chars.len() {
                        out.push(unescape(chars[i + 1])); i += 2; continue;
                    }
                    out.push(chars[i]); i += 1;
                }
                return (out, i);
            }
            let mut out = String::new();
            while i < chars.len() {
                let c = chars[i];
                if c == ',' || c == ']' || c == '}' { break; }
                if as_key && c == ':' { break; }
                out.push(c); i += 1;
            }
            (out, i)
        }

        fn fold_lines(lines: &[String]) -> String {
            // `>` folding: blank lines become newlines; consecutive non-blank lines join
            // with a single space.
            let mut out = String::new();
            let mut prev_blank = true;
            for l in lines {
                if l.trim().is_empty() {
                    out.push('\n');
                    prev_blank = true;
                } else {
                    if !prev_blank { out.push(' '); }
                    out.push_str(l);
                    prev_blank = false;
                }
            }
            out
        }

        fn unescape(c: char) -> char {
            match c {
                'n' => '\n', 't' => '\t', 'r' => '\r', '0' => '\0',
                '\\' => '\\', '"' => '"', _ => c,
            }
        }

        // Strip a trailing ` #...` comment that is outside quotes, or a leading `#`.
        fn strip_comment(s: &str) -> String {
            let chars: Vec<char> = s.chars().collect();
            let mut in_s = false;
            let mut in_d = false;
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                match c {
                    '\'' if !in_d => in_s = !in_s,
                    '"' if !in_s => in_d = !in_d,
                    '#' if !in_s && !in_d && (i == 0 || chars[i - 1] == ' ' || chars[i - 1] == '\t') => {
                        let kept: String = chars[..i].iter().collect();
                        return kept.trim_end().to_string();
                    }
                    _ => {}
                }
                i += 1;
            }
            s.trim_end().to_string()
        }

        // Is this content a `key: value` mapping entry (top-level `:` outside flow/quotes)?
        fn is_map_entry(s: &str) -> bool {
            top_level_colon(s).is_some()
        }
        fn top_level_colon(s: &str) -> Option<usize> {
            let chars: Vec<char> = s.chars().collect();
            let mut in_s = false;
            let mut in_d = false;
            let mut depth = 0i32;
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                match c {
                    '\'' if !in_d => in_s = !in_s,
                    '"' if !in_s => in_d = !in_d,
                    '[' | '{' if !in_s && !in_d => depth += 1,
                    ']' | '}' if !in_s && !in_d => depth -= 1,
                    ':' if !in_s && !in_d && depth == 0 => {
                        // A mapping `:` must be followed by space or end-of-line.
                        if i + 1 >= chars.len() || chars[i + 1] == ' ' {
                            return Some(i);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        fn split_key(s: &str) -> Option<(String, String)> {
            let idx = top_level_colon(s)?;
            let chars: Vec<char> = s.chars().collect();
            let key_raw: String = chars[..idx].iter().collect();
            let rest: String = chars[idx + 1..].iter().collect();
            Some((unquote_key(key_raw.trim()), rest))
        }
        fn unquote_key(k: &str) -> String {
            if (k.starts_with('"') && k.ends_with('"') && k.len() >= 2)
                || (k.starts_with('\'') && k.ends_with('\'') && k.len() >= 2)
            {
                k[1..k.len() - 1].to_string()
            } else {
                k.to_string()
            }
        }

        // Type a plain/quoted scalar by the YAML core schema.
        fn scalar_value(s: &str) -> DataTree {
            let s = s.trim();
            if s.is_empty() {
                return DataTree::Null;
            }
            // Quoted strings are always text.
            if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                let inner = &s[1..s.len() - 1];
                let mut out = String::new();
                let mut chars = inner.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        if let Some(n) = chars.next() { out.push(unescape(n)); }
                    } else {
                        out.push(c);
                    }
                }
                return DataTree::Text(out);
            }
            if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
                return DataTree::Text(s[1..s.len() - 1].replace("''", "'"));
            }
            match s {
                "null" | "Null" | "NULL" | "~" => return DataTree::Null,
                "true" | "True" | "TRUE" => return DataTree::Bool(true),
                "false" | "False" | "FALSE" => return DataTree::Bool(false),
                ".inf" | ".Inf" | ".INF" => return DataTree::Float(f64::INFINITY),
                "-.inf" | "-.Inf" => return DataTree::Float(f64::NEG_INFINITY),
                ".nan" | ".NaN" | ".NAN" => return DataTree::Float(f64::NAN),
                _ => {}
            }
            // Integer (decimal, with optional sign).
            if let Ok(n) = s.parse::<i64>() {
                return DataTree::Int(n);
            }
            // Float: must contain a `.`, `e`/`E` and parse cleanly.
            if (s.contains('.') || s.contains('e') || s.contains('E'))
                && s.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
            {
                if let Ok(f) = s.parse::<f64>() {
                    return DataTree::Float(f);
                }
            }
            DataTree::Text(s.to_string())
        }

        // ── Render: a `DataTree` → block YAML text ───────────────────────────────
        pub fn render(t: &DataTree) -> String {
            let mut out = String::new();
            render_node(t, 0, &mut out);
            let s = out.trim_end().to_string();
            if s.is_empty() { "{}".to_string() } else { s }
        }
        fn render_node(t: &DataTree, indent: usize, out: &mut String) {
            let pad = " ".repeat(indent);
            match t {
                DataTree::Object(entries) => {
                    if entries.is_empty() {
                        out.push_str(&format!("{}{{}}\n", pad));
                        return;
                    }
                    for (k, v) in entries {
                        match v {
                            DataTree::Object(e) if !e.is_empty() => {
                                out.push_str(&format!("{}{}:\n", pad, render_key(k)));
                                render_node(v, indent + 2, out);
                            }
                            DataTree::Array(a) if !a.is_empty() => {
                                out.push_str(&format!("{}{}:\n", pad, render_key(k)));
                                render_seq(a, indent, out);
                            }
                            _ => {
                                out.push_str(&format!("{}{}: {}\n", pad, render_key(k), render_scalar(v)));
                            }
                        }
                    }
                }
                DataTree::Array(items) => render_seq(items, indent, out),
                _ => out.push_str(&format!("{}{}\n", pad, render_scalar(t))),
            }
        }
        fn render_seq(items: &[DataTree], indent: usize, out: &mut String) {
            let pad = " ".repeat(indent);
            for item in items {
                match item {
                    DataTree::Object(e) if !e.is_empty() => {
                        out.push_str(&format!("{}-\n", pad));
                        render_node(item, indent + 2, out);
                    }
                    DataTree::Array(a) if !a.is_empty() => {
                        out.push_str(&format!("{}-\n", pad));
                        render_seq(a, indent + 2, out);
                    }
                    _ => out.push_str(&format!("{}- {}\n", pad, render_scalar(item))),
                }
            }
        }
        fn render_key(k: &str) -> String {
            if k.is_empty() || k.contains(':') || k.contains(' ') || k.contains('#') {
                format!("{:?}", k)
            } else {
                k.to_string()
            }
        }
        fn render_scalar(v: &DataTree) -> String {
            match v {
                DataTree::Null => "null".to_string(),
                DataTree::Bool(b) => b.to_string(),
                DataTree::Int(n) => n.to_string(),
                DataTree::Float(f) => format!("{:?}", f),
                DataTree::Text(s) => {
                    if needs_quote(s) { format!("{:?}", s) } else { s.clone() }
                }
                DataTree::Bytes(bs) => {
                    let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                    format!("[{}]", parts.join(", "))
                }
                // An inline collection value renders in flow form.
                DataTree::Array(items) => {
                    let parts: Vec<String> = items.iter().map(render_scalar).collect();
                    format!("[{}]", parts.join(", "))
                }
                DataTree::Object(entries) => {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, val)| format!("{}: {}", render_key(k), render_scalar(val)))
                        .collect();
                    format!("{{{}}}", parts.join(", "))
                }
            }
        }
        fn needs_quote(s: &str) -> bool {
            if s.is_empty() { return true; }
            matches!(s, "null" | "Null" | "NULL" | "~" | "true" | "True" | "TRUE"
                | "false" | "False" | "FALSE")
                || s.parse::<i64>().is_ok()
                || s.parse::<f64>().is_ok()
                || s.starts_with(' ') || s.ends_with(' ')
                || s.starts_with(['-', '?', ':', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`'])
                || s.contains(": ") || s.contains(" #") || s.contains('\n')
        }
    }
}

// ── View<T> (D-DYNARRAY1) ────────────────────────────────────────────────────
// `list.view(a..b)` is a zero-copy window: unlike every bridge type below,
// `View<T>` has no owning Rust struct here — it lowers straight to a plain
// borrowed slice `&[T]` (`Context::rust_type`'s `View` arm, crates/jet-codegen/
// src/Codegen/Context.rs), and its constructor/method helpers
// (`jet_view_new`/`jet_view_fold`/`jet_view_map`) live in Core.rs next to
// `jet_slice_vec`/`jet_list_fold` — the same bare (non-`jet_std::`-namespaced)
// family every other list method belongs to, since `.view(...)` dispatches
// through the ordinary list-method machinery, not the handle-type dispatch
// the structs below use. Ownership (the window cannot outlive its list) is
// proved by sema's E2305, not by a Rust lifetime parameter on a wrapper type.

// ── Streaming file handles (E2-M7, D-IO2) ────────────────────────────────────
// FileReader / FileWriter are RAII: Drop closes (and flushes) them
// on every exit path — including `?` early returns and panics.
struct JetFileReader {
    inner: std::io::BufReader<std::fs::File>,
    path: String,
}
struct JetFileWriter {
    inner: std::io::BufWriter<std::fs::File>,
    path: String,
}

// ── core.db connection handle (D-DBDRIVER1) ──────────────────────────────────
// The real SQLite connection lives in the FFI bridge crate's thread-local
// handle map (`rusqlite::Connection` can't cross into this always-compiled
// prelude — I6). `JetDbConnection` is a thin, `Copy` handle wrapper so
// `.query`/`.execute`/`.begin`/`.commit`/`.rollback`/`.close` dispatch by
// receiver TYPE (`DbConnection`), the same mechanism `FileReader`/`FileWriter`
// use, instead of exposing the bare `u64` to Jet code.
#[derive(Clone, Copy, Debug)]
struct JetDbConnection {
    handle: u64,
}

// ── Typed Path API (D-PATHFS1) ────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct JetPath {
    inner: std::path::PathBuf,
}
impl JetShow for JetPath {
    fn jet_show(&self) -> String { self.inner.to_string_lossy().to_string() }
}
fn jet_path_from(s: &String) -> JetPath {
    JetPath { inner: std::path::PathBuf::from(s) }
}
fn jet_path_join(p: &JetPath, other: &String) -> JetPath {
    JetPath { inner: p.inner.join(other.as_str()) }
}
fn jet_path_parent(p: &JetPath) -> Option<JetPath> {
    p.inner.parent().map(|par| JetPath { inner: par.to_path_buf() })
}
fn jet_path_extension(p: &JetPath) -> Option<String> {
    p.inner.extension().map(|e| e.to_string_lossy().to_string())
}
fn jet_path_stem(p: &JetPath) -> Option<String> {
    p.inner.file_stem().map(|s| s.to_string_lossy().to_string())
}
fn jet_path_write_atomic(p: &JetPath, content: &Vec<u8>) -> Result<(), jet_std::IoError> {
    let path_s = p.inner.to_string_lossy();
    let dir = p.inner.parent().ok_or_else(|| {
        jet_std::io_error(&path_s, std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent directory"))
    })?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".jet_tmp_{}", nanos));
    std::fs::write(&tmp, content).map_err(|e| jet_std::io_error(tmp.to_string_lossy().as_ref(), e))?;
    std::fs::rename(&tmp, &p.inner).map_err(|e| jet_std::io_error(&path_s, e))
}
fn jet_path_walk(p: &JetPath) -> Vec<JetPath> {
    let mut result = Vec::new();
    let mut stack = vec![p.inner.clone()];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        let canonical = match std::fs::canonicalize(&dir) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !visited.insert(canonical) {
            continue; // symlink loop — skip
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            result.push(JetPath { inner: path.clone() });
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    result
}
// ─────────────────────────────────────────────────────────────────────────────

fn jet_std_files_open(path: &String) -> Result<JetFileReader, jet_std::IoError> {
    let f = std::fs::File::open(path).map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileReader { inner: std::io::BufReader::new(f), path: path.clone() })
}
fn jet_std_files_create(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::File::create(path).map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileWriter { inner: std::io::BufWriter::new(f), path: path.clone() })
}
fn jet_std_files_append(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::OpenOptions::new()
        .create(true).append(true).open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileWriter { inner: std::io::BufWriter::new(f), path: path.clone() })
}
fn jet_std_file_reader_read_line(r: &mut JetFileReader) -> Result<Option<String>, jet_std::IoError> {
    use std::io::BufRead;
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') { line.pop(); }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::io_error(&r.path, e)),
    }
}
fn jet_std_file_writer_write_line(w: &mut JetFileWriter, line: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner.write_all(line.as_bytes()).and_then(|_| w.inner.write_all(b"\n"))
        .map_err(|e| jet_std::io_error(&w.path, e))
}
fn jet_std_file_writer_flush(w: &mut JetFileWriter) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner.flush().map_err(|e| jet_std::io_error(&w.path, e))
}

// ── std.path helpers (D-IO1) ──────────────────────────────────────────────────
fn jet_std_path_join(base: &String, part: &String) -> String {
    let b = std::path::Path::new(base.as_str());
    b.join(part.as_str()).to_string_lossy().to_string()
}
fn jet_std_path_parent(path: &String) -> String {
    std::path::Path::new(path.as_str())
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_path_extension(path: &String) -> String {
    std::path::Path::new(path.as_str())
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_path_normalize(path: &String) -> String {
    // Resolve `.` and `..` components without hitting the filesystem.
    let mut parts: Vec<&str> = Vec::new();
    let s = path.as_str();
    let absolute = s.starts_with('/');
    for seg in s.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop(); }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute { format!("/{}", joined) } else { joined }
}

// ── core.text.unicode helpers (D-TEXTUNICODE1) ───────────────────────────────
fn jet_text_unicode_scalar_count(s: &String) -> i64 {
    s.chars().count() as i64
}
fn jet_text_unicode_byte_count(s: &String) -> i64 {
    s.len() as i64
}
fn jet_text_unicode_is_ascii(s: &String) -> bool {
    s.is_ascii()
}
fn jet_text_unicode_lower(s: &String) -> String {
    s.to_lowercase()
}
fn jet_text_unicode_upper(s: &String) -> String {
    s.to_uppercase()
}
fn jet_text_unicode_scalars(s: &String) -> Vec<String> {
    s.chars().map(|c| c.to_string()).collect()
}

fn jet_std_fs_read(path: &String) -> Result<String, jet_std::IoError> {
    std::fs::read_to_string(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_read_bytes(path: &String) -> Result<Vec<u8>, jet_std::IoError> {
    std::fs::read(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_write(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    std::fs::write(path, text).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_append(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    f.write_all(text.as_bytes()).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_exists(path: &String) -> bool { std::path::Path::new(path).exists() }
fn jet_std_fs_remove(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::remove_file(path).map_err(|e| jet_std::io_error(path, e))
}
// D-LSDIR1=A: returns DirEntry values with name, full path, and is_dir flag.
fn jet_std_fs_list_dir(path: &String) -> Result<Vec<jet_std::DirEntry>, jet_std::IoError> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(path).map_err(|e| jet_std::io_error(path, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| jet_std::io_error(path, e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = std::path::Path::new(path.as_str())
            .join(&name)
            .to_string_lossy()
            .to_string();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        out.push(jet_std::DirEntry { name, path: full_path, is_dir });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
fn jet_std_fs_create_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::create_dir_all(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_is_dir(path: &String) -> bool { std::path::Path::new(path).is_dir() }
fn jet_std_fs_copy(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| jet_std::io_error(from, e))
}
fn jet_std_fs_rename(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::rename(from, to).map_err(|e| jet_std::io_error(from, e))
}

fn jet_std_io_args() -> Vec<String> { std::env::args().collect() }
fn jet_std_io_input(prompt: Option<&String>) -> Result<String, jet_std::IoError> {
    use std::io::Write;
    if let Some(p) = prompt {
        print!("{}", p);
        std::io::stdout().flush().map_err(|e| jet_std::IoError::Other { message: e.to_string() })?;
    }
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| jet_std::IoError::Other { message: e.to_string() })?;
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    Ok(s)
}
fn jet_std_io_read_all_input() -> Result<String, jet_std::IoError> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .map_err(|e| jet_std::IoError::Other { message: e.to_string() })?;
    Ok(s)
}

// D-STDIN1=A: streaming line-by-line stdin.
struct JetStdinReader {
    inner: std::io::BufReader<std::io::Stdin>,
}
fn jet_std_io_stdin() -> JetStdinReader {
    JetStdinReader { inner: std::io::BufReader::new(std::io::stdin()) }
}
fn jet_std_io_stdin_read_line(r: &mut JetStdinReader) -> Result<Option<String>, jet_std::IoError> {
    use std::io::BufRead;
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') { line.pop(); }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::IoError::Other { message: e.to_string() }),
    }
}

fn jet_std_env_get(name: &String) -> Option<String> { std::env::var(name).ok() }
fn jet_std_env_set(name: &String, value: &String) { std::env::set_var(name, value); }
fn jet_std_env_current_dir() -> Result<String, jet_std::IoError> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::IoError::Other { message: e.to_string() })
}
fn jet_std_env_home_dir() -> Option<String> {
    std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok())
}

fn jet_std_process_exit(code: i64) -> ! { std::process::exit(code as i32) }
fn jet_std_process_run(cmd: &Vec<String>) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    if cmd.is_empty() {
        return Err(jet_std::IoError::Other { message: "process.run needs at least one command word".to_string() });
    }
    let out = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .output()
        .map_err(|e| jet_std::IoError::Other { message: e.to_string() })?;
    Ok(jet_std::ProcessResult {
        code: out.status.code().unwrap_or(-1) as i64,
        output: String::from_utf8_lossy(&out.stdout).to_string(),
        errors: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

fn jet_std_math_sqrt(x: f64) -> f64 { x.sqrt() }
fn jet_std_math_pow(a: f64, b: f64) -> f64 { a.powf(b) }
fn jet_std_math_floor(x: f64) -> f64 { x.floor() }
fn jet_std_math_ceil(x: f64) -> f64 { x.ceil() }
fn jet_std_math_round(x: f64) -> i64 { x.round() as i64 }
// D-FLOATW1 (ratified 2026-06-22): F32 variants — sqrt(F32)->F32, pow(F32,F32)->F32 etc.
// F32 is a real precision choice, not just storage; no silent widening to f64 (I3).
fn jet_std_math_sqrt_f32(x: f32) -> f32 { x.sqrt() }
fn jet_std_math_pow_f32(a: f32, b: f32) -> f32 { a.powf(b) }
fn jet_std_math_floor_f32(x: f32) -> f32 { x.floor() }
fn jet_std_math_ceil_f32(x: f32) -> f32 { x.ceil() }

thread_local! { static JET_RNG: std::cell::Cell<u64> = std::cell::Cell::new(0x4d595df4d0f33173); }
fn jet_rng_next() -> u64 {
    JET_RNG.with(|cell| {
        let mut x = cell.get();
        x ^= x << 7;
        x ^= x >> 9;
        x = x.wrapping_mul(0x9e3779b97f4a7c15);
        cell.set(x);
        x
    })
}
fn jet_std_random_seed(n: i64) { JET_RNG.with(|cell| cell.set(n as u64)); }
fn jet_std_random_int(low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (jet_rng_next() % ((high - low + 1) as u64)) as i64
}
fn jet_std_random_float() -> f64 { (jet_rng_next() as f64) / (u64::MAX as f64) }
fn jet_std_random_pick<T: Clone>(xs: &Vec<T>) -> Option<T> {
    if xs.is_empty() { None } else { Some(xs[jet_std_random_int(0, xs.len() as i64 - 1) as usize].clone()) }
}
fn jet_std_random_shuffle<T>(xs: &mut Vec<T>) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = jet_std_random_int(0, i as i64) as usize;
        xs.swap(i, j);
    }
}
// D-RANDSPLIT1=A: PRNG bytes via the ambient SplitMix64 state — fast, seedable,
// NOT cryptographically secure. Use for simulation, testing, or shuffles only.
fn jet_std_random_bytes(n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(jet_rng_next() as u8);
    }
    out
}
// D-RANDSPLIT1=A: CSPRNG bytes via /dev/urandom (POSIX) with SplitMix64 fallback.
// Cryptographically secure — use for tokens, keys, nonces, and secrets.
fn jet_std_crypto_random_bytes(n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = vec![0u8; n];
    jet_uuid_fill_random(&mut out);
    out
}

fn jet_std_time_now() -> i64 {
    if let Ok(s) = std::env::var("LEX_TEST_EPOCH") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

thread_local! {
    static JET_CTX_DEADLINE_MS: std::cell::Cell<Option<i64>> = std::cell::Cell::new(None);
}

struct JetDeadlineGuard {
    saved: Option<i64>,
}

impl Drop for JetDeadlineGuard {
    fn drop(&mut self) {
        JET_CTX_DEADLINE_MS.with(|c| c.set(self.saved));
    }
}

fn jet_ctx_deadline_ms() -> Option<i64> {
    JET_CTX_DEADLINE_MS.with(|c| c.get())
}

fn jet_ctx_push_deadline(deadline_ms: i64) -> JetDeadlineGuard {
    let saved = JET_CTX_DEADLINE_MS.with(|c| c.get());
    JET_CTX_DEADLINE_MS.with(|c| c.set(Some(deadline_ms)));
    JetDeadlineGuard { saved }
}

fn jet_deadline_remaining_ms() -> Option<i64> {
    let deadline = jet_ctx_deadline_ms()?;
    Some(deadline.saturating_sub(jet_std_time_now()))
}

fn jet_deadline_exceeded(wait_kind: &str) -> ! {
    eprintln!("Error [E3003]: deadline exceeded while waiting in {wait_kind}");
    eprintln!("Why: this wait point observed the task context deadline from `#Context(deadline: …)`");
    eprintln!("Fix: raise the deadline budget or shorten the work before this wait point");
    std::process::exit(70);
}

fn jet_deadline_check(wait_kind: &str) {
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        jet_deadline_exceeded(wait_kind);
    }
}

fn jet_std_time_sleep(millis: i64) {
    let want = millis.max(0);
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("time sleep");
        }
        if want > remaining {
            jet_scheduler_sleep_ms(remaining as u64);
            jet_deadline_exceeded("time sleep");
        }
    }
    jet_scheduler_sleep_ms(want as u64);
    jet_deadline_check("time sleep");
}
fn jet_std_time_start() -> jet_std::Stopwatch {
    jet_std::Stopwatch { start: std::time::Instant::now() }
}

// ── D-DET1: deterministic injected Clock / Rng capabilities ───────────────────
// Built from a caller-supplied seed (a pure value), so a `@Pure fn` may read
// time/randomness THROUGH the handle and stay reproducible. No wall-clock or
// OS-RNG read; std-only (no external crate, I6).
fn jet_std_clock_new(seed: i64) -> jet_std::Clock {
    jet_std::Clock { now: seed }
}
fn jet_clock_now(c: &jet_std::Clock) -> i64 {
    c.now
}
fn jet_clock_tick(c: &mut jet_std::Clock, ms: i64) -> i64 {
    c.now = c.now.wrapping_add(ms);
    c.now
}
// D-DET-CAPAPI: `clock.advance(to_ms)` sets the clock to an ABSOLUTE instant;
// `clock.wait(d)` advances by a `Duration` (relative). Both return the new value.
fn jet_clock_advance(c: &mut jet_std::Clock, to_ms: i64) -> i64 {
    c.now = to_ms;
    c.now
}
fn jet_clock_wait(c: &mut jet_std::Clock, d: &jet_std::Duration) -> i64 {
    c.now = c.now.wrapping_add(d.ms);
    c.now
}
fn jet_std_rng_new(seed: i64) -> jet_std::Rng {
    jet_std::Rng { state: seed as u64 }
}
// SplitMix64 step — a small, well-distributed deterministic PRNG (public domain).
fn jet_det_rng_next(r: &mut jet_std::Rng) -> u64 {
    r.state = r.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = r.state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
fn jet_rng_int(r: &mut jet_std::Rng, lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let span = (hi - lo + 1) as u64;
    lo + (jet_det_rng_next(r) % span) as i64
}
fn jet_rng_float(r: &mut jet_std::Rng) -> f64 {
    // 53-bit mantissa → [0, 1).
    (jet_det_rng_next(r) >> 11) as f64 / (1u64 << 53) as f64
}
// D-DET-CAPAPI: the widened deterministic draws — coin, uniform choice, in-place
// Fisher–Yates shuffle. Each advances the SplitMix64 stream, so they are
// reproducible from the seed and mirror the ambient `random.*` set.
fn jet_rng_bool(r: &mut jet_std::Rng) -> bool {
    (jet_det_rng_next(r) & 1) == 1
}
fn jet_rng_pick<T: Clone>(r: &mut jet_std::Rng, xs: &Vec<T>) -> Option<T> {
    if xs.is_empty() {
        None
    } else {
        Some(xs[jet_rng_int(r, 0, xs.len() as i64 - 1) as usize].clone())
    }
}
fn jet_rng_shuffle<T>(r: &mut jet_std::Rng, xs: &mut Vec<T>) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = jet_rng_int(r, 0, i as i64) as usize;
        xs.swap(i, j);
    }
}
// D-DET-CAPAPI: `Duration` constructors + read. Pure value ops, ms-based.
fn jet_std_duration_ms(n: i64) -> jet_std::Duration {
    jet_std::Duration { ms: n }
}
fn jet_std_duration_secs(n: i64) -> jet_std::Duration {
    jet_std::Duration { ms: n.wrapping_mul(1000) }
}
fn jet_duration_millis(d: &jet_std::Duration) -> i64 {
    d.ms
}

// D-BIGINT1 / D-DECIMAL1: precise numeric constructors and methods.
fn jet_bigint_from_int(n: i64) -> jet_std::JetBigInt {
    jet_std::JetBigInt::from_int(n)
}
fn jet_bigint_from_str(s: &String) -> jet_std::JetBigInt {
    jet_std::JetBigInt::from_str(s).expect("invalid BigInt string")
}
fn jet_bigint_add(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.add(b)
}
fn jet_bigint_sub(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.sub(b)
}
fn jet_bigint_mul(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.mul(b)
}
fn jet_bigint_neg(a: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.neg()
}
fn jet_bigint_to_string(a: &jet_std::JetBigInt) -> String {
    a.to_string_rep()
}
fn jet_decimal_from_str(s: &String) -> jet_std::JetDecimal {
    jet_std::JetDecimal::from_str(s).expect("invalid Decimal string")
}
fn jet_decimal_add(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.add(b)
}
fn jet_decimal_sub(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.sub(b)
}
fn jet_decimal_mul(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.mul(b)
}
fn jet_decimal_to_string(a: &jet_std::JetDecimal) -> String {
    a.to_string_rep()
}

// D-ENC-DYN1=A+: the dynamic `parse` returns the one rich `Data` value (the
// user-facing face of `DataTree`). JSON text parses through the internal `Json`
// enum, then collapses onto `DataTree` (integral numbers become `Int`, fractional
// `Float`). Object keys arrive in sorted order (the internal `Json` enum is
// `BTreeMap`-keyed), matching the pre-`Data` dynamic JSON behavior.
fn jet_std_json_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    jet_std::parse_json(text).map(|j| jet_std::datatree_from_json(&j))
}
fn jet_std_json_render(d: &jet_std::DataTree) -> String { jet_std::render_datatree_json(d, false, 0) }
fn jet_std_json_render_pretty(d: &jet_std::DataTree) -> String { jet_std::render_datatree_json(d, true, 0) }

// D-JSON1-decode + D-JSON3: lenient JSON decode with coercion surfacing.
// Parses `text`, then walks the result. Any JSON string that looks like a
// number or boolean is coerced to that type; one log line is emitted per
// coercion naming the field and the from→to types. The coerced value collapses
// onto `Data` (D-ENC-DYN1=A+).
fn jet_std_json_decode_lenient(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    let parsed = jet_std::parse_json(text)?;
    Ok(jet_std::datatree_from_json(&jet_std_json_coerce_walk(&parsed, "")))
}

fn jet_std_json_coerce_walk(value: &jet_std::Json, path: &str) -> jet_std::Json {
    match value {
        jet_std::Json::Text(s) => {
            // try bool first (exact match only)
            if s == "true" {
                jet_std_json_emit_coerce(path, "string", "boolean");
                return jet_std::Json::Boolean(true);
            }
            if s == "false" {
                jet_std_json_emit_coerce(path, "string", "boolean");
                return jet_std::Json::Boolean(false);
            }
            // try number (must parse as valid f64 and round-trip cleanly)
            if let Ok(n) = s.parse::<f64>() {
                if n.is_finite() {
                    jet_std_json_emit_coerce(path, "string", "number");
                    return jet_std::Json::Number(n);
                }
            }
            value.clone()
        }
        jet_std::Json::Object(entries) => {
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in entries {
                let child_path = if path.is_empty() {
                    format!("{}", k)
                } else {
                    format!("{}.{}", path, k)
                };
                out.insert(k.clone(), jet_std_json_coerce_walk(v, &child_path));
            }
            jet_std::Json::Object(out)
        }
        jet_std::Json::Array(items) => {
            let coerced: Vec<jet_std::Json> = items.iter().enumerate().map(|(i, v)| {
                let child_path = if path.is_empty() {
                    format!("[{}]", i)
                } else {
                    format!("{}[{}]", path, i)
                };
                jet_std_json_coerce_walk(v, &child_path)
            }).collect();
            jet_std::Json::Array(coerced)
        }
        // Null, Boolean, Number — already the right type, no coercion.
        other => other.clone(),
    }
}

fn jet_std_json_emit_coerce(path: &str, from: &str, to: &str) {
    let field_label = if path.is_empty() { "<root>" } else { path };
    let msg = format!("json coerce: field \"{}\" {} \u{2192} {}", field_label, from, to);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    eprintln!("{{\"level\":\"info\",\"body\":\"{}\",\"ts\":{}}}", msg, ts);
}

fn jet_string_bytes(s: &String) -> Vec<u8> { s.as_bytes().to_vec() }
fn jet_string_from_bytes(bs: &Vec<u8>) -> Result<String, jet_std::Utf8Error> {
    String::from_utf8(bs.clone()).map_err(|e| jet_std::Utf8Error { message: e.to_string() })
}
fn jet_int_to_u8(n: i64) -> Result<u8, String> {
    if (0..=255).contains(&n) { Ok(n as u8) } else { Err("a U8 holds 0..255".to_string()) }
}
fn jet_stopwatch_elapsed_millis(sw: &jet_std::Stopwatch) -> i64 {
    sw.start.elapsed().as_millis() as i64
}

// ── D-SIMD2 / D-LINALG1: math value-type free functions ───────────────────────
// Constructors (`_new`), statics (`splat`/`from_array`), instance methods, lane
// reads, and reductions. Codegen names these `jet_math_<Type>_<fn>` and always
// passes the receiver as `&recv` (value types — every op returns a fresh value).
// Plain std math; no intrinsics, no `un`+`safe`.

fn jet_math_F32x4_new(a: f32, b: f32, c: f32, d: f32) -> jet_std::F32x4 { jet_std::F32x4([a, b, c, d]) }
fn jet_math_F64x2_new(a: f64, b: f64) -> jet_std::F64x2 { jet_std::F64x2([a, b]) }
fn jet_math_F32x4_splat(x: f32) -> jet_std::F32x4 { jet_std::F32x4([x; 4]) }
fn jet_math_F64x2_splat(x: f64) -> jet_std::F64x2 { jet_std::F64x2([x; 2]) }
fn jet_math_F32x4_from_array(a: [f32; 4]) -> jet_std::F32x4 { jet_std::F32x4(a) }
fn jet_math_F64x2_from_array(a: [f64; 2]) -> jet_std::F64x2 { jet_std::F64x2(a) }
fn jet_math_F32x4_to_array(v: &jet_std::F32x4) -> [f32; 4] { v.0 }
fn jet_math_F64x2_to_array(v: &jet_std::F64x2) -> [f64; 2] { v.0 }

fn jet_math_F32x4_lane(v: &jet_std::F32x4, i: i64, file: &str, line: u32) -> f32 {
    if i < 0 || i as usize >= 4 { jet_panic(file, line, &format!("lane index {} out of range for F32x4 (4 lanes)", i)); }
    v.0[i as usize]
}
fn jet_math_F64x2_lane(v: &jet_std::F64x2, i: i64, file: &str, line: u32) -> f64 {
    if i < 0 || i as usize >= 2 { jet_panic(file, line, &format!("lane index {} out of range for F64x2 (2 lanes)", i)); }
    v.0[i as usize]
}

fn jet_math_F32x4_sum(v: &jet_std::F32x4) -> f32 { v.0.iter().sum() }
fn jet_math_F32x4_product(v: &jet_std::F32x4) -> f32 { v.0.iter().product() }
fn jet_math_F32x4_min(v: &jet_std::F32x4) -> f32 { v.0.iter().copied().fold(f32::INFINITY, f32::min) }
fn jet_math_F32x4_max(v: &jet_std::F32x4) -> f32 { v.0.iter().copied().fold(f32::NEG_INFINITY, f32::max) }
fn jet_math_F32x4_reduce_add(v: &jet_std::F32x4) -> f32 { jet_math_F32x4_sum(v) }
fn jet_math_F32x4_reduce_mul(v: &jet_std::F32x4) -> f32 { jet_math_F32x4_product(v) }
fn jet_math_F32x4_reduce_min(v: &jet_std::F32x4) -> f32 { jet_math_F32x4_min(v) }
fn jet_math_F32x4_reduce_max(v: &jet_std::F32x4) -> f32 { jet_math_F32x4_max(v) }

fn jet_math_F64x2_sum(v: &jet_std::F64x2) -> f64 { v.0.iter().sum() }
fn jet_math_F64x2_product(v: &jet_std::F64x2) -> f64 { v.0.iter().product() }
fn jet_math_F64x2_min(v: &jet_std::F64x2) -> f64 { v.0.iter().copied().fold(f64::INFINITY, f64::min) }
fn jet_math_F64x2_max(v: &jet_std::F64x2) -> f64 { v.0.iter().copied().fold(f64::NEG_INFINITY, f64::max) }
fn jet_math_F64x2_reduce_add(v: &jet_std::F64x2) -> f64 { jet_math_F64x2_sum(v) }
fn jet_math_F64x2_reduce_mul(v: &jet_std::F64x2) -> f64 { jet_math_F64x2_product(v) }
fn jet_math_F64x2_reduce_min(v: &jet_std::F64x2) -> f64 { jet_math_F64x2_min(v) }
fn jet_math_F64x2_reduce_max(v: &jet_std::F64x2) -> f64 { jet_math_F64x2_max(v) }

// Vectors.
fn jet_math_Vec2_new(x: f64, y: f64) -> jet_std::Vec2 { jet_std::Vec2([x, y]) }
fn jet_math_Vec3_new(x: f64, y: f64, z: f64) -> jet_std::Vec3 { jet_std::Vec3([x, y, z]) }
fn jet_math_Vec4_new(x: f64, y: f64, z: f64, w: f64) -> jet_std::Vec4 { jet_std::Vec4([x, y, z, w]) }
fn jet_math_Vec2_splat(x: f64) -> jet_std::Vec2 { jet_std::Vec2([x; 2]) }
fn jet_math_Vec3_splat(x: f64) -> jet_std::Vec3 { jet_std::Vec3([x; 3]) }
fn jet_math_Vec4_splat(x: f64) -> jet_std::Vec4 { jet_std::Vec4([x; 4]) }
fn jet_math_Vec2_from_array(a: [f64; 2]) -> jet_std::Vec2 { jet_std::Vec2(a) }
fn jet_math_Vec3_from_array(a: [f64; 3]) -> jet_std::Vec3 { jet_std::Vec3(a) }
fn jet_math_Vec4_from_array(a: [f64; 4]) -> jet_std::Vec4 { jet_std::Vec4(a) }
fn jet_math_Vec2_to_array(v: &jet_std::Vec2) -> [f64; 2] { v.0 }
fn jet_math_Vec3_to_array(v: &jet_std::Vec3) -> [f64; 3] { v.0 }
fn jet_math_Vec4_to_array(v: &jet_std::Vec4) -> [f64; 4] { v.0 }

fn jet_math_Vec2_dot(v: &jet_std::Vec2, o: jet_std::Vec2) -> f64 { v.0[0] * o.0[0] + v.0[1] * o.0[1] }
fn jet_math_Vec3_dot(v: &jet_std::Vec3, o: jet_std::Vec3) -> f64 { v.0[0] * o.0[0] + v.0[1] * o.0[1] + v.0[2] * o.0[2] }
fn jet_math_Vec4_dot(v: &jet_std::Vec4, o: jet_std::Vec4) -> f64 { (0..4).map(|i| v.0[i] * o.0[i]).sum() }
fn jet_math_Vec3_cross(v: &jet_std::Vec3, o: jet_std::Vec3) -> jet_std::Vec3 {
    jet_std::Vec3([
        v.0[1] * o.0[2] - v.0[2] * o.0[1],
        v.0[2] * o.0[0] - v.0[0] * o.0[2],
        v.0[0] * o.0[1] - v.0[1] * o.0[0],
    ])
}
fn jet_math_Vec2_length(v: &jet_std::Vec2) -> f64 { jet_math_Vec2_dot(v, *v).sqrt() }
fn jet_math_Vec3_length(v: &jet_std::Vec3) -> f64 { jet_math_Vec3_dot(v, *v).sqrt() }
fn jet_math_Vec4_length(v: &jet_std::Vec4) -> f64 { jet_math_Vec4_dot(v, *v).sqrt() }
fn jet_math_Vec2_normalize(v: &jet_std::Vec2) -> jet_std::Vec2 {
    let l = jet_math_Vec2_length(v); if l == 0.0 { *v } else { jet_std::Vec2([v.0[0] / l, v.0[1] / l]) }
}
fn jet_math_Vec3_normalize(v: &jet_std::Vec3) -> jet_std::Vec3 {
    let l = jet_math_Vec3_length(v); if l == 0.0 { *v } else { jet_std::Vec3([v.0[0] / l, v.0[1] / l, v.0[2] / l]) }
}
fn jet_math_Vec4_normalize(v: &jet_std::Vec4) -> jet_std::Vec4 {
    let l = jet_math_Vec4_length(v); if l == 0.0 { *v } else { let mut r = v.0; for i in 0..4 { r[i] /= l; } jet_std::Vec4(r) }
}

// Matrices (column-major). Constructors take N*N components in column-major order.
fn jet_math_Mat3_new(
    m0: f64, m1: f64, m2: f64, m3: f64, m4: f64, m5: f64, m6: f64, m7: f64, m8: f64,
) -> jet_std::Mat3 { jet_std::Mat3([m0, m1, m2, m3, m4, m5, m6, m7, m8]) }
fn jet_math_Mat4_new(
    m0: f64, m1: f64, m2: f64, m3: f64, m4: f64, m5: f64, m6: f64, m7: f64,
    m8: f64, m9: f64, m10: f64, m11: f64, m12: f64, m13: f64, m14: f64, m15: f64,
) -> jet_std::Mat4 {
    jet_std::Mat4([m0, m1, m2, m3, m4, m5, m6, m7, m8, m9, m10, m11, m12, m13, m14, m15])
}
fn jet_math_Mat3_from_array(a: [f64; 9]) -> jet_std::Mat3 { jet_std::Mat3(a) }
fn jet_math_Mat4_from_array(a: [f64; 16]) -> jet_std::Mat4 { jet_std::Mat4(a) }
fn jet_math_Mat3_to_array(m: &jet_std::Mat3) -> [f64; 9] { m.0 }
fn jet_math_Mat4_to_array(m: &jet_std::Mat4) -> [f64; 16] { m.0 }
fn jet_math_Mat3_matmul(m: &jet_std::Mat3, o: jet_std::Mat3) -> jet_std::Mat3 { *m * o }
fn jet_math_Mat4_matmul(m: &jet_std::Mat4, o: jet_std::Mat4) -> jet_std::Mat4 { *m * o }
fn jet_math_Mat3_transform(m: &jet_std::Mat3, v: jet_std::Vec3) -> jet_std::Vec3 { *m * v }
fn jet_math_Mat4_transform(m: &jet_std::Mat4, v: jet_std::Vec4) -> jet_std::Vec4 { *m * v }
fn jet_math_Mat3_transpose(m: &jet_std::Mat3) -> jet_std::Mat3 {
    let mut r = [0.0f64; 9];
    for c in 0..3 { for row in 0..3 { r[c * 3 + row] = m.0[row * 3 + c]; } }
    jet_std::Mat3(r)
}
fn jet_math_Mat4_transpose(m: &jet_std::Mat4) -> jet_std::Mat4 {
    let mut r = [0.0f64; 16];
    for c in 0..4 { for row in 0..4 { r[c * 4 + row] = m.0[row * 4 + c]; } }
    jet_std::Mat4(r)
}

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
}

impl user_Encode for i64 {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Int(*self) }
}
impl user_Encode for f64 {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Float(*self) }
}
impl user_Encode for bool {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Bool(*self) }
}
impl user_Encode for String {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Text(self.clone()) }
}
impl user_Encode for char {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Text(self.to_string()) }
}
impl<T: user_Encode> user_Encode for Vec<T> {
    fn jet_encode(&self) -> jet_std::DataTree {
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
        jet_std::DataTree::Object(self.iter().map(|(k, v)| (k.clone(), v.jet_encode())).collect())
    }
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
                "expected Int, found {}", jet_std::datatree_kind(other)
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
                "expected Float, found {}", jet_std::datatree_kind(other)
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
                _ => Err(jet_std::DecodeError::new(format!("expected Bool, found text {:?}", s))),
            },
            other => Err(jet_std::DecodeError::new(format!(
                "expected Bool, found {}", jet_std::datatree_kind(other)
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
                "expected Text, found {}", jet_std::datatree_kind(other)
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
            _ => Err(jet_std::DecodeError::new(format!("expected a single Char, found {:?}", s))),
        }
    }
}
impl<T: user_Decode> user_Decode for Vec<T> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    out.push(T::jet_decode(item).map_err(|e| jet_std::DecodeError::under(&format!("[{}]", i), e))?);
                }
                Ok(out)
            }
            other => Err(jet_std::DecodeError::new(format!(
                "expected a list, found {}", jet_std::datatree_kind(other)
            ))),
        }
    }
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
                    out.insert(k.clone(), V::jet_decode(v).map_err(|e| jet_std::DecodeError::under(k, e))?);
                }
                Ok(out)
            }
            other => Err(jet_std::DecodeError::new(format!(
                "expected an object, found {}", jet_std::datatree_kind(other)
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
    let j = jet_std::parse_json(text)
        .map_err(|e| jet_std::DecodeError::new(format!("invalid JSON (line {}): {}", e.line, e.message)))?;
    T::jet_decode(&jet_std::datatree_from_json(&j))
}

// D-MIGRATE3=A: `decode_traced<T>` — same decode, wrapped in `DecodeResult` so the
// caller can ask whether/how it migrated, without `decode` itself paying for it.
fn jet_enc_json_decode_traced<T: user_Decode>(text: &String) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let value = jet_enc_json_decode::<T>(text)?;
    Ok(jet_std::DecodeResult { value, migration: jet_std::MigrationStatus::fresh() })
}

// CSV typed decode: header row maps columns to fields by name; each data row
// becomes a DataTree::Object of Text cells, then decodes to `T`. A short row or a
// per-row decode failure is a typed `DecodeError` naming the 1-based row.
fn jet_enc_csv_decode<T: user_Decode>(text: &String) -> Result<Vec<T>, jet_std::DecodeError> {
    let rows = jet_ring_csv_parse(text).map_err(jet_std::DecodeError::new)?;
    let mut it = rows.into_iter();
    let Some(header) = it.next() else { return Ok(Vec::new()); };
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
        out.push(T::jet_decode(&tree).map_err(|e| jet_std::DecodeError::under(&format!("row {}", i + 1), e))?);
    }
    Ok(out)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_csv_decode` — see json's for the shape.
fn jet_enc_csv_decode_traced<T: user_Decode>(text: &String) -> Result<jet_std::DecodeResult<Vec<T>>, jet_std::DecodeError> {
    let value = jet_enc_csv_decode::<T>(text)?;
    Ok(jet_std::DecodeResult { value, migration: jet_std::MigrationStatus::fresh() })
}

// CSV typed encode: `[T]` → header row (field names from the first row's Object)
// + one record per element. Requires every element to encode to a flat Object.
fn jet_enc_csv_to_string<T: user_Encode>(values: &Vec<T>) -> String {
    let trees: Vec<jet_std::DataTree> = values.iter().map(|v| v.jet_encode()).collect();
    let mut header: Vec<String> = Vec::new();
    if let Some(jet_std::DataTree::Object(entries)) = trees.first() {
        header = entries.iter().map(|(k, _)| k.clone()).collect();
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    rows.push(header.clone());
    for tree in &trees {
        let mut record = Vec::with_capacity(header.len());
        for key in &header {
            let cell = match jet_std::datatree_get(tree, key) {
                Some(jet_std::DataTree::Text(s)) => s.clone(),
                Some(jet_std::DataTree::Int(n)) => n.to_string(),
                Some(jet_std::DataTree::Float(f)) => format!("{:?}", f),
                Some(jet_std::DataTree::Bool(b)) => b.to_string(),
                Some(jet_std::DataTree::Null) | None => String::new(),
                Some(other) => jet_std::render_datatree_json(other, false, 0),
            };
            record.push(cell);
        }
        rows.push(record);
    }
    jet_ring_csv_render(&rows)
}

// D-ENC-DYN1=A+ (c152): TOML is a full serde-equivalent adapter over the one rich
// `DataTree` — nested `[table]`s, arrays-of-tables, dotted keys, and typed scalars.
// The dynamic `parse` returns the `Data` value; `decode<T>` walks the rich tree;
// `to_string` renders a `DataTree` back to a nested document.
fn jet_std_toml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    jet_std::toml::parse_to_tree(text)
        .map_err(|e| jet_std::JsonError { line: e.line as i64, message: e.message })
}
fn jet_std_toml_render(d: &jet_std::DataTree) -> String { jet_std::toml::render(d) }

fn jet_enc_toml_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let tree = jet_std::toml::parse_to_tree(text)
        .map_err(|e| jet_std::DecodeError::new(format!("invalid TOML (line {}): {}", e.line, e.message)))?;
    T::jet_decode(&tree)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_toml_decode` — see json's for the shape.
fn jet_enc_toml_decode_traced<T: user_Decode>(text: &String) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let value = jet_enc_toml_decode::<T>(text)?;
    Ok(jet_std::DecodeResult { value, migration: jet_std::MigrationStatus::fresh() })
}

// YAML typed decode: parse flat scalars into a DataTree::Object of Text, then decode.
// D-ENC-DYN1=A+ / D-ENC-YAML1 (c152): YAML is a full serde adapter over the one
// rich `DataTree` — block + flow maps/sequences, typed core scalars, block scalars,
// comments, documents, anchors/aliases. parse → `Data`; decode<T> → typed tree.
fn jet_std_yaml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    jet_std::yaml::parse_to_tree(text)
        .map_err(|e| jet_std::JsonError { line: e.line as i64, message: e.message })
}
fn jet_std_yaml_render(d: &jet_std::DataTree) -> String { jet_std::yaml::render(d) }

fn jet_enc_yaml_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let tree = jet_std::yaml::parse_to_tree(text)
        .map_err(|e| jet_std::DecodeError::new(format!("invalid YAML (line {}): {}", e.line, e.message)))?;
    T::jet_decode(&tree)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_yaml_decode` — see json's for the shape.
fn jet_enc_yaml_decode_traced<T: user_Decode>(text: &String) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let value = jet_enc_yaml_decode::<T>(text)?;
    Ok(jet_std::DecodeResult { value, migration: jet_std::MigrationStatus::fresh() })
}
fn jet_enc_toml_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::toml::render(&v.jet_encode())
}
fn jet_enc_yaml_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::yaml::render(&v.jet_encode())
}

// ── E2-M9: First-party ring libraries ────────────────────────────────────────
// Pure-Rust, zero external crates (I6). CSV, TOML, YAML, log, time, crypto.

// ── jet.csv ───────────────────────────────────────────────────────────────────
fn jet_ring_csv_parse(text: &String) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        match csv_parse_row(line) {
            Ok(row) => rows.push(row),
            Err(msg) => {
                return Err(format!("E2701: CSV row {} — {}", line_no + 1, msg));
            }
        }
    }
    Ok(rows)
}

fn csv_parse_row(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        let field = if chars.peek() == Some(&'"') {
            chars.next(); // consume opening quote
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('"') => {
                        if chars.peek() == Some(&'"') {
                            chars.next(); // escaped quote
                            s.push('"');
                        } else {
                            break;
                        }
                    }
                    Some(c) => s.push(c),
                    None => break,
                }
            }
            s
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c == ',' { break; }
                s.push(c);
                chars.next();
            }
            s
        };
        fields.push(field);
        match chars.next() {
            Some(',') => {}
            None => break,
            Some(c) => return Err(format!("unexpected character {:?} after field", c)),
        }
    }
    Ok(fields)
}

fn jet_ring_csv_render(rows: &Vec<Vec<String>>) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|field| {
                    if field.contains(',') || field.contains('"') || field.contains('\n') {
                        format!("\"{}\"", field.replace('"', "\"\""))
                    } else {
                        field.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── jet.log ───────────────────────────────────────────────────────────────────
// E2-M12 D-OBS3: structured JSON logs (OTel-aligned field names).
// Each log record is a JSON object on stderr:
//   {"level":"info","body":"...","ts":<unix-ms>}
// When a trace_id is set (log.set_trace_id), it appears as "trace_id":"...".
// Log level: 0=debug, 1=info, 2=warn, 3=error. Default is info (1).
// D-LOGFMT1=A: format 0=auto (TTY→text, else JSON), 1=json, 2=text.
thread_local! {
    static JET_LOG_LEVEL: std::cell::Cell<u8> = std::cell::Cell::new(1);
    static JET_LOG_TRACE_ID: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    static JET_LOG_FORMAT: std::cell::Cell<u8> = std::cell::Cell::new(0);
}

fn jet_ring_log_set_level(level: &String) {
    let n: u8 = match level.as_str() {
        "debug" => 0, "info" => 1, "warn" => 2, "error" => 3, _ => 1,
    };
    JET_LOG_LEVEL.with(|l| l.set(n));
}

fn jet_ring_log_set_trace_id(id: &String) {
    JET_LOG_TRACE_ID.with(|t| *t.borrow_mut() = id.clone());
}

// D-LOGFMT1=A: explicit format override.
fn jet_ring_log_setup(format: &String) {
    let n: u8 = match format.as_str() { "json" => 1, "text" => 2, _ => 0 };
    JET_LOG_FORMAT.with(|f| f.set(n));
}

fn jet_log_format_active() -> u8 {
    let explicit = JET_LOG_FORMAT.with(|f| f.get());
    if explicit != 0 {
        return explicit;
    }
    // Auto-detect: text if stderr is a terminal, JSON otherwise.
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() { 2 } else { 1 }
}

fn jet_log_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c    => out.push(c),
        }
    }
    out
}

fn jet_log_emit_json(level: &str, msg: &str, ts: i64) {
    let trace = JET_LOG_TRACE_ID.with(|t| t.borrow().clone());
    if trace.is_empty() {
        eprintln!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"ts\":{}}}",
            level,
            jet_log_json_escape(msg),
            ts,
        );
    } else {
        eprintln!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"trace_id\":\"{}\",\"ts\":{}}}",
            level,
            jet_log_json_escape(msg),
            jet_log_json_escape(&trace),
            ts,
        );
    }
}

fn jet_log_emit_text(level: &str, msg: &str, ts: i64) {
    let secs = ts / 1000;
    let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
    let level_tag = match level {
        "debug" => "DEBUG", "info" => "INFO", "warn" => "WARN", "error" => "ERROR", _ => level,
    };
    let trace = JET_LOG_TRACE_ID.with(|t| t.borrow().clone());
    if trace.is_empty() {
        eprintln!(
            "[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z | {}",
            level_tag, y, mo, d, h, mi, s, msg
        );
    } else {
        eprintln!(
            "[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z trace={} | {}",
            level_tag, y, mo, d, h, mi, s, trace, msg
        );
    }
}

fn jet_log_emit(level: &str, msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    if jet_log_format_active() == 2 {
        jet_log_emit_text(level, msg, ts);
    } else {
        jet_log_emit_json(level, msg, ts);
    }
}

fn jet_ring_log_debug(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jet_log_emit("debug", msg);
    }
}
fn jet_ring_log_info(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jet_log_emit("info", msg);
    }
}
fn jet_ring_log_warn(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jet_log_emit("warn", msg);
    }
}
fn jet_ring_log_error(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jet_log_emit("error", msg);
    }
}

// ── jet.time ──────────────────────────────────────────────────────────────────
// Format a Unix millisecond timestamp using a strftime-like pattern.
// Supported tokens: %Y year, %m month, %d day, %H hour, %M minute, %S second.
fn jet_ring_time_format(millis: i64, fmt: &String) -> String {
    let secs = (millis / 1000) as i64;
    let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
    let mut out = fmt.clone();
    out = out.replace("%Y", &format!("{:04}", y));
    out = out.replace("%m", &format!("{:02}", mo));
    out = out.replace("%d", &format!("{:02}", d));
    out = out.replace("%H", &format!("{:02}", h));
    out = out.replace("%M", &format!("{:02}", mi));
    out = out.replace("%S", &format!("{:02}", s));
    out
}

fn unix_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Days since epoch, treating every year as having the right leap-year logic.
    let mut days = secs / 86400;
    let time_of_day = (secs % 86400).unsigned_abs();
    let h = (time_of_day / 3600) as u32;
    let mi = ((time_of_day % 3600) / 60) as u32;
    let s = (time_of_day % 60) as u32;
    // Walk from 1970.
    let mut year: i32 = 1970;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy { break; }
        days -= dy;
        year += 1;
    }
    let month_days: [i64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month: u32 = 1;
    for &md in &month_days {
        if days < md { break; }
        days -= md;
        month += 1;
    }
    (year, month, (days + 1) as u32, h, mi, s)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

// ── jet.crypto ────────────────────────────────────────────────────────────────
// SHA-256 of a UTF-8 string, returned as a lowercase hex string.
fn jet_ring_crypto_sha256(s: &String) -> String {
    let hash = jet_sha256_raw(s.as_bytes());
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

// SHA-256 of a byte list (Vec<u8>), returned as lowercase hex.
fn jet_ring_crypto_sha256_bytes(bs: &Vec<u8>) -> String {
    let hash = jet_sha256_raw(bs.as_slice());
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

// D-TTLVAL1=A: Expiring<T> / Rotting<T> (pure std, injectable Clock).
fn jet_expiring_new<T: Clone>(value: T, ttl_ms: i64, clock_now: i64) -> JetExpiring<T> {
    JetExpiring::new(value, clock_now.saturating_add(ttl_ms))
}
fn jet_rotting_new<T: Clone + 'static>(value: T, ttl_ms: i64, clock_now: i64) -> JetRotting<T> {
    JetRotting::new(value, clock_now.saturating_add(ttl_ms))
}
fn jet_expiring_get<T: Clone>(exp: &JetExpiring<T>, now_ms: i64) -> Result<T, JetExpired> {
    exp.get(now_ms)
}
fn jet_rotting_get<T: Clone + 'static>(rot: &mut JetRotting<T>, now_ms: i64) -> Result<T, JetExpired> {
    rot.get(now_ms)
}

// Minimal SHA-256 (same algorithm as src/sha256.rs — duplicated here so the
// prelude doesn't need to reach into the compiler crate; I6 forbids extern deps).
fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut state = H0;
    let bit_len = (data.len() as u64) * 8;
    let mut msg: Vec<u8> = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes(block[i*4..i*4+4].try_into().unwrap()); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] =
            [state[0], state[1], state[2], state[3], state[4], state[5], state[6], state[7]];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g; g = f; f = e; e = d.wrapping_add(temp1);
            d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a); state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c); state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e); state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g); state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (i, &s) in state.iter().enumerate() {
        out[i*4..i*4+4].copy_from_slice(&s.to_be_bytes());
    }
    out
}

// ── D-UUIDENC1=A: core.encoding.hex / core.encoding.base64 / core.uuid ───────
// Pure std implementations; zero external crates (I6); memory-safe (I1).

fn jet_std_hex_encode(bytes: &Vec<u8>) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn jet_std_hex_decode(text: &String) -> Result<Vec<u8>, String> {
    let s = text.trim();
    if s.len() % 2 != 0 {
        return Err(format!("hex string has odd length ({})", s.len()));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        match u8::from_str_radix(&s[i..i + 2], 16) {
            Ok(b) => out.push(b),
            Err(_) => return Err(format!("invalid hex at offset {}: {:?}", i, &s[i..i + 2])),
        }
    }
    Ok(out)
}

const JET_B64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn jet_std_b64_encode(bytes: &Vec<u8>) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(JET_B64_CHARS[(n >> 18) as usize] as char);
        out.push(JET_B64_CHARS[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 { JET_B64_CHARS[((n >> 6) & 0x3f) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { JET_B64_CHARS[(n & 0x3f) as usize] as char } else { '=' });
    }
    out
}

fn jet_b64_val(b: u8) -> Result<u8, String> {
    match b {
        b'A'..=b'Z' => Ok(b - b'A'),
        b'a'..=b'z' => Ok(b - b'a' + 26),
        b'0'..=b'9' => Ok(b - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("invalid base64 character: {:?}", b as char)),
    }
}

fn jet_std_b64_decode(text: &String) -> Result<Vec<u8>, String> {
    let input: Vec<u8> = text.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
    if input.len() % 4 != 0 {
        return Err(format!("base64 length must be a multiple of 4 (got {})", input.len()));
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks(4) {
        let a = jet_b64_val(chunk[0])?;
        let b = jet_b64_val(chunk[1])?;
        out.push(((a << 2) | (b >> 4)) as u8);
        if chunk[2] != b'=' {
            let c = jet_b64_val(chunk[2])?;
            out.push(((b << 4) | (c >> 2)) as u8);
            if chunk[3] != b'=' {
                let d = jet_b64_val(chunk[3])?;
                out.push(((c << 6) | d) as u8);
            }
        }
    }
    Ok(out)
}

// UUID helpers — pure std, zero deps. CSPRNG via /dev/urandom (POSIX); the
// fallback SplitMix64 engages only when /dev/urandom is unavailable.
fn jet_uuid_fill_random(out: &mut [u8]) {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(out).is_ok() {
            return;
        }
    }
    // Fallback: SplitMix64 seeded from wall-clock nanoseconds.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    for b in out.iter_mut() {
        state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = (state ^ (state >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        *b = (z ^ (z >> 31)) as u8;
    }
}

fn jet_uuid_format(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
         {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn jet_std_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    jet_uuid_fill_random(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    jet_uuid_format(&bytes)
}

fn jet_std_uuid_v7(clock: &jet_std::Clock) -> String {
    let ts_ms = clock.now as u64;
    let mut bytes = [0u8; 16];
    // 48-bit timestamp in the high bytes
    bytes[0] = (ts_ms >> 40) as u8;
    bytes[1] = (ts_ms >> 32) as u8;
    bytes[2] = (ts_ms >> 24) as u8;
    bytes[3] = (ts_ms >> 16) as u8;
    bytes[4] = (ts_ms >> 8) as u8;
    bytes[5] = ts_ms as u8;
    jet_uuid_fill_random(&mut bytes[6..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70; // version 7
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    jet_uuid_format(&bytes)
}

// ── E2-M10: networking (core.net + jet.http) ─────────────────────────────────
// All networking uses std::net only — zero external crates in the prelude (I6).
// TLS (D-NET1) is delivered as the `jet.tls` FFI package and is not included here.

pub struct JetTcpListener {
    inner: std::net::TcpListener,
}

pub struct JetTcpStream {
    inner: std::net::TcpStream,
}

#[derive(Clone)]
pub struct JetHttpRequest {
    pub method: String,
    pub path: String,
    pub body: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub params: std::collections::BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct JetHttpResponse {
    pub status: String,
    pub body: String,
    pub headers: std::collections::BTreeMap<String, String>,
}

// D-ROUTE1=A: HTTP router — registration + :param dispatch.
#[derive(Clone)]
enum RouteSegment {
    Static(String),
    Param(String),
}

type JetHttpHandler = Box<dyn Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync>;

struct JetHttpRoute {
    method: String,
    segments: Vec<RouteSegment>,
    handler: JetHttpHandler,
}

pub struct JetHttpRouter {
    routes: Vec<JetHttpRoute>,
}

impl JetShow for JetTcpListener {
    fn jet_show(&self) -> String { format!("TcpListener({})", self.inner.local_addr().map(|a| a.to_string()).unwrap_or_default()) }
}
impl JetShow for JetTcpStream {
    fn jet_show(&self) -> String { format!("TcpStream({})", self.inner.peer_addr().map(|a| a.to_string()).unwrap_or_default()) }
}
impl JetShow for JetHttpRequest {
    fn jet_show(&self) -> String { format!("{} {}", self.method, self.path) }
}
impl JetShow for JetHttpResponse {
    fn jet_show(&self) -> String { format!("HTTP {}", self.status) }
}
impl JetShow for JetHttpRouter {
    fn jet_show(&self) -> String { format!("HttpRouter({} routes)", self.routes.len()) }
}

fn jet_net_tcp_listen(addr: &String) -> Result<JetTcpListener, String> {
    std::net::TcpListener::bind(addr.as_str())
        .map(|l| JetTcpListener { inner: l })
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))
}

fn jet_net_tcp_accept(listener: &JetTcpListener) -> Result<JetTcpStream, String> {
    listener.inner.accept()
        .map(|(s, _)| JetTcpStream { inner: s })
        .map_err(|e| format!("accept failed: {}", e))
}

fn jet_net_tcp_connect(addr: &String) -> Result<JetTcpStream, String> {
    std::net::TcpStream::connect(addr.as_str())
        .map(|s| JetTcpStream { inner: s })
        .map_err(|e| format!("connect to `{}` failed: {}", addr, e))
}

fn jet_net_tcp_read(stream: &mut JetTcpStream) -> Result<String, String> {
    use std::io::Read;
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("tcp read");
        }
        let _ = stream
            .inner
            .set_read_timeout(Some(std::time::Duration::from_millis(remaining as u64)));
    }
    let mut buf = [0u8; 8192];
    loop {
        match stream.inner.read(&mut buf) {
            Ok(0) => return Ok(String::new()),
            Ok(n) => {
                jet_deadline_check("tcp read");
                return String::from_utf8(buf[..n].to_vec())
                    .map_err(|e| format!("tcp read: invalid UTF-8: {}", e));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                jet_scheduler_io_wait(&stream.inner, true, false, "tcp read");
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    jet_deadline_exceeded("tcp read");
                }
                return Err(format!("tcp read failed: {}", e));
            }
        }
    }
}

fn jet_net_tcp_write(stream: &mut JetTcpStream, data: &String) -> Result<(), String> {
    use std::io::Write;
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("tcp write");
        }
        let _ = stream
            .inner
            .set_write_timeout(Some(std::time::Duration::from_millis(remaining as u64)));
    }
    let bytes = data.as_bytes();
    let mut off = 0usize;
    while off < bytes.len() {
        match stream.inner.write(&bytes[off..]) {
            Ok(0) => return Err("tcp write failed: zero bytes written".to_string()),
            Ok(n) => off += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                jet_scheduler_io_wait(&stream.inner, false, true, "tcp write");
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    jet_deadline_exceeded("tcp write");
                }
                return Err(format!("tcp write failed: {}", e));
            }
        }
    }
    jet_deadline_check("tcp write");
    Ok(())
}

fn jet_net_tcp_local_addr(stream: &JetTcpStream) -> String {
    stream.inner.local_addr().map(|a| a.to_string()).unwrap_or_default()
}

fn jet_net_tcp_peer_addr(stream: &JetTcpStream) -> String {
    stream.inner.peer_addr().map(|a| a.to_string()).unwrap_or_default()
}

fn jet_net_listener_local_addr(listener: &JetTcpListener) -> String {
    listener.inner.local_addr().map(|a| a.to_string()).unwrap_or_default()
}

fn jet_net_set_timeout(stream: &mut JetTcpStream, ms: i64) {
    let dur = std::time::Duration::from_millis(ms as u64);
    let _ = stream.inner.set_read_timeout(Some(dur));
    let _ = stream.inner.set_write_timeout(Some(dur));
}

/// Send a well-formed HTTP/1.1 response on a TcpStream and close it.
/// Handles CRLF line endings internally so Jet code doesn't need `\r`.
fn jet_net_tcp_reply(mut stream: JetTcpStream, status: &String, body: &String) {
    use std::io::Write;
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
        status, body.len(), body
    );
    let _ = stream.inner.write_all(response.as_bytes());
}

// ── HTTP/1.1 client (minimal, over std::net::TcpStream) ──────────────────────

fn jet_http_get(url: &String) -> Result<JetHttpResponse, String> {
    jet_http_request(url, "GET", &[], "")
}

fn jet_http_post(url: &String, body: &String) -> Result<JetHttpResponse, String> {
    jet_http_request(url, "POST", &[], body.as_str())
}

fn jet_http_request(url: &str, method: &str, extra_headers: &[(&str, &str)], body: &str) -> Result<JetHttpResponse, String> {
    use std::io::{Read, Write};
    // Parse URL: http://host[:port]/path
    let url_str = url;
    let (host_port, path) = if let Some(rest) = url_str.strip_prefix("http://") {
        let slash = rest.find('/').unwrap_or(rest.len());
        let hp = &rest[..slash];
        let p = if slash < rest.len() { &rest[slash..] } else { "/" };
        (hp.to_string(), p.to_string())
    } else if let Some(rest) = url_str.strip_prefix("https://") {
        return Err("HTTPS requires the `jet.tls` package; this is plain HTTP. Add `jet.tls` to your pkg.jet to enable HTTPS.".to_string());
        // Keep the variable to silence unused warning in case we extend later.
        #[allow(unreachable_code)]
        { (rest.to_string(), "/".to_string()) }
    } else {
        return Err(format!("URL must start with http:// — got `{}`", url));
    };
    // Default port 80 if not specified.
    let addr = if host_port.contains(':') {
        host_port.clone()
    } else {
        format!("{}:80", host_port)
    };
    let host = host_port.split(':').next().unwrap_or(&host_port);
    let mut stream = std::net::TcpStream::connect(&addr)
        .map_err(|e| format!("connect to `{}` failed: {}", addr, e))?;
    // Build HTTP/1.1 request.
    let content_len = body.len();
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Jet/1.0\r\nConnection: close\r\n",
        method, path, host
    );
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", content_len));
    }
    for (k, v) in extra_headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    if !body.is_empty() {
        req.push_str(body);
    }
    stream.write_all(req.as_bytes())
        .map_err(|e| format!("http write failed: {}", e))?;
    // Read response.
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)
        .map_err(|e| format!("http read failed: {}", e))?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    // Parse status line + headers + body.
    let sep = text.find("\r\n\r\n").unwrap_or(text.len());
    let header_part = &text[..sep];
    let body_part = if sep + 4 <= text.len() { text[sep + 4..].to_string() } else { String::new() };
    let mut lines = header_part.lines();
    let status_line = lines.next().unwrap_or("HTTP/1.1 200 OK");
    let status = status_line.splitn(2, ' ').nth(1).unwrap_or("200 OK").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    Ok(JetHttpResponse { status, body: body_part, headers })
}

// ── HTTP/1.1 server (blocking, one thread per connection) ────────────────────
// note: `jet serve` uses one task per connection. This is excellent for internal
//       services and tools at hundreds of concurrent connections. For very high
//       connection counts, Jet is not the right tool yet — see docs/services.md.

fn jet_http_serve<F>(addr: &String, handler: F)
where
    F: Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync + 'static,
{
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .unwrap_or_else(|e| { eprintln!("E2801: bind on `{}` failed: {}", addr, e); std::process::exit(1); });
    let handler = std::sync::Arc::new(handler);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => { eprintln!("E2801: accept failed: {}", e); continue; }
        };
        let h = handler.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_parse_request(&raw);
            let resp = h(req);
            let response_text = jet_http_format_response(&resp);
            let _ = stream.write_all(response_text.as_bytes());
        });
    }
}

fn jet_http_parse_request(raw: &str) -> JetHttpRequest {
    let sep = raw.find("\r\n\r\n").unwrap_or(raw.len());
    let header_part = &raw[..sep];
    let body = if sep + 4 <= raw.len() { raw[sep + 4..].to_string() } else { String::new() };
    let mut lines = header_part.lines();
    let request_line = lines.next().unwrap_or("GET / HTTP/1.1");
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    JetHttpRequest { method, path, body, headers, params: std::collections::BTreeMap::new() }
}

fn jet_http_format_response(resp: &JetHttpResponse) -> String {
    let mut out = format!("HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n", resp.status, resp.body.len());
    for (k, v) in &resp.headers {
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str("\r\n");
    out.push_str(&resp.body);
    out
}

// D-ROUTE1=A: router runtime ──────────────────────────────────────────────────

fn jet_http_router_new() -> JetHttpRouter {
    JetHttpRouter { routes: Vec::new() }
}

fn jet_http_router_parse_pattern(pattern: &str) -> Vec<RouteSegment> {
    pattern.split('/').filter_map(|seg| {
        if seg.is_empty() { return None; }
        if let Some(name) = seg.strip_prefix(':') {
            Some(RouteSegment::Param(name.to_string()))
        } else {
            Some(RouteSegment::Static(seg.to_string()))
        }
    }).collect()
}

fn jet_http_router_register(router: &mut JetHttpRouter, method: String, pattern: String, handler: JetHttpHandler) {
    // E2804 (runtime): duplicate method+pattern panics at registration time.
    let segs = jet_http_router_parse_pattern(&pattern);
    let is_dup = router.routes.iter().any(|r| {
        r.method == method && r.segments.len() == segs.len() && r.segments.iter().zip(segs.iter()).all(|(a, b)| {
            match (a, b) {
                (RouteSegment::Static(x), RouteSegment::Static(y)) => x == y,
                (RouteSegment::Param(_), RouteSegment::Param(_)) => true,
                _ => false,
            }
        })
    });
    if is_dup {
        panic!("E2804: duplicate route `{} {}`", method, pattern);
    }
    router.routes.push(JetHttpRoute {
        method,
        segments: segs,
        handler,
    });
}

/// Count static segments in a route (for precedence: more statics win).
fn route_static_count(segs: &[RouteSegment]) -> usize {
    segs.iter().filter(|s| matches!(s, RouteSegment::Static(_))).count()
}

fn jet_http_router_dispatch(router: &JetHttpRouter, req: JetHttpRequest) -> JetHttpResponse {
    let path_segs: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
    // Collect matching routes with their static count (for precedence).
    let mut candidates: Vec<(usize, usize)> = Vec::new(); // (route_idx, static_count)
    for (i, route) in router.routes.iter().enumerate() {
        if route.segments.len() != path_segs.len() { continue; }
        let mut ok = true;
        for (rseg, pseg) in route.segments.iter().zip(path_segs.iter()) {
            if let RouteSegment::Static(s) = rseg {
                if s != pseg { ok = false; break; }
            }
        }
        if ok { candidates.push((i, route_static_count(&route.segments))); }
    }
    if candidates.is_empty() {
        return JetHttpResponse {
            status: "404 Not Found".to_string(),
            body: "404 not found".to_string(),
            headers: std::collections::BTreeMap::new(),
        };
    }
    // Pick highest static-count match with the right method; otherwise 405.
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    let method_match = candidates.iter().find(|(i, _)| router.routes[*i].method == req.method);
    let Some((route_idx, _)) = method_match.copied() else {
        return JetHttpResponse {
            status: "405 Method Not Allowed".to_string(),
            body: "405 method not allowed".to_string(),
            headers: std::collections::BTreeMap::new(),
        };
    };
    let route = &router.routes[route_idx];
    let mut params = std::collections::BTreeMap::new();
    for (rseg, pseg) in route.segments.iter().zip(path_segs.iter()) {
        if let RouteSegment::Param(name) = rseg {
            params.insert(name.clone(), pseg.to_string());
        }
    }
    let mut req2 = req;
    req2.params = params;
    (route.handler)(req2)
}

fn jet_http_serve_router(addr: &String, router: JetHttpRouter) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .unwrap_or_else(|e| { eprintln!("E2801: bind on `{}` failed: {}", addr, e); std::process::exit(1); });
    let router = std::sync::Arc::new(router);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => { eprintln!("E2801: accept failed: {}", e); continue; }
        };
        let r = router.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_parse_request(&raw);
            let resp = jet_http_router_dispatch(&r, req);
            let response_text = jet_http_format_response(&resp);
            let _ = stream.write_all(response_text.as_bytes());
        });
    }
}

fn jet_http_request_param(req: &JetHttpRequest, name: &String) -> Option<String> {
    req.params.get(name.as_str()).cloned()
}

// ── D-HTTPLIB2=B / D-HTTPLIB4=B: core.http.client — request builder ─────────
// JetHttpClientReq and JetHttpClientResp live here (in the generated program's
// crate) so they're accessible without cross-crate type imports. The ureq
// bridge functions use only primitive types (i64, String, Vec<String>) and are
// called through wrappers here. This is the I6-safe pattern.

#[derive(Clone)]
struct JetHttpClientReq {
    method: String,
    url: String,
    headers: Vec<String>, // alternating key, value pairs
    body: Option<String>,
    timeout_ms: Option<i64>,
}

#[derive(Clone)]
struct JetHttpClientResp {
    status: i64,
    body: String,
    headers: Vec<String>, // alternating key, value pairs
}

fn jet_http_client_request_new(method: &String, url: &String) -> JetHttpClientReq {
    JetHttpClientReq { method: method.clone(), url: url.clone(), headers: Vec::new(), body: None, timeout_ms: None }
}

fn jet_http_client_request_header(mut req: JetHttpClientReq, name: &String, value: &String) -> JetHttpClientReq {
    req.headers.push(name.clone());
    req.headers.push(value.clone());
    req
}

fn jet_http_client_request_body(mut req: JetHttpClientReq, body: &String) -> JetHttpClientReq {
    req.body = Some(body.clone());
    req
}

fn jet_http_client_request_timeout(mut req: JetHttpClientReq, ms: i64) -> JetHttpClientReq {
    req.timeout_ms = Some(ms);
    req
}

fn jet_http_client_response_status(resp: &JetHttpClientResp) -> i64 { resp.status }
fn jet_http_client_response_body(resp: &JetHttpClientResp) -> String { resp.body.clone() }
fn jet_http_client_response_header(resp: &JetHttpClientResp, name: &String) -> Option<String> {
    let name_lc = name.to_lowercase();
    let mut i = 0;
    while i + 1 < resp.headers.len() {
        if resp.headers[i].to_lowercase() == name_lc { return Some(resp.headers[i+1].clone()); }
        i += 2;
    }
    None
}


// ── D-HTTPLIB1=A / D-HTTPLIB2=B: core.http.server — function-first mux ───────
// Pure std, no external crates (I6). HTTP/1.1 blocking server with path-param
// extraction and a typed mux surface. HTTP/2 and WebSocket require the bridge
// crate and are tracked as follow-up work.

#[derive(Clone)]
struct JetHttpSrvResp {
    status: i64,
    body: String,
    headers: std::collections::BTreeMap<String, String>,
}

#[derive(Clone)]
struct JetHttpSrvReq {
    method: String,
    path: String,
    params: std::collections::BTreeMap<String, String>,
    body: String,
    headers: std::collections::BTreeMap<String, String>,
}

type JetHttpMuxHandlerFn = std::sync::Arc<dyn Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync>;

struct JetHttpMuxRoute {
    method: String,
    pattern: String,
    handler: JetHttpMuxHandlerFn,
}

#[derive(Clone)]
struct JetHttpMux(std::sync::Arc<std::sync::Mutex<Vec<JetHttpMuxRoute>>>);

impl JetHttpMux {
    fn new() -> Self { JetHttpMux(std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))) }
    fn add<F>(&self, method: &str, pattern: &str, f: F)
    where F: Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync + 'static
    {
        self.0.lock().unwrap().push(JetHttpMuxRoute {
            method: method.to_uppercase(),
            pattern: pattern.to_string(),
            handler: std::sync::Arc::new(f) as JetHttpMuxHandlerFn,
        });
    }
}

fn jet_http_mux_new() -> JetHttpMux { JetHttpMux::new() }

fn jet_http_mux_add<F>(mux: &JetHttpMux, method: &str, pattern: &str, f: F)
where F: Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync + 'static
{ mux.add(method, pattern, f); }

fn jet_http_srv_response(status: i64, body: &String) -> JetHttpSrvResp {
    JetHttpSrvResp { status, body: body.clone(), headers: std::collections::BTreeMap::new() }
}

fn jet_http_srv_response_header(mut resp: JetHttpSrvResp, name: &String, value: &String) -> JetHttpSrvResp {
    resp.headers.insert(name.clone(), value.clone());
    resp
}

fn jet_http_mux_serve(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    let mux = std::sync::Arc::new(mux);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => { eprintln!("http accept failed: {}", e); continue; }
        };
        let m = mux.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 65536];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_srv_parse(&raw);
            let resp = jet_http_mux_dispatch(&m, req);
            let text = jet_http_srv_format(&resp);
            let _ = stream.write_all(text.as_bytes());
        });
    }
}

fn jet_http_srv_parse(raw: &str) -> JetHttpSrvReq {
    let sep = raw.find("\r\n\r\n").unwrap_or(raw.len());
    let header_part = &raw[..sep];
    let body = if sep + 4 <= raw.len() { raw[sep + 4..].to_string() } else { String::new() };
    let mut lines = header_part.lines();
    let req_line = lines.next().unwrap_or("GET / HTTP/1.1");
    let mut parts = req_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    JetHttpSrvReq { method, path, params: std::collections::BTreeMap::new(), body, headers }
}

fn jet_http_mux_dispatch(mux: &JetHttpMux, req: JetHttpSrvReq) -> JetHttpSrvResp {
    let routes = mux.0.lock().unwrap();
    for route in routes.iter() {
        if route.method != req.method { continue; }
        if let Some(params) = jet_http_match_path(&route.pattern, &req.path) {
            let mut r2 = req.clone();
            r2.params = params;
            return (route.handler)(r2);
        }
    }
    JetHttpSrvResp {
        status: 404,
        body: "404 Not Found".to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}

fn jet_http_match_path(pattern: &str, path: &str) -> Option<std::collections::BTreeMap<String, String>> {
    let p_segs: Vec<&str> = pattern.split('/').collect();
    let r_segs: Vec<&str> = path.split('?').next().unwrap_or(path).split('/').collect();
    if p_segs.len() != r_segs.len() { return None; }
    let mut params = std::collections::BTreeMap::new();
    for (p, r) in p_segs.iter().zip(r_segs.iter()) {
        if let Some(key) = p.strip_prefix(':') {
            params.insert(key.to_string(), r.to_string());
        } else if *p != *r {
            return None;
        }
    }
    Some(params)
}

fn jet_http_srv_format(resp: &JetHttpSrvResp) -> String {
    let reason = match resp.status {
        200 => "OK", 201 => "Created", 204 => "No Content",
        301 => "Moved Permanently", 302 => "Found",
        400 => "Bad Request", 401 => "Unauthorized", 403 => "Forbidden",
        404 => "Not Found", 405 => "Method Not Allowed",
        500 => "Internal Server Error", _ => "OK",
    };
    let mut out = format!("HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp.status, reason, resp.body.len());
    for (k, v) in &resp.headers {
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str("\r\n");
    out.push_str(&resp.body);
    out
}

fn jet_http_srv_req_method(req: &JetHttpSrvReq) -> String { req.method.clone() }
fn jet_http_srv_req_path(req: &JetHttpSrvReq) -> String { req.path.clone() }
fn jet_http_srv_req_param(req: &JetHttpSrvReq, name: &String) -> Option<String> { req.params.get(name).cloned() }
fn jet_http_srv_req_body(req: &JetHttpSrvReq) -> String { req.body.clone() }
fn jet_http_srv_req_header(req: &JetHttpSrvReq, name: &String) -> Option<String> { req.headers.get(&name.to_lowercase()).cloned() }

// ── D-ARGS1: declarative CLI arg parsing (ratified 2026-06-22) ───────────────
// The builder accumulates a spec; `jet_args_parse` runs it against an argv
// list, producing `ParsedArgs` or an error string (never exits — the caller
// decides what to print and how to exit, which keeps the API testable).
//
// Design: builder methods take the spec BY VALUE and return a new one —
// ownership-safe, no aliasing, works with both immutable (::) and mutable
// (:=) bindings in Jet. The parse result is cloneable.
//
// `--help` is recognized but NOT parsed out of argv here; the caller tests
// `parsed.flag("help")` if they want to handle it. The auto-generated help
// text is available via `spec.help()` and `spec.help_auto()`.

/// A single entry in the spec.
#[derive(Clone)]
enum JetArgKind {
    /// Boolean flag: `--name` sets it to true.
    Flag { name: String, help: String },
    /// Value option: `--name VALUE` captures VALUE.
    Option { name: String, help: String, meta: String },
    /// Positional argument (in declaration order).
    Positional { name: String, help: String },
}

/// The builder. All methods consume self and return a new spec (builder pattern).
#[derive(Clone)]
struct JetArgsSpec {
    entries: Vec<JetArgKind>,
    prog: String,
}

/// The parse result.
#[derive(Clone)]
struct JetParsedArgs {
    flags: std::collections::HashMap<String, bool>,
    options: std::collections::HashMap<String, String>,
    positionals: Vec<String>,
}

impl JetArgsSpec {
    /// Render the generated --help text.
    fn help(&self) -> String {
        let mut s = String::new();
        // usage line
        let prog = if self.prog.is_empty() { "program".to_string() } else { self.prog.clone() };
        let has_opts = self.entries.iter().any(|e| matches!(e, JetArgKind::Flag { .. } | JetArgKind::Option { .. }));
        let positionals: Vec<&JetArgKind> = self.entries.iter().filter(|e| matches!(e, JetArgKind::Positional { .. })).collect();
        s.push_str("Usage: ");
        s.push_str(&prog);
        if has_opts { s.push_str(" [options]"); }
        for p in &positionals {
            if let JetArgKind::Positional { name, .. } = p {
                s.push(' ');
                s.push_str(name);
            }
        }
        s.push('\n');
        // flags and options
        let flags_opts: Vec<&JetArgKind> = self.entries.iter().filter(|e| !matches!(e, JetArgKind::Positional { .. })).collect();
        if !flags_opts.is_empty() {
            s.push('\n');
            s.push_str("Options:\n");
            for e in flags_opts {
                match e {
                    JetArgKind::Flag { name, help } => {
                        s.push_str(&format!("  --{:<20} {}\n", name, help));
                    }
                    JetArgKind::Option { name, help, meta } => {
                        s.push_str(&format!("  --{} {:<16} {}\n", name, meta, help));
                    }
                    _ => {}
                }
            }
        }
        // positionals
        if !positionals.is_empty() {
            s.push('\n');
            s.push_str("Arguments:\n");
            for p in positionals {
                if let JetArgKind::Positional { name, help } = p {
                    s.push_str(&format!("  {:<22} {}\n", name, help));
                }
            }
        }
        s
    }
}

fn jet_args_spec() -> JetArgsSpec {
    // argv[0] is the program name — capture it from env at spec-creation time.
    let prog = std::env::args().next().unwrap_or_default();
    JetArgsSpec { entries: Vec::new(), prog }
}

fn jet_args_flag(mut spec: JetArgsSpec, name: &String, help: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Flag { name: name.clone(), help: help.clone() });
    spec
}

fn jet_args_option(mut spec: JetArgsSpec, name: &String, help: &String, meta: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Option { name: name.clone(), help: help.clone(), meta: meta.clone() });
    spec
}

fn jet_args_positional(mut spec: JetArgsSpec, name: &String, help: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Positional { name: name.clone(), help: help.clone() });
    spec
}

/// Parse argv against the spec. Returns `Err(message)` on unknown flags/options
/// or missing required positionals. `argv[0]` (the program name) is skipped.
fn jet_args_parse(spec: &JetArgsSpec, argv: &Vec<String>) -> Result<JetParsedArgs, String> {
    let mut flags: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut options: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut positionals: Vec<String> = Vec::new();

    // Seed all flags as false (so .flag("name") returns false when absent).
    for e in &spec.entries {
        if let JetArgKind::Flag { name, .. } = e {
            flags.insert(name.clone(), false);
        }
    }

    // Build fast lookup sets.
    let flag_names: std::collections::HashSet<String> = spec.entries.iter()
        .filter_map(|e| if let JetArgKind::Flag { name, .. } = e { Some(name.clone()) } else { None })
        .collect();
    let option_names: std::collections::HashSet<String> = spec.entries.iter()
        .filter_map(|e| if let JetArgKind::Option { name, .. } = e { Some(name.clone()) } else { None })
        .collect();

    let mut i = 1usize; // skip argv[0]
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--" {
            i += 1;
            // Everything after `--` is positional.
            while i < argv.len() {
                positionals.push(argv[i].clone());
                i += 1;
            }
            break;
        }
        if let Some(rest) = arg.strip_prefix("--") {
            // Try `--name=value` form.
            if let Some(eq) = rest.find('=') {
                let name = &rest[..eq];
                let val = &rest[eq + 1..];
                if option_names.contains(name) {
                    options.insert(name.to_string(), val.to_string());
                } else if flag_names.contains(name) {
                    return Err(format!("--{} is a flag; it takes no value (got `={}`)\n\n{}", name, val, spec.help()));
                } else {
                    return Err(format!("unknown option `--{}`\n\n{}", name, spec.help()));
                }
            } else if flag_names.contains(rest) {
                flags.insert(rest.to_string(), true);
            } else if option_names.contains(rest) {
                i += 1;
                if i >= argv.len() {
                    return Err(format!("`--{}` requires a value\n\n{}", rest, spec.help()));
                }
                options.insert(rest.to_string(), argv[i].clone());
            } else {
                return Err(format!("unknown option `--{}`\n\n{}", rest, spec.help()));
            }
        } else {
            positionals.push(arg.clone());
        }
        i += 1;
    }

    // Check required positionals.
    let required_count = spec.entries.iter().filter(|e| matches!(e, JetArgKind::Positional { .. })).count();
    if positionals.len() < required_count {
        let missing: Vec<&str> = spec.entries.iter()
            .filter_map(|e| if let JetArgKind::Positional { name, .. } = e { Some(name.as_str()) } else { None })
            .skip(positionals.len())
            .collect();
        return Err(format!("missing required argument{}: {}\n\n{}", if missing.len() == 1 { "" } else { "s" }, missing.join(", "), spec.help()));
    }

    Ok(JetParsedArgs { flags, options, positionals })
}

fn jet_parsed_flag(parsed: &JetParsedArgs, name: &String) -> bool {
    *parsed.flags.get(name.as_str()).unwrap_or(&false)
}

fn jet_parsed_option(parsed: &JetParsedArgs, name: &String) -> Option<String> {
    parsed.options.get(name.as_str()).cloned()
}

fn jet_parsed_positional(parsed: &JetParsedArgs, idx: i64) -> Option<String> {
    if idx < 0 { return None; }
    parsed.positionals.get(idx as usize).cloned()
}

impl JetShow for JetArgsSpec {
    fn jet_show(&self) -> String { format!("ArgsSpec({})", self.entries.len()) }
}
impl JetShow for JetParsedArgs {
    fn jet_show(&self) -> String { format!("ParsedArgs(flags={}, options={}, positionals={})", self.flags.len(), self.options.len(), self.positionals.len()) }
}

// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` — the runtime reflection floor.
// `JetReflectValue` is the whole-value handle (`type_name`/`display` always
// populated; `fields` non-empty only when the reflected value was a known
// user struct — built entirely at the call site, `Codegen/TIR/emit.rs`
// `("core.reflect", "of")`). `JetReflectField` is one struct field's name
// and its `.jet_show()`-rendered value. Both are plain data — no runtime
// type registry, no raw-pointer/audited-region casting of any kind (I1):
// everything here is a string captured at compile time from the call
// site's already-known static type.

#[derive(Clone)]
struct JetReflectValue {
    type_name: String,
    display: String,
    fields: Vec<JetReflectField>,
}

#[derive(Clone)]
struct JetReflectField {
    name: String,
    value: String,
}

impl JetReflectValue {
    fn type_name(&self) -> String { self.type_name.clone() }
    fn display(&self) -> String { self.display.clone() }
    fn fields(&self) -> Vec<JetReflectField> { self.fields.clone() }
}

impl JetReflectField {
    fn name(&self) -> String { self.name.clone() }
    fn value(&self) -> String { self.value.clone() }
}

impl JetShow for JetReflectValue {
    fn jet_show(&self) -> String { format!("Value({})", self.type_name) }
}
impl JetShow for JetReflectField {
    fn jet_show(&self) -> String { format!("Field({}: {})", self.name, self.value) }
}
