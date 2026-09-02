use std::env;
use std::fs;
use std::path::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use std::thread;

fn collect_files(path: &Path, files: &mut Vec<String>) {
    for entry in fs::read_dir(path).expect("list failed") {
        let entry = entry.expect("entry failed");
        let child = entry.path();
        let kind = entry.file_type().expect("entry type failed");
        if kind.is_dir() {
            collect_files(&child, files);
        } else if kind.is_file()
            && child.extension().and_then(|extension| extension.to_str()) == Some("txt")
        {
            files.push(child.to_string_lossy().into_owned());
        }
    }
}

fn count_file(path: &str, needle: &str) -> (String, usize) {
    let text = fs::read_to_string(path).expect("read failed");
    let count = text.lines().map(|line| line.matches(needle).count()).sum();
    (path.to_owned(), count)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let root = args.get(1).map(String::as_str).unwrap_or("files");
    let needle = args.get(2).map(String::as_str).unwrap_or("needle-7f");
    let mut paths = Vec::new();
    collect_files(Path::new(root), &mut paths);
    paths.sort();
    let next = AtomicUsize::new(0);
    let matches = Mutex::new(Vec::new());
    let workers = thread::available_parallelism().map_or(1, |count| count.get());
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let mut batch = Vec::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= paths.len() {
                        break;
                    }
                    let result = count_file(&paths[index], needle);
                    if result.1 > 0 {
                        batch.push(result);
                    }
                }
                matches.lock().expect("results lock").extend(batch);
            });
        }
    });
    let mut matches = matches.into_inner().expect("results lock");
    matches.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let total: usize = matches.iter().map(|(_, count)| count).sum();
    for (path, count) in &matches {
        println!("{path}:{count}");
    }
    println!("files {}/{} total {total}", matches.len(), paths.len());
}
