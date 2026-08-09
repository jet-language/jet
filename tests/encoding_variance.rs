//! Card #715 criterion 6: encoding stream variance proof.
//!
//! Production codecs only (json / jsonl / csv / cbor file reader+writer).
//! Hostile I/O schedules force byte splits through the real FileReader/FileWriter
//! seam (`JET_ENC_HOSTILE_*`); no fixture codecs and no transcript substitution.
//!
//! Coverage:
//! - every 2-chunk byte split for inputs with len ≤ 4096
//! - 32 recorded deterministic multi-chunk plans for a larger input
//! - EncodingLimits at limit−1 / limit / limit+1 / overflow
//! - three clean repetitions on the host AOT tier
//!
//! Platform matrix (this environment): linux x86_64 AOT only.
//! aarch64 / macOS / Windows hosts are unavailable here and are recorded as
//! explicit gaps — not claimed as green.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

mod common;

static SEQ: AtomicU64 = AtomicU64::new(0);

const VARIANCE_SEED: u64 = 0x715C6_A11C_E5EEu64;
const LARGE_INPUT_LEN: usize = 8192;
const EVERY_SPLIT_MAX: usize = 4096;
const CHUNK_PLAN_COUNT: usize = 32;
const CLEAN_REPS: usize = 3;

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "jet_enc_variance_{tag}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

#[derive(Clone, Debug)]
struct RunResult {
    exit: i32,
    stdout: String,
    stderr: String,
}

fn host_facts() -> String {
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into())
        .trim()
        .to_string();
    let rustc = Command::new("rustc")
        .arg("-vV")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "rustc unavailable".into());
    let host = rustc
        .lines()
        .find(|l| l.starts_with("host:"))
        .unwrap_or("host: unknown")
        .to_string();
    format!(
        "commit={commit}\nos={}\narch={}\n{host}\nseed={VARIANCE_SEED:#x}\ntier=aot-native\nplatforms_unavailable=aarch64,macos,windows\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

fn write_evidence(name: &str, body: &str) {
    let dir = PathBuf::from("/tmp/tower-burndown-e3");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join(name), body);
}

fn compile_aot(dir: &Path, name: &str, src: &str) -> PathBuf {
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected variance fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|d| d.is_dir()) {
            rustc
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "rustc rejected variance fixture {name}:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    bin
}

fn run_bin(bin: &Path, cwd: &Path, env: &[(&str, &str)]) -> RunResult {
    let mut cmd = Command::new(bin);
    cmd.current_dir(cwd);
    // Fresh hostile TLS each process; clear inherited schedules.
    cmd.env_remove("JET_ENC_HOSTILE_IO");
    cmd.env_remove("JET_ENC_HOSTILE_READ_ONE");
    cmd.env_remove("JET_ENC_HOSTILE_READ_PLAN");
    cmd.env_remove("JET_ENC_HOSTILE_WRITE_MAX");
    cmd.env_remove("JET_ENC_HOSTILE_WRITE_PLAN");
    cmd.env_remove("JET_ENC_HOSTILE_INTERRUPT_READS");
    cmd.env_remove("JET_ENC_HOSTILE_INTERRUPT_WRITES");
    cmd.env_remove("JET_ENC_HOSTILE_FAIL_READ_AFTER");
    cmd.env_remove("JET_ENC_HOSTILE_FAIL_WRITE_AFTER");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    RunResult {
        exit: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn hostile(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![("JET_ENC_HOSTILE_IO".to_string(), "1".to_string())];
    env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    env
}

fn run_hostile(bin: &Path, cwd: &Path, extra: &[(&str, &str)]) -> RunResult {
    let owned = hostile(extra);
    let refs: Vec<_> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    run_bin(bin, cwd, &refs)
}

fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1);
    *state
}

