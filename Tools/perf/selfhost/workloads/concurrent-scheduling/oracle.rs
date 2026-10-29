use std::thread;

fn work(value: i64) -> i64 {
    value * 2
}

fn main() {
    let first = thread::spawn(|| work(5));
    let second = thread::spawn(|| work(7));
    let mut values = vec![first.join().expect("first task"), second.join().expect("second task")];
    values.sort_unstable();
    println!("results={},{} order=sorted", values[0], values[1]);
}
