// The Rust peer of int_mod_div.jet: the same kernel, rounds and output line.
//
//   rustc -O -C target-cpu=native -o /tmp/int_mod_div Tools/Perf/ArithKernels/int_mod_div.rs && /tmp/int_mod_div [TICKS]
use std::env;
use std::hint::black_box;
use std::time::Instant;

const TICKS: i64 = 10_000_000;
const ROUNDS: usize = 5;

fn mod_div_kernel(ticks: i64) -> i64 {
    let mut check = 0i64;
    for tick in 0..ticks {
        let raw = (tick * 73 + 19) % 4_096;
        let frame = (raw * 17 + tick * 31 + 7) % 65_536;
        check = (check * 33 + frame / 7 + frame % 13 + raw / 8 + tick % 127) % 1_000_000_007;
    }
    check
}

fn main() {
    let ticks: i64 = env::args().nth(1).and_then(|text| text.parse().ok()).unwrap_or(TICKS);

    let mut best = u128::MAX;
    let mut check = 0i64;
    for _ in 0..ROUNDS {
        let started = Instant::now();
        check = mod_div_kernel(black_box(ticks));
        best = best.min(started.elapsed().as_nanos());
    }
    println!("int_mod_div ticks={ticks} best_ns={best} check={check}");
}
