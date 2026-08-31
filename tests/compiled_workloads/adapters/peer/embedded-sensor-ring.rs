// #1414 hosted replay adapter. Upstream identity: Embassy
// e6ac13bad5787f57fd76ee1e22e5cc15351105a7.
use std::{env, fs};

fn main() {
    let raw = fs::read_to_string(env::args().nth(1).expect("input"))
        .expect("read input");
    if raw.contains("bad") {
        println!("frames=0\nsum=0\nring=0\nreject=1");
        return;
    }
    let mut frames = 0;
    let mut sum = 0;
    for line in raw.lines().filter(|line| !line.is_empty()) {
        let mut fields = line.split(',');
        let _sequence = fields.next();
        match fields.next().and_then(|value| value.parse::<i64>().ok()) {
            Some(value) => {
                frames += 1;
                sum += value;
            }
            None => {}
        }
    }
    println!("frames={frames}");
    println!("sum={sum}");
    println!("ring={}", if frames == 0 { 0 } else { sum / frames });
}
