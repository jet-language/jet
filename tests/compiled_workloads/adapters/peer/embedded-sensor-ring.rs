// #1414 embedded sensor-ring peer adapter.
// Provenance: Embassy revision e6ac13bad5787f57fd76ee1e22e5cc15351105a7.
// The host replay keeps the embedded queue's fixed storage and explicit
// head/tail/count state. It does not call Embassy or another peer binary.
use std::{env, fs};

const CAPACITY: usize = 4;
const DIGEST_MODULUS: i64 = 1_000_003;

struct RingBuffer {
    slots: [i64; CAPACITY],
    head: usize,
    tail: usize,
    count: usize,
}

impl RingBuffer {
    fn enqueue(&mut self, value: i64) -> bool {
        if self.count == CAPACITY {
            return false;
        }
        self.slots[self.tail] = value;
        self.tail = (self.tail + 1) % CAPACITY;
        self.count += 1;
        true
    }

    fn dequeue(&mut self) -> Option<i64> {
        if self.count == 0 {
            return None;
        }
        let value = self.slots[self.head];
        self.head = (self.head + 1) % CAPACITY;
        self.count -= 1;
        Some(value)
    }
}

fn fold_digest(
    digest: i64,
    step: i64,
    code: i64,
    value: i64,
    ring: &RingBuffer,
) -> i64 {
    let value_part = value.rem_euclid(DIGEST_MODULUS);
    (digest * 131
        + step * 17
        + code * 257
        + value_part * 3
        + ring.head as i64 * 5
        + ring.tail as i64 * 7
        + ring.count as i64 * 11)
        .rem_euclid(DIGEST_MODULUS)
}

fn print_state(
    step: usize,
    op: &str,
    result: &str,
    value: Option<i64>,
    ring: &RingBuffer,
) {
    match value {
        Some(value) => println!(
            "step={step}|op={op}|value={value}|result={result}|head={}|tail={}|count={}",
            ring.head, ring.tail, ring.count
        ),
        None => println!(
            "step={step}|op={op}|result={result}|head={}|tail={}|count={}",
            ring.head, ring.tail, ring.count
        ),
    }
}

fn main() {
    let input = env::args().nth(1).expect("input");
    let raw = fs::read_to_string(input).expect("read input");
    let mut ring = RingBuffer {
        slots: [0; CAPACITY],
        head: 0,
        tail: 0,
        count: 0,
    };
    let mut step = 0_i64;
    let mut accepted_enqueues = 0_i64;
    let mut accepted_dequeues = 0_i64;
    let mut rejects = 0_i64;
    let mut digest = 17_i64;

    println!("capacity=4");
    for line in raw.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        step += 1;
        let mut fields = line.split('|');
        let first = fields.next();
        let second = fields.next();
        let has_extra = fields.next().is_some();
        let second_is_none = second.is_none();
        if !has_extra && first == Some("ENQ") {
            match second.and_then(|value| value.parse::<i64>().ok()) {
                Some(value) if ring.enqueue(value) => {
                    accepted_enqueues += 1;
                    digest = fold_digest(digest, step, 1, value, &ring);
                    print_state(step as usize, "enqueue", "accepted", Some(value), &ring);
                }
                Some(value) => {
                    rejects += 1;
                    digest = fold_digest(digest, step, 3, value, &ring);
                    print_state(
                        step as usize,
                        "enqueue",
                        "reject-overflow",
                        Some(value),
                        &ring,
                    );
                }
                None => {
                    rejects += 1;
                    digest = fold_digest(digest, step, 5, 0, &ring);
                    print_state(
                        step as usize,
                        "malformed",
                        "reject-malformed",
                        None,
                        &ring,
                    );
                }
            }
        } else if !has_extra && first == Some("DEQ") && second_is_none {
            match ring.dequeue() {
                Some(value) => {
                    accepted_dequeues += 1;
                    digest = fold_digest(digest, step, 2, value, &ring);
                    print_state(step as usize, "dequeue", "accepted", Some(value), &ring);
                }
                None => {
                    rejects += 1;
                    digest = fold_digest(digest, step, 4, 0, &ring);
                    print_state(
                        step as usize,
                        "dequeue",
                        "reject-underflow",
                        None,
                        &ring,
                    );
                }
            }
        } else {
            rejects += 1;
            digest = fold_digest(digest, step, 5, 0, &ring);
            print_state(
                step as usize,
                "malformed",
                "reject-malformed",
                None,
                &ring,
            );
        }
    }

    println!(
        "summary|accepted_enqueues={accepted_enqueues}|accepted_dequeues={accepted_dequeues}|rejects={rejects}|head={}|tail={}|count={}",
        ring.head, ring.tail, ring.count
    );
    println!("digest={digest}");
}
