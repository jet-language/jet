// Function-boundary cell recursion-aggregate: Rust peer.
struct Stats {
    count: i64,
    sum: i64,
    low: i64,
    high: i64,
}

fn stats(lo: i64, hi: i64) -> Stats {
    if hi - lo <= 4 {
        let mut s = Stats { count: 0, sum: 0, low: 10007, high: 0 };
        for i in lo..hi {
            let v = (i * 7919) % 10007;
            s.count += 1;
            s.sum += v;
            if v < s.low {
                s.low = v;
            }
            if v > s.high {
                s.high = v;
            }
        }
        return s;
    }
    let mid = lo + (hi - lo) / 2;
    let a = stats(lo, mid);
    let b = stats(mid, hi);
    Stats { count: a.count + b.count, sum: a.sum + b.sum, low: a.low.min(b.low), high: a.high.max(b.high) }
}

fn main() {
    let n: i64 = std::env::args().nth(1).expect("n").parse().expect("n");
    let mut acc: i64 = 0;
    for r in 0..n {
        let s = stats(r, r + 100000);
        acc += s.count + s.sum + s.low + s.high;
    }
    println!("{acc}");
}
