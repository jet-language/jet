//! Required native C ABI matrix for card #436.
//!
//! Unlike the broad `cffi` suite, this lane never skips. CI supplies explicit
//! C/Rust toolchains and a runner where cross execution is required.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required in the native C ABI matrix"))
}

fn command(program: &str) -> Command {
    let mut parts = program.split_whitespace();
    let mut cmd = Command::new(parts.next().expect("empty command"));
    cmd.args(parts);
    cmd
}

fn run_ok(cmd: &mut Command, label: &str) {
    let output = cmd.output().unwrap_or_else(|e| panic!("could not start {label}: {e}"));
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn binary(root: &Path) -> PathBuf {
    root.join("matrix")
}

#[test]
fn required_native_c_abi_matrix() {
    assert_eq!(required("JET_CFFI_MATRIX_REQUIRED"), "1");
    let cc = required("JET_CFFI_CC");
    let ar = required("JET_CFFI_AR");
    let rustc = required("JET_CFFI_RUSTC");
    let rust_target = std::env::var("JET_CFFI_RUST_TARGET").ok();
    let rust_linker = std::env::var("JET_CFFI_RUST_LINKER").ok();
    let runner = std::env::var("JET_CFFI_RUNNER").ok();
    let abi = required("JET_CFFI_ABI");

    let root = std::env::temp_dir().join(format!(
        "jet-cffi-required-{}-{}",
        std::process::id(),
        abi.replace(['/', ' '], "-")
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let c_source = root.join("matrix.c");
    fs::write(
        &c_source,
        r#"
#include <stddef.h>
#include <stdint.h>

typedef struct { int64_t x; int64_t y; } Coord;
typedef int32_t Status;
typedef uint8_t PacketTag;
typedef union { int64_t ping; struct { int64_t x; int64_t y; } data; } PacketPayload;
typedef struct { PacketTag tag; PacketPayload payload; } Packet;

int64_t coord_sum(Coord p) { return p.x + p.y; }
int32_t status_value(Status s) { return s; }
int32_t packet_value(Packet p) { return (int32_t)p.tag * 100 + (int32_t)p.payload.ping; }
int32_t packet_size(void) { return (int32_t)sizeof(Packet); }
int32_t packet_align(void) { struct A { char c; Packet p; }; return (int32_t)offsetof(struct A, p); }
int32_t packet_payload_offset(void) { return (int32_t)offsetof(Packet, payload); }

typedef int32_t (*callback_t)(int32_t);
int32_t callback_twice(callback_t cb, int32_t x) { return cb(cb(x)); }

#include <pthread.h>
typedef struct { callback_t cb; int32_t x; int32_t out; } Job;
static void *run_job(void *raw) { Job *j = (Job *)raw; j->out = j->cb(j->x); return 0; }
int32_t callback_parallel(callback_t cb) {
    pthread_t threads[4]; Job jobs[4]; int32_t sum = 0;
    for (int i=0;i<4;i++) { jobs[i]=(Job){cb,i,0}; if(pthread_create(&threads[i],0,run_job,&jobs[i])) return -1000; }
    for (int i=0;i<4;i++) { pthread_join(threads[i],0); sum += jobs[i].out; }
    return sum;
}

typedef struct { uint64_t id; uint32_t flags; } Record;
int32_t load_record(uint64_t id, Record *out) {
    if (id == 7) { out->id=70; out->flags=3; return 0; }
    unsigned char *p=(unsigned char *)out; for(size_t i=0;i<sizeof(*out);i++) p[i]=0xA5;
    return 9;
}

int32_t abi_default(int32_t a, int32_t b) { return a + b; }
int32_t abi_explicit(int32_t a, int32_t b) { return a + b; }
"#,
    )
    .unwrap();

    let object = root.join("matrix.o");
    let library = root.join("libjetmatrix.a");
    let mut compile_c = command(&cc);
    compile_c.arg("-c").arg(&c_source).arg("-o").arg(&object);
    run_ok(&mut compile_c, "C compiler");
    let mut archive = command(&ar);
    archive.arg("rcs").arg(&library).arg(&object);
    run_ok(&mut archive, "C archiver");
    fs::write(
        root.join("package.jet"),
        format!(
            "name: \"jet-cffi-matrix\"\nversion: \"0.1.0\"\ndeps: {{ jetmatrix: c@\"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();

    let explicit_abi = match abi.as_str() {
        "sysv64" => "#ABI(sysv64) ",
        "default" => "",
        other => panic!("unsupported matrix ABI {other}"),
    };
    let jet_source = format!(
        r#"use core.mem
use c.jetmatrix as c

#Layout(c)
struct Coord {{ x: Int; y: Int }}
#Layout(c)
enum Status {{ Ok = 0; Lost = 7 }}
#Layout(c, tag: U8)
enum Packet {{ Ping(Int) = 3; Data(x: Int, y: Int) = 7 }}
#Layout(c)
struct Record {{ id: U64; flags: U32 }}

fn increment(x: I32) =[]=> I32 {{ return x + 1 }}

#Extern module c.jetmatrix {{
    fn coord_sum(p: Coord) => Int = "coord_sum"
    fn status_value(s: Status) => I32 = "status_value"
    fn packet_value(p: Packet) => I32 = "packet_value"
    fn packet_size() => I32 = "packet_size"
    fn packet_align() => I32 = "packet_align"
    fn packet_payload_offset() => I32 = "packet_payload_offset"
    fn callback_twice(cb: fn(I32) =[]=> I32, x: I32) => I32 = "callback_twice"
    fn callback_parallel(cb: fn(I32) =[]=> I32) => I32 = "callback_parallel"
    fn load_record(id: U64, out: *Record) => I32 = "load_record"
    fn abi_default(a: I32, b: I32) => I32 = "abi_default"
    {explicit_abi}fn abi_explicit(a: I32, b: I32) => I32 = "abi_explicit"
}}

fn load(id: U64) => Record ? String {{
    slot := Record.{{id: 0, flags: 0}}
    status := I32.{{ 1 }}
    #Unsafe("live non-null out slot; read only after status zero") {{
        p :: mem.Ptr<Record>.from_addr(mem.address_of(slot))
        status = c.load_record(id, p)
        if Int.from_i32(status) == 0 {{ slot = ~p.* }}
    }}
    if Int.from_i32(status) != 0 {{ return Err("status {{status}}") }}
    return Ok(slot)
}}

fn run() {{
    print(c.coord_sum(Coord.{{x: 3, y: 4}}))
    print(c.status_value(Status.Lost))
    print(c.packet_value(Packet.Ping(41)))
    print(c.packet_size())
    print(c.packet_align())
    print(c.packet_payload_offset())
    print(c.callback_twice(increment, 40))
    print(c.callback_parallel(increment))
    print((load(7) ?? panic("success expected")).id)
    if load(8) == {{ .Ok(v) -> {{ print("unexpected {{v.id}}") }} .Err(e) -> {{ print(e) }} }}
    print(c.abi_default(20, 22))
    print(c.abi_explicit(19, 23))
}}
"#
    );
    let jet_path = root.join("matrix.jet");
    fs::write(&jet_path, &jet_source).unwrap();
    let out = jet::compile_with_path(&jet_source, jet_path.to_str().unwrap()).unwrap_or_else(|d| {
        panic!(
            "front end rejected required matrix fixture:\n{}",
            jet::render_diagnostics(jet_path.to_str().unwrap(), &jet_source, &d)
        )
    });
    assert!(!out.rust.contains("/* unsupported"));
    assert!(out.rust.contains("extern \"C\" fn user_increment"));
    assert!(out.rust.contains("#[repr(C, u8)]"));
    assert!(out.rust.contains(&format!("extern \"{}\"", if abi == "default" { "C" } else { &abi })));

    let rust_path = root.join("matrix.rs");
    fs::write(&rust_path, out.rust).unwrap();
    let mut compile_rust = command(&rustc);
    compile_rust.args(["--edition", "2021"]);
    if let Some(target) = &rust_target {
        compile_rust.arg("--target").arg(target);
    }
    if let Some(linker) = &rust_linker {
        compile_rust.arg("-C").arg(format!("linker={linker}"));
    }
    compile_rust
        .arg(&rust_path)
        .arg("-o")
        .arg(binary(&root))
        .arg("-L")
        .arg(format!("native={}", root.display()))
        .arg("-l")
        .arg("static=jetmatrix");
    compile_rust.arg("-l").arg("pthread");
    run_ok(&mut compile_rust, "Rust compiler/linker");

    let mut execute = if let Some(runner) = runner {
        let mut cmd = command(&runner);
        cmd.arg(binary(&root));
        cmd
    } else {
        Command::new(binary(&root))
    };
    let output = execute.output().expect("could not execute generated matrix binary");
    assert!(output.status.success(), "generated matrix failed: {}", String::from_utf8_lossy(&output.stderr));
    let lines: Vec<_> = String::from_utf8(output.stdout).unwrap().lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 12, "unexpected output: {lines:?}");
    assert_eq!(lines[0], "7");
    assert_eq!(lines[1], "7");
    assert_eq!(lines[2], "341");
    assert_eq!(&lines[3..6], &["24", "8", "8"], "wrong native Packet size/alignment/offset");
    assert_eq!(&lines[6..], &["42", "10", "70", "status 9", "42", "42"]);

    let _ = fs::remove_dir_all(root);
}