/// 32 deterministic multi-chunk read plans for inputs larger than 4096.
fn recorded_chunk_plans(seed: u64, input_len: usize) -> Vec<Vec<usize>> {
    let mut state = seed;
    let mut plans = Vec::with_capacity(CHUNK_PLAN_COUNT);
    for plan_i in 0..CHUNK_PLAN_COUNT {
        let chunks = 3 + (lcg(&mut state) as usize % 6); // 3..8 chunks
        let mut sizes = Vec::with_capacity(chunks);
        let mut remaining = input_len;
        for c in 0..chunks {
            if c + 1 == chunks {
                sizes.push(remaining.max(1));
                break;
            }
            let span = (remaining / (chunks - c)).max(1);
            let take = 1 + (lcg(&mut state) as usize % span);
            let take = take.min(remaining.saturating_sub(chunks - c - 1).max(1));
            sizes.push(take);
            remaining = remaining.saturating_sub(take);
        }
        // Ensure plan sums cover the input (last entry may overshoot; reader stops at EOF).
        if sizes.iter().sum::<usize>() < input_len {
            sizes.push(input_len);
        }
        assert!(
            !sizes.is_empty(),
            "plan {plan_i} empty for seed {seed:#x}"
        );
        plans.push(sizes);
    }
    plans
}

fn json_reader_src(input_rel: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    input :: files.open("{input_rel}") ?? panic("open")
    reader :: json.reader(^input, encoding.EncodingLimits.safe()) ?? panic("reader")
    count := 0
    loop item, reader {{
        count++
    }}
    print(count)
}}
"#
    )
}

fn jsonl_reader_src(input_rel: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    input :: files.open("{input_rel}") ?? panic("open")
    reader :: jsonl.reader(^input) ?? panic("reader")
    count := 0
    loop item, reader {{
        count++
    }}
    print(count)
}}
"#
    )
}

fn csv_reader_src(input_rel: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.csv as csv
use core.files as files

fn run() {{
    input :: files.open("{input_rel}") ?? panic("open")
    reader :: csv.reader(^input) ?? panic("reader")
    count := 0
    loop item, reader {{
        count++
    }}
    print(count)
}}
"#
    )
}

fn cbor_reader_src(input_rel: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn run() {{
    input :: files.open("{input_rel}") ?? panic("open")
    reader :: cbor.reader(^input) ?? panic("reader")
    count := 0
    loop item, reader {{
        count++
    }}
    print(count)
}}
"#
    )
}

fn json_limit_reader_src(input_rel: &str, max_total: u64) -> String {
    // Drain the full stream: first next() alone can return ObjectStart before
    // max_total_bytes is crossed.
    format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    limits := encoding.EncodingLimits.safe()
    limits.max_total_bytes = Val({max_total})
    input :: files.open("{input_rel}") ?? panic("open")
    reader :: json.reader(^input, limits) ?? panic("reader")
    loop {{
        result :: reader.next()
        if result == {{
            Ok(maybe) -> {{
                if maybe == {{
                    None -> {{
                        print("eof")
                        break
                    }}
                    Val(_) -> {{}}
                }}
            }}
            Err(error) -> {{
                print(error.kind == encoding.EncodingErrorKind.Limit)
                again :: reader.next()
                if again == {{
                    Err(second) -> print(error.reason == second.reason)
                    Ok(_) -> print("not-latched")
                }}
                break
            }}
        }}
    }}
}}
"#
    )
}

