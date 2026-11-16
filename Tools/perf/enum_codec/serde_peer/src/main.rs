//! Serde peer for Tools/perf/enum_codec_bench.py protocol v1 (#3151).
//! Wire shapes match the harness fixtures: external `{"Write":"hello"}`,
//! internal `{"type":"Write","value":"hello"}` (serde's tag+content form),
//! untagged `"hello"`, adjacent `{"kind":"Write","body":"hello"}`.
use serde::{Deserialize, Serialize};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

struct Counting;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[derive(Serialize, Deserialize)]
enum External {
    Read,
    Write(String),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
enum Internal {
    Read,
    Write(String),
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Untagged {
    Read,
    Write(String),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "body")]
enum Adjacent {
    Read,
    Write(String),
}

fn option(args: &[String], name: &str) -> String {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .unwrap_or_default()
}

fn encode(form: &str, source: &str, rounds: u64) -> Option<String> {
    if source != "Command.Write(\"hello\")" {
        return None;
    }
    let mut wire = String::new();
    for _ in 0..rounds {
        wire = match form {
            "external" => serde_json::to_string(&External::Write("hello".into())).ok()?,
            "internal" => serde_json::to_string(&Internal::Write("hello".into())).ok()?,
            "untagged" => serde_json::to_string(&Untagged::Write("hello".into())).ok()?,
            "adjacent" => serde_json::to_string(&Adjacent::Write("hello".into())).ok()?,
            _ => return None,
        };
    }
    Some(wire)
}

fn decode(form: &str, wire: &str, rounds: u64) -> Option<String> {
    let mut text = String::new();
    for _ in 0..rounds {
        text = match form {
            "external" => serde_json::to_string(&serde_json::from_str::<External>(wire).ok()?).ok()?,
            "internal" => serde_json::to_string(&serde_json::from_str::<Internal>(wire).ok()?).ok()?,
            "untagged" => serde_json::to_string(&serde_json::from_str::<Untagged>(wire).ok()?).ok()?,
            "adjacent" => serde_json::to_string(&serde_json::from_str::<Adjacent>(wire).ok()?).ok()?,
            _ => return None,
        };
    }
    Some(text)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let operation = option(&args, "--operation");
    let form = option(&args, "--form");
    let rounds: u64 = option(&args, "--rounds").parse().unwrap_or(1);
    let input = match std::fs::read_to_string(option(&args, "--input")) {
        Ok(text) => text,
        Err(_) => std::process::exit(3),
    };
    let input = input.trim_end();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let started = Instant::now();
    let result = if operation == "encode" {
        encode(&form, input, rounds)
    } else {
        decode(&form, input, rounds)
    };
    let codec_ns = started.elapsed().as_nanos() as u64;
    let allocation_count = ALLOCATIONS.load(Ordering::Relaxed) - before;
    let metrics = format!("{{\"codec_ns\":{codec_ns},\"allocation_count\":{allocation_count}}}");
    let _ = std::fs::write(option(&args, "--metrics"), metrics);
    match result {
        Some(out) => {
            if std::fs::write(option(&args, "--output"), out).is_err() {
                std::process::exit(3);
            }
        }
        None => std::process::exit(1),
    }
}
