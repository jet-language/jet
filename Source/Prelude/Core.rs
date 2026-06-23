trait JetShow { fn jet_show(&self) -> String; }
impl JetShow for i64 { fn jet_show(&self) -> String { self.to_string() } }
// D-SG9: fixed-width integers and the 32-bit float print like their defaults.
impl JetShow for i8 { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for i16 { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for i32 { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for u8 { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for u16 { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for u32 { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for u64 { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for f32 { fn jet_show(&self) -> String { format!("{:?}", self) } }
impl JetShow for f64 { fn jet_show(&self) -> String { format!("{:?}", self) } }
impl JetShow for bool { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for String { fn jet_show(&self) -> String { self.clone() } }
impl<T: JetShow> JetShow for &T { fn jet_show(&self) -> String { (**self).jet_show() } }
impl<T: JetShow> JetShow for Vec<T> { fn jet_show(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
    format!("[{}]", parts.join(", "))
} }
impl JetShow for char { fn jet_show(&self) -> String { self.to_string() } }
impl<T: JetShow> JetShow for Option<T> {
    fn jet_show(&self) -> String {
        match self {
            Some(v) => v.jet_show(),
            None => "null".to_string(),
        }
    }
}
fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{}", file, line);
    std::process::exit(70);
}
// D-NUMOPS1: plain integer arithmetic traps on overflow (safe by default) — a
// silent corruption becomes a caught bug. Each `+`/`-`/`*`/`/` on a fixed-width
// integer lowers to one of these, which panic with the source location instead
// of wrapping. `wrapping(…)`/`saturating(…)`/`checked(…)` opt out at the use
// site. Floats and `#Numeric` distinct types keep the plain Rust operators.
trait JetArith: Copy {
    fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self;
}
macro_rules! jet_arith_impl {
    ($($t:ty),*) => { $(
        impl JetArith for $t {
            fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_add(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this addition overflows the value's type (the result is outside its range)")))
            }
            fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_sub(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this subtraction overflows the value's type (the result is outside its range)")))
            }
            fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_mul(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this multiplication overflows the value's type (the result is outside its range)")))
            }
            fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_div(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this division can't be done (dividing by zero, or overflow)")))
            }
        }
    )* };
}
jet_arith_impl!(i8, i16, i32, i64, u8, u16, u32, u64);
/// E3001 (E2-M12, D-OBS1/D-OBS2): rich panic report — includes the function name,
/// a source-line context box, and (in debug builds only) safe local variable values.
/// `col` is 1-based; `caret_len` covers the highlighted span in the source line.
/// `locals` is an empty string in release builds; "x = 1, y = false" in debug builds.
fn jet_panic_rich(
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    msg: &str,
    locals: &str,
) -> ! {
    let line_s = line.to_string();
    let margin = line_s.len();
    let pad = " ".repeat(margin);
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{} in {}", file, line, fn_name);
    eprintln!("   {}|", pad);
    eprintln!("{} | {}", line_s, src_line);
    let col_offset = col.saturating_sub(1) as usize;
    let caret = "^".repeat(caret_len.max(1) as usize);
    eprintln!("   {}| {}{}", pad, " ".repeat(col_offset), caret);
    if !locals.is_empty() {
        eprintln!("locals: {}", locals);
    }
    std::process::exit(70);
}
/// E3002 (E2-M12, D-OBS1): error-return trace frame. In debug builds, when a `?`
/// actually propagates an `Err`, print one Zig-style frame to stderr, then hand
/// the `Result` back unchanged so the caller's `?` proceeds (incl. any
/// `From`/`to_error` conversion). In release builds this is a no-op.
fn jet_trace_err<T, E>(r: Result<T, E>, file: &str, line: u32, fn_name: &str) -> Result<T, E> {
    if cfg!(debug_assertions) && r.is_err() {
        eprintln!("error propagated from: {} ({}:{}) via ?", fn_name, file, line);
    }
    r
}
fn jet_index_vec<T: Clone>(xs: &Vec<T>, i: i64, file: &str, line: u32) -> T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(file, line, &format!("the list has {} items, so position {} doesn't exist", len, i));
    }
    xs[i as usize].clone()
}
fn jet_unpack_vec<T: Clone>(xs: &Vec<T>, want: usize, i: usize, file: &str, line: u32) -> T {
    if xs.len() != want {
        jet_panic(file, line, &format!("this pattern needs exactly {} item{}, but the list has {}", want, if want == 1 { "" } else { "s" }, xs.len()));
    }
    xs[i].clone()
}
fn jet_slice_vec<T: Clone>(xs: &Vec<T>, a: i64, b: i64, file: &str, line: u32) -> Vec<T> {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(file, line, &format!("can't slice {} items from {} to {} (inclusive)", len, a, b));
    }
    xs[a as usize..=b as usize].to_vec()
}
fn jet_index_map<K: Ord + Clone, V: Clone>(m: &std::collections::BTreeMap<K, V>, k: &K, file: &str, line: u32) -> V {
    match m.get(k) {
        Some(v) => v.clone(),
        None => jet_panic(file, line, &format!("the map has no entry for this key")),
    }
}
fn jet_map_insert<K: Ord, V>(m: &mut std::collections::BTreeMap<K, V>, k: K, v: V) {
    m.insert(k, v);
}
fn jet_list_remove<T: Clone>(xs: &mut Vec<T>, i: i64, file: &str, line: u32) -> T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(file, line, &format!("the list has {} items, so position {} doesn't exist", len, i));
    }
    xs.remove(i as usize)
}
fn jet_char_len(s: &String) -> i64 { s.chars().count() as i64 }
fn jet_string_split(s: &String, sep: &str) -> Vec<String> { s.split(sep).map(|x| x.to_string()).collect() }
fn jet_string_lines(s: &String) -> Vec<String> { s.lines().map(|x| x.to_string()).collect() }
fn jet_string_slice(s: &String, a: i64, b: i64, file: &str, line: u32) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(file, line, &format!("can't slice {} characters from {} to {} (inclusive)", len, a, b));
    }
    chars[a as usize..=b as usize].iter().collect()
}
fn jet_list_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U> where F: Fn(T) -> U {
    xs.into_iter().map(f).collect()
}
fn jet_list_map_mut<T, U, F>(xs: Vec<T>, mut f: F) -> Vec<U> where F: FnMut(T) -> U {
    xs.into_iter().map(|x| f(x)).collect()
}
fn jet_list_filter<T: Clone, F>(xs: Vec<T>, f: F) -> Vec<T> where F: Fn(T) -> bool {
    xs.into_iter().filter(|x| f(x.clone())).collect()
}
fn jet_list_each<T, F>(xs: Vec<T>, f: F) where F: Fn(T) {
    for x in xs { f(x); }
}
fn jet_list_each_ref<T, F>(xs: &Vec<T>, f: F) where F: Fn(&T) {
    for x in xs.iter() { f(x); }
}
fn jet_list_each_mut<T, F>(xs: Vec<T>, mut f: F) where F: FnMut(T) {
    for x in xs { f(x); }
}
fn jet_list_find<T: Clone, F>(xs: Vec<T>, f: F) -> Option<T> where F: Fn(T) -> bool {
    xs.into_iter().find(|x| f(x.clone()))
}
fn jet_list_any<T: Clone, F>(xs: Vec<T>, f: F) -> bool where F: Fn(T) -> bool {
    xs.iter().any(|x| f(x.clone()))
}
fn jet_list_all<T: Clone, F>(xs: Vec<T>, f: F) -> bool where F: Fn(T) -> bool {
    xs.iter().all(|x| f(x.clone()))
}
fn jet_list_sort_by<T: Clone, K: Ord, F>(xs: &mut Vec<T>, f: F) where F: Fn(T) -> K {
    xs.sort_by_key(|x| f(x.clone()));
}
fn jet_list_reduce<T, U, F>(xs: Vec<T>, init: U, f: F) -> U where F: Fn(U, T) -> U {
    xs.into_iter().fold(init, f)
}
fn jet_map_each<K: Ord, V, F>(m: std::collections::BTreeMap<K, V>, f: F) where F: Fn(K, V) {
    for (k, v) in m { f(k, v); }
}
// ── D-DEFER1 option B: core.scope.guard ──────────────────────────────────────
// A ScopeGuard stores a zero-argument closure and runs it in Drop — on every
// exit path (normal fall-through, early `return`, `?` propagation).
// LIFO ordering is guaranteed by Rust's reverse-declaration drop order.
// Generic over F: avoids boxing and allows non-'static captures. Purely safe.
struct JetScopeGuard<F: FnOnce()> {
    f: Option<F>,
}
fn jet_scope_guard<F: FnOnce()>(f: F) -> JetScopeGuard<F> {
    JetScopeGuard { f: Some(f) }
}
impl<F: FnOnce()> Drop for JetScopeGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.f.take() { f(); }
    }
}
trait user_Serialize { fn to_json(&self) -> String; }