fn padded_json_object(target_len: usize) -> Vec<u8> {
    // {"d":"<pad>"}  — keep well-formed JSON of exact length.
    let overhead = 8; // {"d":""}
    assert!(target_len >= overhead, "target too small");
    let pad = target_len - overhead;
    let mut out = Vec::with_capacity(target_len);
    out.extend_from_slice(br#"{"d":""#);
    out.extend(std::iter::repeat(b'x').take(pad));
    out.extend_from_slice(br#""}"#);
    assert_eq!(out.len(), target_len);
    out
}

fn padded_jsonl(target_len: usize) -> Vec<u8> {
    // One JSONL row: {"n":N,"d":"<pad>"}\n sized to target_len.
    let prefix = br#"{"n":1,"d":""#;
    let suffix = br#""}"#;
    let overhead = prefix.len() + suffix.len() + 1; // + newline
    assert!(target_len >= overhead);
    let pad = target_len - overhead;
    let mut out = Vec::with_capacity(target_len);
    out.extend_from_slice(prefix);
    out.extend(std::iter::repeat(b'y').take(pad));
    out.extend_from_slice(suffix);
    out.push(b'\n');
    assert_eq!(out.len(), target_len);
    out
}

fn padded_csv(target_len: usize) -> Vec<u8> {
    // RFC 4180 row: name,payload\r\nada,<pad>\r\n
    let header = b"name,payload\r\n";
    let row_prefix = b"ada,";
    let overhead = header.len() + row_prefix.len() + 2; // trailing \r\n
    assert!(target_len >= overhead);
    let pad = target_len - overhead;
    let mut out = Vec::with_capacity(target_len);
    out.extend_from_slice(header);
    out.extend_from_slice(row_prefix);
    out.extend(std::iter::repeat(b'z').take(pad));
    out.extend_from_slice(b"\r\n");
    assert_eq!(out.len(), target_len);
    out
}

fn padded_cbor_text(target_len: usize) -> Vec<u8> {
    // Major type 3 text string with definite length, UTF-8 'q' payload.
    // Prefer 2-byte length header (0x79 + u16) when payload fits u16.
    let payload_len = target_len.checked_sub(3).expect("cbor target too small");
    assert!(payload_len <= u16::MAX as usize);
    let mut out = Vec::with_capacity(target_len);
    out.push(0x79);
    out.extend_from_slice(&(payload_len as u16).to_be_bytes());
    out.extend(std::iter::repeat(b'q').take(payload_len));
    assert_eq!(out.len(), target_len);
    out
}

fn assert_same(label: &str, baseline: &RunResult, got: &RunResult) {
    assert_eq!(got.exit, baseline.exit, "{label} exit drift: {}", got.stderr);
    assert_eq!(
        got.stdout, baseline.stdout,
        "{label} stdout drift\nbase={:?}\ngot ={:?}",
        baseline.stdout, got.stdout
    );
    assert_eq!(
        got.stderr, baseline.stderr,
        "{label} stderr drift"
    );
}

fn prove_every_byte_split(
    label: &str,
    bin: &Path,
    cwd: &Path,
    input_len: usize,
    baseline: &RunResult,
    log: &mut String,
) {
    assert!(
        input_len <= EVERY_SPLIT_MAX,
        "{label}: every-split input must be ≤ {EVERY_SPLIT_MAX}, got {input_len}"
    );
    let t0 = Instant::now();
    for split in 0..=input_len {
        let rest = (input_len - split).max(1);
        let plan = if split == 0 {
            format!("{rest}")
        } else {
            format!("{split},{rest}")
        };
        let got = run_hostile(bin, cwd, &[("JET_ENC_HOSTILE_READ_PLAN", &plan)]);
        if got.exit != baseline.exit || got.stdout != baseline.stdout {
            panic!(
                "{label} first difference at split={split} plan={plan}\nbase exit={} out={:?}\ngot  exit={} out={:?} err={}",
                baseline.exit, baseline.stdout, got.exit, got.stdout, got.stderr
            );
        }
    }
    // One-byte streaming is the strongest schedule and must also match.
    let one = run_hostile(bin, cwd, &[("JET_ENC_HOSTILE_READ_ONE", "1")]);
    assert_same(&format!("{label} read-one"), baseline, &one);
    log.push_str(&format!(
        "{label}: every-split 0..={input_len} + READ_ONE ok in {:?}\n",
        t0.elapsed()
    ));
}

fn prove_chunk_plans(
    label: &str,
    bin: &Path,
    cwd: &Path,
    plans: &[Vec<usize>],
    baseline: &RunResult,
    log: &mut String,
) {
    let t0 = Instant::now();
    for (i, plan) in plans.iter().enumerate() {
        let plan_s = plan
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let got = run_hostile(bin, cwd, &[("JET_ENC_HOSTILE_READ_PLAN", &plan_s)]);
        if got.exit != baseline.exit || got.stdout != baseline.stdout {
            panic!(
                "{label} chunk plan {i} first difference plan={plan_s}\nbase={:?}\ngot={:?}\nerr={}",
                baseline.stdout, got.stdout, got.stderr
            );
        }
        log.push_str(&format!("{label}: plan[{i}]={plan_s}\n"));
    }
    log.push_str(&format!(
        "{label}: {CHUNK_PLAN_COUNT} chunk plans ok in {:?}\n",
        t0.elapsed()
    ));
}

#[test]
fn encoding_variance_every_split_chunk_plans_limits_and_reps() {
    if !common::have_rustc() {
        eprintln!("note: skipping encoding variance (need rustc)");
        return;
    }

    let mut evidence = host_facts();
    evidence.push_str("criterion=C6\nagent=e3-enc-715-fix\n");
    write_evidence("715-c6-host.txt", &evidence);

    let scratch = Scratch::new("main");
    let dir = scratch.path();

    // --- ≤4096 every-byte-split inputs (exact bound exercised) ---
    let cases: [(&str, Vec<u8>, fn(&str) -> String); 4] = [
        ("json", padded_json_object(EVERY_SPLIT_MAX), json_reader_src),
        ("jsonl", padded_jsonl(2048), jsonl_reader_src),
        ("csv", padded_csv(1024), csv_reader_src),
        ("cbor", padded_cbor_text(EVERY_SPLIT_MAX), cbor_reader_src),
    ];

    let mut log = String::new();
    for (name, bytes, src_fn) in &cases {
        let input = dir.join(format!("{name}_small.in"));
        fs::write(&input, bytes).unwrap();
        let input_rel = input
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let bin = compile_aot(dir, &format!("{name}_small"), &src_fn(&input_rel));
        let baseline = run_bin(&bin, dir, &[]);
        assert_eq!(
            baseline.exit, 0,
            "{name} baseline failed: {}",
            baseline.stderr
        );
        assert!(
            !baseline.stdout.trim().is_empty(),
            "{name} baseline produced empty stdout"
        );

        for rep in 0..CLEAN_REPS {
            let again = run_bin(&bin, dir, &[]);
            assert_same(&format!("{name} clean-rep-{rep}"), &baseline, &again);
        }
        log.push_str(&format!(
            "{name}: {CLEAN_REPS} clean AOT reps ok; baseline_stdout={:?}\n",
            baseline.stdout.trim()
        ));

        prove_every_byte_split(name, &bin, dir, bytes.len(), &baseline, &mut log);
    }

    // --- >4096: 32 recorded deterministic chunk plans ---
    let large = padded_json_object(LARGE_INPUT_LEN);
    assert!(large.len() > EVERY_SPLIT_MAX);
    let large_path = dir.join("json_large.in");
    fs::write(&large_path, &large).unwrap();
    let large_bin = compile_aot(dir, "json_large", &json_reader_src("json_large.in"));
    let large_base = run_bin(&large_bin, dir, &[]);
    assert_eq!(large_base.exit, 0, "large baseline: {}", large_base.stderr);
    for rep in 0..CLEAN_REPS {
        assert_same(
            &format!("json-large clean-rep-{rep}"),
            &large_base,
            &run_bin(&large_bin, dir, &[]),
        );
    }
    let plans = recorded_chunk_plans(VARIANCE_SEED, large.len());
    assert_eq!(plans.len(), CHUNK_PLAN_COUNT);
    let plans_record = plans
        .iter()
        .enumerate()
        .map(|(i, p)| {
            format!(
                "{i}\t{}",
                p.iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    write_evidence("715-c6-chunk-plans.tsv", &format!("# seed={VARIANCE_SEED:#x} len={}\n{plans_record}\n", large.len()));
    prove_chunk_plans("json-large", &large_bin, dir, &plans, &large_base, &mut log);

    // --- limit−1 / limit / limit+1 / overflow on a fixed JSON blob ---
    let limit_bytes = br#"{"a":"abcdefghij"}"#; // 16 bytes wire
    let limit_path = dir.join("limit.in");
    fs::write(&limit_path, limit_bytes).unwrap();
    let wire_len = limit_bytes.len() as u64;
    let limit_cases: [(u64, &str); 4] = [
        (wire_len.saturating_sub(1), "Limit"), // limit−1 → must error
        (wire_len, "ok-or-limit"),             // exact limit
        (wire_len + 1, "ok"),                  // limit+1 → accept
        (1_000_000, "ok"),                     // huge budget → accept
    ];
    for (max_total, expect_kind) in limit_cases {
        let src = json_limit_reader_src("limit.in", max_total);
        let bin = compile_aot(dir, &format!("limit_{max_total}"), &src);
        let mut last: Option<RunResult> = None;
        for rep in 0..CLEAN_REPS {
            let got = run_bin(&bin, dir, &[]);
            assert_eq!(got.exit, 0, "limit {max_total} rep {rep}: {}", got.stderr);
            if let Some(prev) = &last {
                assert_same(&format!("limit-{max_total}-rep-{rep}"), prev, &got);
            }
            last = Some(got);
        }
        let got = last.unwrap();
        match expect_kind {
            "Limit" => {
                assert!(
                    got.stdout.contains("true"),
                    "limit−1 must surface Limit latch, got {:?}",
                    got.stdout
                );
                let lines: Vec<_> = got.stdout.lines().collect();
                assert!(
                    lines.first() == Some(&"true"),
                    "limit−1 first line must be Limit match true, got {:?}",
                    got.stdout
                );
            }
            "ok" => {
                assert!(
                    got.stdout.contains("ok") || got.stdout.contains("eof"),
                    "limit+ / overflow must accept, got {:?}",
                    got.stdout
                );
            }
            "ok-or-limit" => {
                // Exact budget: accept if the implementation counts finished
                // value within budget, else Limit — either is deterministic.
                assert!(
                    got.stdout.contains("ok")
                        || got.stdout.contains("eof")
                        || got.stdout.lines().next() == Some("true"),
                    "exact limit must be deterministic accept or Limit, got {:?}",
                    got.stdout
                );
            }
            _ => unreachable!(),
        }
        // Hostile one-byte reads must preserve the same limit outcome.
        let host = run_hostile(&bin, dir, &[("JET_ENC_HOSTILE_READ_ONE", "1")]);
        assert_same(&format!("limit-{max_total}-hostile"), &got, &host);
        log.push_str(&format!(
            "limit max_total={max_total} expect={expect_kind} stdout={:?}\n",
            got.stdout.trim()
        ));
    }

    // Overflow write path: max_total_bytes = 0 must fail before publishing body.
    let overflow_src = r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {
    limits := encoding.EncodingLimits.safe()
    limits.max_total_bytes = Val(0)
    out :: files.create("overflow.json") ?? panic("create")
    writer :: json.writer(^out, limits, false) ?? panic("writer")
    result :: writer.write(encoding.DataEvent.Text("x"))
    if result == {
        Err(first) -> {
            print(first.kind == encoding.EncodingErrorKind.Limit)
            again :: writer.finish()
            if again == {
                Err(second) -> print(first.reason == second.reason)
                Ok(_) -> print("finish-missed")
            }
        }
        Ok(_) -> print("missed")
    }
}
"#;
    let overflow_bin = compile_aot(dir, "overflow_write", overflow_src);
    let mut overflow_base: Option<RunResult> = None;
    for rep in 0..CLEAN_REPS {
        let got = run_bin(&overflow_bin, dir, &[]);
        assert_eq!(got.exit, 0, "overflow write: {}", got.stderr);
        assert!(
            got.stdout.lines().next() == Some("true"),
            "zero budget must Limit, got {:?}",
            got.stdout
        );
        if let Some(prev) = &overflow_base {
            assert_same(&format!("overflow-rep-{rep}"), prev, &got);
        }
        overflow_base = Some(got);
    }
    let overflow_host = run_hostile(
        &overflow_bin,
        dir,
        &[("JET_ENC_HOSTILE_WRITE_PLAN", "1,1,1,1")],
    );
    assert_same("overflow-write-plan", overflow_base.as_ref().unwrap(), &overflow_host);
    log.push_str("overflow write max_total=0 + WRITE_PLAN ok\n");

    evidence.push_str(&log);
    evidence.push_str("C6_RESULT=met on linux-x86_64 AOT; other native platforms not present in this env\n");
    write_evidence("715-c6-evidence.txt", &evidence);
    eprintln!("{evidence}");
}
