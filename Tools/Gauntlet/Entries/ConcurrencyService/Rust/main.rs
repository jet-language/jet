use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

struct Job {
    value: i64,
    result: mpsc::SyncSender<i64>,
}

fn worker(jobs: Arc<Mutex<mpsc::Receiver<Job>>>) {
    loop {
        let job = match jobs.lock().expect("job lock").recv() {
            Ok(job) => job,
            Err(_) => return,
        };
        thread::sleep(Duration::from_millis(1));
        let _ = job.result.send(job.value * job.value);
    }
}

fn run_batch(count: i64, jobs: &mpsc::SyncSender<Job>) -> i64 {
    let (result_tx, result_rx) = mpsc::sync_channel(count as usize);
    for value in 1..=count {
        jobs.send(Job { value, result: result_tx.clone() }).expect("worker queue");
    }
    let mut total = 0;
    for _ in 0..count {
        total += result_rx.recv().expect("worker result");
    }
    total
}

fn serve(mut stream: TcpStream, jobs: &mpsc::SyncSender<Job>) -> bool {
    let mut command = String::new();
    if BufReader::new(stream.try_clone().expect("clone stream")).read_line(&mut command).is_err() {
        return false;
    }
    let command = command.trim();
    let mut response = "error".to_string();
    let mut stop = false;
    if command == "ready" {
        response = "ready".to_string();
    } else if command == "shutdown" {
        response = "bye".to_string();
        stop = true;
    } else if let Some(raw_count) = command.strip_prefix("batch ") {
        if let Ok(count) = raw_count.parse::<i64>() {
            if (1..=32).contains(&count) {
                response = format!("batch {count} total {}", run_batch(count, jobs));
            }
        }
    }
    let _ = writeln!(stream, "{response}");
    stop
}

fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "18080".to_string());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).expect("bind");
    let (job_tx, job_rx) = mpsc::sync_channel::<Job>(32);
    let jobs = Arc::new(Mutex::new(job_rx));
    let workers: Vec<_> = (0..4).map(|_| {
        let jobs = Arc::clone(&jobs);
        thread::spawn(move || worker(jobs))
    }).collect();

    for stream in listener.incoming() {
        let stream = stream.expect("accept");
        if serve(stream, &job_tx) {
            break;
        }
    }
    drop(job_tx);
    for worker in workers {
        let _ = worker.join();
    }
}
