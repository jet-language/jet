//! Reference wordfreq for M14 benchmarks (std only).
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let paths: Vec<String> = env::args().skip(1).collect();
    let roots = if paths.is_empty() {
        vec![".".to_string()]
    } else {
        paths
    };
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for root in roots {
        for file in collect_files(Path::new(&root)) {
            if let Ok(text) = fs::read_to_string(&file) {
                count_words(&text, &mut counts);
            }
        }
    }
    let mut ranked: Vec<(String, i64)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (word, n) in ranked {
        println!("{word}: {n}");
    }
}

fn count_words(text: &str, counts: &mut BTreeMap<String, i64>) {
    for line in text.to_lowercase().lines() {
        for word in line.split_whitespace() {
            if !word.is_empty() {
                *counts.entry(word.to_string()).or_insert(0) += 1;
            }
        }
    }
}

fn collect_files(path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if path.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    out.extend(collect_files(&p));
                } else if p.extension().and_then(|e| e.to_str()) == Some("txt") {
                    out.push(p.to_string_lossy().into_owned());
                }
            }
        }
    } else if path.extension().and_then(|e| e.to_str()) == Some("txt") {
        out.push(path.to_string_lossy().into_owned());
    }
    out
}
