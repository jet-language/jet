use std::io;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> io::Result<()> {
    let calc = Command::new("python3")
        .args(["-c", "print(6*7)"])
        .output()?;
    println!("calc {}", String::from_utf8_lossy(&calc.stdout).trim());

    let mut source = Command::new("python3")
        .args(["-c", "print('b');print('a');print('c')"])
        .stdout(Stdio::piped())
        .spawn()?;
    let source_stdout = source.stdout.take().expect("source stdout");
    let sorter = Command::new("sort")
        .stdin(source_stdout)
        .stdout(Stdio::piped())
        .spawn()?;
    let sorted = sorter.wait_with_output()?;
    source.wait()?;
    let sorted_text = String::from_utf8_lossy(&sorted.stdout);
    let lines: Vec<&str> = sorted_text.lines().collect();
    println!("sorted {}", lines.join(","));

    let mut slow = Command::new("python3")
        .args(["-c", "import time;time.sleep(5)"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_millis(300);
    loop {
        if slow.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            slow.kill()?;
            slow.wait()?;
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    println!("slow timeout");

    let checked = Command::new("python3")
        .args(["-c", "import sys;sys.exit(3)"])
        .status()?;
    if !checked.success() {
        println!("exit {}", checked.code().unwrap_or(-1));
    }
    Ok(())
}
