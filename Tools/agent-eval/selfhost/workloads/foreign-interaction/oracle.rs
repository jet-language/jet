fn independent_sum(bytes: &[i8]) -> i64 {
    bytes.iter().map(|byte| i64::from(*byte)).sum()
}

fn main() {
    let bytes = [10_i8, 20_i8, 30_i8];
    let total = independent_sum(&bytes);
    println!("foreign_sum={} count={} retention=none", total, bytes.len());
}
