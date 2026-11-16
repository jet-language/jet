struct Pair<T> {
    left: T,
    right: T,
}

fn identity<T>(value: T) -> T {
    value
}

fn sum_pair(pair: &Pair<i64>) -> i64 {
    pair.left + pair.right
}

fn make_table() -> Vec<i64> {
    (0..7).map(|i| (i * i) % 7).collect()
}

fn main() {
    let pair = Pair { left: 17_i64, right: 25_i64 };
    let first = identity(pair.left);
    let table = make_table();
    println!("generic_sum={} first={}", sum_pair(&pair), first);
    println!("comptime_value={} fuel={}", table[3], table.len());
}
