use std::env;
use std::fs;
use std::thread;

fn count_file(path: String, needle: &str) -> (String, usize) {
    let text = fs::read_to_string(&path).expect("read failed");
    let count = text.lines().map(|line| line.matches(needle).count()).sum();
    (path, count)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let root = args.get(1).map(String::as_str).unwrap_or("files");
    let needle = args.get(2).map(String::as_str).unwrap_or("needle-7f");
    let mut paths: Vec<String> = fs::read_dir(root)
        .expect("list failed")
        .map(|entry| entry.expect("entry failed").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    paths.sort();
    let results = thread::scope(|scope| {
        paths
            .iter()
            .map(|path| scope.spawn(move || count_file(path.clone(), needle)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().expect("worker failed"))
            .collect::<Vec<_>>()
    });
    let mut matches: Vec<_> = results.into_iter().filter(|(_, count)| *count > 0).collect();
    matches.sort_by(|left, right| left.0.cmp(&right.0));
    let total: usize = matches.iter().map(|(_, count)| count).sum();
    for (path, count) in &matches {
        println!("{path}:{count}");
    }
    println!("files {}/{} total {total}", matches.len(), paths.len());
}
