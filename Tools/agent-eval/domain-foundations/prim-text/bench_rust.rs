use std::collections::HashMap;
use std::fs;
use std::time::Instant;

fn merge_sequence(sequence: &[String], best: &(String, String)) -> Vec<String> {
    let mut out = Vec::with_capacity(sequence.len());
    let mut index = 0;
    while index < sequence.len() {
        if index + 1 < sequence.len()
            && sequence[index] == best.0
            && sequence[index + 1] == best.1
        {
            out.push(format!("{}{}", best.0, best.1));
            index += 2;
        } else {
            out.push(sequence[index].clone());
            index += 1;
        }
    }
    out
}

fn main() {
    let root = "/home/nate/.cache/jet-luna/dx3/prim-text";
    let corpus_path = format!("{root}/fixtures/corpus_1m.txt");
    let log_path = format!("{root}/fixtures/logs_100m.txt");
    let corpus = fs::read_to_string(&corpus_path).expect("read corpus");
    let corpus_bytes = fs::metadata(&corpus_path).expect("stat corpus").len();

    let started = Instant::now();
    let mut sequences: Vec<Vec<String>> = corpus
        .split(' ')
        .map(|word| word.chars().map(|ch| ch.to_string()).collect())
        .collect();
    let words = sequences.len();
    let mut merges = 0;
    for _ in 0..12 {
        let mut counts: HashMap<(String, String), usize> = HashMap::new();
        for sequence in &sequences {
            for pair in sequence.windows(2) {
                *counts
                    .entry((pair[0].clone(), pair[1].clone()))
                    .or_insert(0) += 1;
            }
        }
        let Some((best, _)) = counts.into_iter().max_by_key(|(_, count)| *count) else {
            break;
        };
        sequences = sequences
            .iter()
            .map(|sequence| merge_sequence(sequence, &best))
            .collect();
        merges += 1;
    }
    let tokens: usize = sequences.iter().map(Vec::len).sum();
    let bpe_ms = started.elapsed().as_secs_f64() * 1000.0;

    let started = Instant::now();
    let log = fs::read_to_string(&log_path).expect("read logs");
    let mut lines = 0;
    let mut matches = 0;
    for line in log.lines() {
        lines += 1;
        if line.contains("ERROR") && line.contains("user_id=") {
            matches += 1;
        }
    }
    let scan_ms = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "bpe bytes={corpus_bytes} words={words} merges={merges} tokens={tokens} sample_tokens=10 elapsed_ms={bpe_ms:.3}"
    );
    println!(
        "scan bytes={} lines={lines} matches={matches} elapsed_ms={scan_ms:.3}",
        fs::metadata(&log_path).expect("stat logs").len()
    );
}
