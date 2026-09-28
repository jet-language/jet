use std::{env, fs};

fn summary(values: &[f64]) -> String {
    let mut ordered = values.to_vec();
    ordered.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = ordered.len();
    let mean = values.iter().sum::<f64>() / n as f64;
    let median = ordered[(n - 1) / 2];
    let p95 = ordered[((19 * n + 19) / 20) - 1];
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / n as f64;
    format!(
        "n={n} mean={mean:.2} median={median:.2} p95={p95:.2} sd={:.2}",
        variance.sqrt()
    )
}

fn main() {
    let input = fs::read_to_string(env::args().nth(1).unwrap()).unwrap();
    let mut groups = [
        ("alpha", Vec::new()),
        ("beta", Vec::new()),
        ("delta", Vec::new()),
        ("epsilon", Vec::new()),
        ("gamma", Vec::new()),
    ];
    for line in input.lines().skip(1) {
        let (group, value) = line.split_once(',').unwrap();
        let values = groups.iter_mut().find(|(name, _)| *name == group).unwrap();
        values.1.push(value.parse::<f64>().unwrap());
    }
    let mut all = Vec::new();
    for (group, values) in groups {
        all.extend_from_slice(&values);
        println!("{group} {}", summary(&values));
    }
    let mean = all.iter().sum::<f64>() / all.len() as f64;
    println!("overall n={} mean={mean:.2}", all.len());
}
