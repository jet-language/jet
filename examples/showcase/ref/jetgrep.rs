//! Reference implementation for M14 benchmark — jetgrep equivalent.
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

struct Config {
    pattern: String,
    recursive: bool,
    ignore_case: bool,
    line_numbers: bool,
    count_only: bool,
    paths: Vec<String>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match parse_flags(&args) {
        Ok(cfg) => run(cfg),
        Err(msg) => {
            eprintln!("{msg}");
            process::exit(2);
        }
    }
}

fn parse_flags(args: &[String]) -> Result<Config, String> {
    let mut recursive = false;
    let mut ignore_case = false;
    let mut line_numbers = false;
    let mut count_only = false;
    let mut pattern = String::new();
    let mut paths = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let flag = &args[i];
        match flag.as_str() {
            "-r" => recursive = true,
            "-i" => ignore_case = true,
            "-n" => line_numbers = true,
            "-c" => count_only = true,
            "-h" | "--help" => return Err("usage: jetgrep [-r] [-i] [-n] [-c] pattern [path ...]".into()),
            s if s.starts_with('-') => return Err(format!("unknown flag: {s}")),
            s if pattern.is_empty() => pattern = s.to_string(),
            s => paths.push(s.to_string()),
        }
        i += 1;
    }
    if pattern.is_empty() {
        return Err("usage: jetgrep [-r] [-i] [-n] [-c] pattern [path ...]".into());
    }
    Ok(Config {
        pattern,
        recursive,
        ignore_case,
        line_numbers,
        count_only,
        paths,
    })
}

fn run(cfg: Config) {
    let targets = if cfg.paths.is_empty() {
        vec![".".to_string()]
    } else {
        cfg.paths.clone()
    };
    let mut files = Vec::new();
    for path in targets {
        let found = collect_files(Path::new(&path), cfg.recursive);
        if found.is_empty() && Path::new(&path).is_file() {
            files.push(path);
        } else {
            files.extend(found);
        }
    }
    let needle = if cfg.ignore_case {
        cfg.pattern.to_lowercase()
    } else {
        cfg.pattern.clone()
    };
    let mut total = 0;
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else { continue };
        let hay = if cfg.ignore_case {
            text.to_lowercase()
        } else {
            text.clone()
        };
        let mut hits = 0;
        for (i, line) in hay.lines().enumerate() {
            if line.contains(&needle) {
                hits += 1;
                if !cfg.count_only {
                    if cfg.line_numbers {
                        println!("{file}:{} {line}", i + 1);
                    } else {
                        println!("{file} {line}");
                    }
                }
            }
        }
        if cfg.count_only && hits > 0 {
            println!("{file} {hits}");
        }
        total += hits;
    }
    process::exit(if total > 0 { 0 } else { 1 });
}

fn collect_files(path: &Path, recursive: bool) -> Vec<String> {
    let mut out = Vec::new();
    if path.is_dir() {
        if !recursive {
            return out;
        }
        let Ok(entries) = fs::read_dir(path) else { return out };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.extend(collect_files(&p, true));
            } else if p.is_file() {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    } else if path.is_file() {
        out.push(path.to_string_lossy().into_owned());
    }
    out
}
