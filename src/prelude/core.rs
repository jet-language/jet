trait JetShow { fn jet_show(&self) -> String; }
impl JetShow for i64 { fn jet_show(&self) -> String { self.to_string() } }
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
    eprintln!("The program stopped: {}", msg);
    eprintln!("  --> {}:{}", file, line);
    std::process::exit(70);
}
fn jet_index_vec<T: Clone>(xs: &Vec<T>, i: i64, file: &str, line: u32) -> T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(file, line, &format!("the list has {} items, so position {} doesn't exist", len, i));
    }
    xs[i as usize].clone()
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
fn jet_char_len(s: &String) -> i64 { s.chars().count() as i64 }
fn jet_string_split(s: &String, sep: &str) -> Vec<String> { s.split(sep).map(|x| x.to_string()).collect() }
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
trait user_Serialize { fn to_json(&self) -> String; }
