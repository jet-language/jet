use super::*;

#[test]
fn perl_bind_launders_parse_failure_as_e3208() {
    if Command::new("perl").arg("-v").output().is_err(){return}
    let dir=isolated_cwd("perl_bind_invalid");let script=dir.join("broken.pl");fs::write(&script,"sub Broken { if ( }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","perl"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("syntax error at"));assert!(!stderr.contains("broken.pl line"));check_snapshot("bind_perl_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[test]
fn ruby_bind_round_trips_datatree_state_timeout_and_cancellation() {
    if Command::new("ruby").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("ruby_bind_round_trip");let script=dir.join("ops.rb");
    let example=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/ruby");fs::copy(example.join("ops.rb"),&script).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","ruby"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"Ruby bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/ruby");assert!(cache.join("libjet_ruby_ops.a").is_file());assert!(cache.join("ops_worker.rb").is_file());assert!(cache.join("ops.provenance").is_file());
    fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated Ruby binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::write(dir.join("cancel.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <unistd.h>
extern int64_t jet_ruby_ops_open(void);
extern const char* jet_ruby_ops_invoke_sleep_call(int64_t,const char*,int64_t);
extern void jet_ruby_ops_cancel(int64_t);
extern void jet_ruby_ops_close(int64_t);
extern int64_t jet_ruby_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_ruby_ops_invoke_sleep_call(handle,"null",60000);code=jet_ruby_ops_take_error();return 0;}
int main(void){handle=jet_ruby_ops_open();if(!handle)return 1;pthread_t thread;if(pthread_create(&thread,0,call,0))return 2;usleep(100000);jet_ruby_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 3;int64_t fresh=jet_ruby_ops_open();if(!fresh)return 4;jet_ruby_ops_close(fresh);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("cancel.c").args(["-L.jet/bindings/ruby","-l:libjet_ruby_ops.a","-lpthread","-o","cancel"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"Ruby cancellation probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let cancel=Command::new(dir.join("cancel")).current_dir(&dir).output().unwrap();assert!(cancel.status.success(),"Ruby cancellation did not clean the worker: {:?}",cancel.status.code());
}

#[test]
fn ruby_bind_launders_parse_failure_as_e3208() {
    if Command::new("ruby").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("ruby_bind_invalid");let script=dir.join("broken.rb");fs::write(&script,"def broken(input)\n  if input\nend\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","ruby"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("syntax error"));assert!(!stderr.contains("broken.rb:"));check_snapshot("bind_ruby_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[test]
fn php_bind_runs_a_persistent_bounded_worker_pool() {
    if Command::new("php").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("php_bind_pool");let script=dir.join("ops.php");
    let example=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/php");fs::copy(example.join("ops.php"),&script).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","php"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"PHP bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/php");assert!(cache.join("libjet_php_ops.a").is_file());assert!(cache.join("ops_worker.php").is_file());let provenance=fs::read_to_string(cache.join("ops.provenance")).unwrap();assert!(provenance.contains("pool_workers=4"));
    fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated PHP binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::write(dir.join("pool.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>
extern int64_t jet_php_ops_open(void);
extern const char* jet_php_ops_invoke_pooled_sleep(int64_t,const char*,int64_t);
extern const char* jet_php_ops_invoke_sleep_call(int64_t,const char*,int64_t);
extern const char* jet_php_ops_invoke_transform(int64_t,const char*,int64_t);
extern void jet_php_ops_cancel(int64_t);
extern void jet_php_ops_close(int64_t);
extern int64_t jet_php_ops_take_error(void);
static int64_t pool;static int64_t codes[4];
static void* parallel_call(void*arg){intptr_t i=(intptr_t)arg;jet_php_ops_invoke_pooled_sleep(pool,"null",5000);codes[i]=jet_php_ops_take_error();return 0;}
static void* cancel_call(void*unused){(void)unused;jet_php_ops_invoke_sleep_call(pool,"null",60000);codes[0]=jet_php_ops_take_error();return 0;}
static int64_t millis(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return (int64_t)t.tv_sec*1000+t.tv_nsec/1000000;}
int main(void){const char*valid="{\"nested\":{},\"list\":[],\"scalar\":1,\"nothing\":null}";pool=jet_php_ops_open();if(!pool)return 1;pthread_t threads[4];int64_t start=millis();for(intptr_t i=0;i<4;i++)if(pthread_create(&threads[i],0,parallel_call,(void*)i))return 2;for(int i=0;i<4;i++)pthread_join(threads[i],0);if(millis()-start>2500)return 3;for(int i=0;i<4;i++)if(codes[i])return 4;jet_php_ops_invoke_sleep_call(pool,"null",100);if(jet_php_ops_take_error()!=2)return 5;jet_php_ops_invoke_transform(pool,valid,5000);int64_t recovery=jet_php_ops_take_error();if(recovery)return 20+(int)recovery;pthread_t cancelled;if(pthread_create(&cancelled,0,cancel_call,0))return 7;usleep(100000);jet_php_ops_cancel(pool);pthread_join(cancelled,0);if(codes[0]!=3)return 8;for(int i=0;i<4;i++){jet_php_ops_invoke_transform(pool,valid,5000);int64_t code=jet_php_ops_take_error();if(code)return 30+(int)code;}jet_php_ops_close(pool);int64_t pools[8];for(int i=0;i<8;i++)if(!(pools[i]=jet_php_ops_open()))return 40+i;if(jet_php_ops_open()!=0||jet_php_ops_take_error()!=1)return 49;for(int i=0;i<8;i++)jet_php_ops_close(pools[i]);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("pool.c").args(["-L.jet/bindings/php","-l:libjet_php_ops.a","-lpthread","-o","pool"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"PHP pool probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let pool=Command::new(dir.join("pool")).current_dir(&dir).output().unwrap();assert!(pool.status.success(),"PHP worker-pool probe failed: {:?}",pool.status.code());
}

#[test]
fn php_bind_launders_parse_failure_as_e3208() {
    if Command::new("php").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("php_bind_invalid");let script=dir.join("broken.php");fs::write(&script,"<?php function broken($input) { if ( }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","php"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("Parse error"));assert!(!stderr.contains("broken.php on line"));check_snapshot("bind_php_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[test]
fn r_bind_round_trips_datatree_state_and_worker_lifecycle() {
    if Command::new("Rscript").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("r_bind_round_trip");let script=dir.join("ops.R");
    let example=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/r");fs::copy(example.join("ops.R"),&script).unwrap();
    fs::OpenOptions::new().append(true).open(&script).unwrap().write_all(br#"
replace_plot <- function(value) {
  device <- dev.cur()
  dev.off(device)
  writeChar(value, file.path(Sys.getenv("JET_BIND_TEMP"), "plot.svg"), eos = NULL, useBytes = TRUE)
}
hostile_plot <- function(input) {
  kind <- input$kind
  value <- switch(kind,
    script = '<svg xmlns="http://www.w3.org/2000/svg"><script>raw secret script</script></svg>',
    event = '<svg xmlns="http://www.w3.org/2000/svg" onload="raw secret event"><path d="M0 0"/></svg>',
    foreign = '<svg xmlns="http://www.w3.org/2000/svg"><foreignObject>raw secret foreign</foreignObject></svg>',
    external = '<svg xmlns="http://www.w3.org/2000/svg"><use href="https://evil.invalid/raw-secret"/></svg>',
    css = '<svg xmlns="http://www.w3.org/2000/svg"><path style="fill:url(https://evil.invalid/raw-secret)"/></svg>',
    doctype = '<!DOCTYPE svg [<!ENTITY xxe SYSTEM "file:///raw-secret">]><svg xmlns="http://www.w3.org/2000/svg">&xxe;</svg>',
    malformed = '<svg xmlns="http://www.w3.org/2000/svg"><path></svg>',
    oversize = paste0('<svg xmlns="http://www.w3.org/2000/svg"><desc>', strrep('x', 524288), '</desc></svg>'),
    stop('unknown hostile plot'))
  replace_plot(value)
}
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","r"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"R bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/r");assert!(cache.join("libjet_r_ops.a").is_file());assert!(cache.join("ops_worker.R").is_file());let provenance=fs::read_to_string(cache.join("ops.provenance")).unwrap();assert!(provenance.contains("workers_per_session=1\nmax_sessions=32\ntransport=jsonlite\n"));assert!(!provenance.to_ascii_lowercase().contains("cran"));
    fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated R binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::write(dir.join("lifecycle.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
extern int64_t jet_r_ops_open(void);
extern const char* jet_r_ops_invoke_sleep_call(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_sleep_call_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_plot_scores_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_hostile_plot_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_transform(int64_t,const char*,int64_t);
extern void jet_r_ops_cancel(int64_t);
extern void jet_r_ops_close(int64_t);
extern int64_t jet_r_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_r_ops_invoke_sleep_call_plot(handle,"1",60000);code=jet_r_ops_take_error();return 0;}
static int hostile(int64_t h,const char*kind){char input[64];snprintf(input,sizeof(input),"{\"kind\":\"%s\"}",kind);const char*response=jet_r_ops_invoke_hostile_plot_plot(h,input,5000);if(jet_r_ops_take_error()!=0||!response||!strstr(response,"\"ok\":false")||strstr(response,"secret"))return 1;return 0;}
int main(void){handle=jet_r_ops_open();if(!handle)return 1;const char*svg=jet_r_ops_invoke_plot_scores_plot(handle,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true")||!strstr(svg,"<svg height=\\\"")||strstr(svg,"<?xml")||strstr(svg,"<script"))return 2;const char*kinds[]={"script","event","foreign","external","css","doctype","malformed","oversize"};for(int i=0;i<8;i++)if(hostile(handle,kinds[i]))return 10+i;const char*recovered=jet_r_ops_invoke_transform(handle,"{\"nested\":{},\"vector\":[1,2],\"scalar\":1,\"nothing\":null}",5000);if(jet_r_ops_take_error()!=0||!recovered||!strstr(recovered,"\"ok\":true"))return 20;jet_r_ops_invoke_sleep_call_plot(handle,"1",100);if(jet_r_ops_take_error()!=2)return 21;int64_t timed=jet_r_ops_open();if(!timed)return 22;svg=jet_r_ops_invoke_plot_scores_plot(timed,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true"))return 23;jet_r_ops_close(timed);handle=jet_r_ops_open();if(!handle)return 24;pthread_t thread;if(pthread_create(&thread,0,call,0))return 25;usleep(100000);jet_r_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 26;int64_t fresh=jet_r_ops_open();if(!fresh)return 27;svg=jet_r_ops_invoke_plot_scores_plot(fresh,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true"))return 28;jet_r_ops_close(fresh);int64_t sessions[32];for(int i=0;i<32;i++)if(!(sessions[i]=jet_r_ops_open()))return 40+i;if(jet_r_ops_open()!=0||jet_r_ops_take_error()!=1)return 72;for(int i=0;i<32;i++)jet_r_ops_close(sessions[i]);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("lifecycle.c").args(["-L.jet/bindings/r","-l:libjet_r_ops.a","-lpthread","-o","lifecycle"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"R lifecycle probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let lifecycle=Command::new(dir.join("lifecycle")).current_dir(&dir).output().unwrap();assert!(lifecycle.status.success(),"R lifecycle probe failed: {:?}",lifecycle.status.code());
}

#[test]
fn r_bind_discovers_functions_without_executing_source() {
    if Command::new("Rscript").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("r_bind_static_discovery");let script=dir.join("static.R");fs::write(&script,r#"stop("discovery executed source")
# fake <- function(input) input
text <- "also_fake <- function(input) input"
outer <- function(input) {
  nested <- function(input) input
  input
}

"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","r"]).arg(&script).args(["--pkg","static_ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"static R discovery failed:\n{}",String::from_utf8_lossy(&bind.stderr));let generated=fs::read_to_string(dir.join(".jet/bindings/r/static_ops.jet")).unwrap();assert!(generated.contains("pub fn outer("));assert!(!generated.contains("pub fn fake("));assert!(!generated.contains("pub fn also_fake("));assert!(!generated.contains("pub fn nested("));
}

#[test]
fn r_bind_launders_parse_failure_as_e3208() {
    if Command::new("Rscript").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("r_bind_invalid");let script=dir.join("broken.R");fs::write(&script,"broken <- function(input) { if ( }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","r"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("unexpected '}'"));assert!(!stderr.contains("broken.R:"));check_snapshot("bind_r_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[cfg(not(target_os="windows"))]
#[test]
fn com_bind_rejects_non_windows_before_reading_input() {
    let output=Command::new(jet()).args(["inspect","bind","com","missing.tlb","--pkg","excel"]).env("NO_COLOR","1").output().unwrap();assert_eq!(output.status.code(),Some(1));assert!(output.stdout.is_empty());check_snapshot("bind_com_non_windows_e3260.txt",&String::from_utf8_lossy(&output.stderr));
}

#[test]
fn ada_bind_launders_gnat_failure_as_e3208() {
    let dir=isolated_cwd("ada_bind_failure");let spec=dir.join("broken.ads");
    fs::write(&spec,"package Broken is function Value (N : Long_Long_Integer) return Long_Long_Integer with Export, Convention => C, External_Name => \"broken_value\"; end Broken;\n").unwrap();
    fs::write(dir.join("broken.adb"),"package body Broken is function Value (N : Long_Long_Integer) return Long_Long_Integer is begin return N +; end Value; end Broken;\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","ada"]).arg(&spec).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));assert!(!stderr.contains("broken.adb:"));
    check_snapshot("bind_ada_invalid_e3208.txt",&scrub(&stderr,&spec));
}

#[test]
fn tcl_bind_missing_source_is_laundered_e3208() {
    let dir=isolated_cwd("tcl_bind_missing");let source=dir.join("missing.tcl");
    let output=Command::new(jet()).args(["inspect","bind","tcl"]).arg(&source).args(["--pkg","missing"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));
    check_snapshot("bind_tcl_missing_e3208.txt",&scrub(&stderr,&source));
}

#[test]
fn fortran_bind_launders_foreign_compiler_failure_as_e3208() {
    let dir = isolated_cwd("fortran_bind_failure");
    let source = dir.join("broken.f90");
    fs::write(
        &source,
        r#"module broken_math
  use iso_c_binding
contains
  function broken(a) result(value) bind(C, name="broken")
    integer(c_int64_t), value :: a
    integer(c_int64_t) :: value
    value = a +
  end function broken
end module broken_math
"#,
    )
    .unwrap();

    let output = Command::new(jet())
        .args(["inspect", "bind", "fortran"])
        .arg(&source)
        .args(["--pkg", "broken"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Error [E3208]:"),
        "missing Jet diagnostic:\n{stderr}"
    );
    assert!(stderr.contains(" Why:"), "missing reason:\n{stderr}");
    assert!(stderr.contains(" Fix:"), "missing fix:\n{stderr}");
    assert!(
        !stderr.contains("broken.f90:"),
        "raw gfortran location leaked:\n{stderr}"
    );
    assert!(
        !stderr.contains("    7 |"),
        "raw gfortran source frame leaked:\n{stderr}"
    );
    check_snapshot(
        "bind_fortran_invalid_e3208.txt",
        &scrub(&stderr, &source),
    );
}

#[test]
fn cobol_bind_launders_foreign_compiler_failure_as_e3208() {
    if Command::new("cobc").arg("--version").output().is_err() { return; }
    let dir=isolated_cwd("cobol_bind_failure"); let source=dir.join("broken.cob"); let copybook=dir.join("record.cpy");
    fs::write(&source,"       IDENTIFICATION DIVISION.\n       PROGRAM-ID. BROKEN.\n       THIS IS NOT COBOL.\n").unwrap();
    fs::write(&copybook,"       01 RECORD.\n          05 AMOUNT PIC S9(7)V99 COMP-3.\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","cobol"]).arg(&source).args(["--copybook"]).arg(&copybook).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success()); let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:")); assert!(stderr.contains(" Why:")); assert!(stderr.contains(" Fix:"));
    assert!(!stderr.contains("broken.cob:"),"raw cobc location leaked:\n{stderr}");
    check_snapshot("bind_cobol_invalid_e3208.txt",&scrub(&stderr,&source));
}

#[test]
fn unknown_cross_target_is_e3302() {
    let src = std::env::temp_dir().join("jet_unknown_cross_target.jet");
    fs::write(&src, "fn run() { print(\"target\") }\n").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .arg(&src)
        .arg("--target=definitely-not-a-rust-target")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E3302]:"), "missing target diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E3302 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E3302 fix:\n{stderr}");
    check_snapshot("unknown_target_e3302.txt", &stderr);
}

#[test]
fn prove_unknown_lens_is_e2941() {
    let root = std::env::temp_dir().join("jet_cli_prove_unknown_lens");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("plain.jet"), "fn run() {}\n").unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "plain.jet", "--lens", "test"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E2941]:"), "missing lens diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E2941 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E2941 fix:\n{stderr}");
    check_snapshot("prove_unknown_lens_e2941.txt", &stderr);
}

#[test]
fn completions_generate_for_every_shell() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = Command::new(jet())
            .args(["self", "completions"])
            .arg(shell)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "completions {} should exit 0",
            shell
        );
        let s = String::from_utf8_lossy(&out.stdout);
        for flag in ["structural", "out", "report", "repo"] {
            let spelling = if shell == "fish" {
                format!("-l {flag}")
            } else {
                format!("--{flag}")
            };
            assert!(
                s.contains(&spelling),
                "{shell} completion missing {spelling}"
            );
        }
        check_snapshot(&format!("completions_{}.txt", shell), &s);
    }
}

#[test]
fn man_page_golden() {
    let out = Command::new(jet()).args(["self", "man"]).output().unwrap();
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Scrub the version so the snapshot is stable across releases.
    s = s.replace(env!("CARGO_PKG_VERSION"), "VERSION");
    for flag in ["--structural", "--out", "--report", "--repo"] {
        assert!(s.contains(flag), "man page missing {flag}");
    }
    check_snapshot("man.txt", &s);
}

#[test]
fn retired_emit_rust_flag_teaches_canonical_command() {
    let out = Command::new(jet())
        .args(["run", "examples/features/basics/hello.jet", "--emit-rust"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E2102]: `--emit-rust` isn't a flag"));
    assert!(stderr.contains("Fix: run `jet emit --rust <file.jet>`"));
}

#[test]
fn fix_dry_run_does_not_write() {
    // A file with an autofixable diagnostic. S14 teaching fixes are paused, so
    // use the still-live Core habit fix (`println` -> `print`).
    let p = std::env::temp_dir().join("jet_cli_fix.jet");
    let original = "fn run() {\n    println(\"hi\")\n}\n";
    fs::write(&p, original).unwrap();
    let out = Command::new(jet())
        .arg("fix")
        .arg(&p)
        .arg("--dry-run")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("dry run"), "dry-run should say so:\n{}", s);
    assert!(s.contains("print"), "diff should show the fix:\n{}", s);
    // The file on disk is unchanged.
    assert_eq!(
        fs::read_to_string(&p).unwrap(),
        original,
        "dry-run must not write"
    );

    // And a real fix DOES write.
    let out2 = Command::new(jet()).arg("fix").arg(&p).output().unwrap();
    assert_eq!(out2.status.code(), Some(0));
    assert!(
        fs::read_to_string(&p).unwrap().contains("print(\"hi\")"),
        "fix should rewrite the file"
    );
}

#[test]
fn external_subcommand_is_discovered() {
    // A fake `jet-greet` on a temp PATH should be invokable as `jet greet`.
    let dir = std::env::temp_dir().join("jet_ext_test_bin");
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("jet-greet");
    fs::write(&script, "#!/bin/sh\necho \"hi from plugin $1\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&script).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&script, perm).unwrap();
    }
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(jet())
        .arg("greet")
        .arg("world")
        .env("PATH", path)
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("hi from plugin world"),
        "external subcommand not forwarded:\n{}",
        s
    );
}

#[test]
fn osc8_hyperlinks_only_when_forced_on() {
    let p = bad_file(&line!().to_string());
    // Piped + NO_COLOR: never an OSC 8 link (existing snapshots stay clean).
    let piped = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(
        !s.contains("\x1b]8;;"),
        "piped output must have no OSC 8 links:\n{:?}",
        s
    );
    // The hyperlink layer is gated behind a real TTY; since tests run piped,
    // we exercise the renderer directly to prove the escape appears when asked.
    let src = "fn run() {}\n";
    let d = jet::Diagnostics::Diagnostic::error(
        "E0001",
        "x".into(),
        "y".into(),
        "z".into(),
        Some(jet::Diagnostics::Span::new(3, 7)),
    );
    let linked = d.render_linked("a.jet", src, true, true);
    assert!(
        linked.contains("\x1b]8;;"),
        "render_linked(hyperlinks=true) should emit OSC 8"
    );
    let plain = d.render_linked("a.jet", src, true, false);
    assert!(
        !plain.contains("\x1b]8;;"),
        "render_linked(hyperlinks=false) must not"
    );
}

#[test]
fn ext_optional_check_resolves_dot_jet() {
    // `jet check <path-without-.jet>` resolves to `<path>.jet` when the bare
    // path does not exist but the .jet file does.
    let stem = std::env::temp_dir().join("jet_cli_extopt_check");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"ok\");\n}\n").unwrap();
    let out = Command::new(jet())
        .arg("check")
        .arg(&stem)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "ext-optional check should resolve {}.jet and exit 0; stderr: {}",
        stem.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_rejected_collection_body_reports_only_frontend_diagnostics() {
    let dir = isolated_cwd("run_rejected_collection_body");
    fs::write(
        dir.join("main.jet"),
        r#"use core.files as fs

struct Row {
    name: String
    count: Int
}

fn run() {
    fs.write("/tmp/jet_1271.csv", "alpha,1\n") ?? panic("write failed")
    text :: fs.read("/tmp/jet_1271.csv") ?? ""
    rows := [Row].{}
    loop line, text.split("\n") {
        parts :: line.split(",")
        rows.push(Row.{ name: parts.get(0), count: missing })
    }
    rows.sort_by((row: Row) => row.name)
}
"#,
    )
    .unwrap();

    let rejected = Command::new(jet())
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert_eq!(rejected.status.code(), Some(1));
    assert!(rejected.stdout.is_empty(), "{:?}", rejected.stdout);
    let stderr = String::from_utf8(rejected.stderr).unwrap();
    assert_eq!(
        stderr,
        concat!(
            "Error [E0102]: `Iter` has no method `get`\n",
            "  --> main.jet:14:37\n",
            "    |\n",
            " 14 |         rows.push(Row.{ name: parts.get(0), count: missing })\n",
            "    |                                     ^^^\n",
            " Why: check the method name on this type\n",
            " Fix: call `.to_list()` first\n",
            "\n",
            "Error [E0107]: nothing named `missing` exists here\n",
            "  --> main.jet:14:52\n",
            "    |\n",
            " 14 |         rows.push(Row.{ name: parts.get(0), count: missing })\n",
            "    |                                                    ^^^^^^^\n",
            " Why: a name must be declared before it's used\n",
            " Fix: declare it first: `missing :: ...`\n",
            "\n",
            "2 problems found\n",
            "run `jet explain E0102` to learn more\n",
        )
    );
}

#[test]
fn check_fixed_dynamic_size_reports_e0103_without_internal_failure() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let invalid = root.join("tests/fuzz/sema/invalid/ui_fixed_dynamic_size.E0103.jet");
    let rejected = Command::new(jet())
        .args(["check", invalid.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("Error [E0103]"), "{stderr}");
    for leaked in [
        "panicked at",
        "entered unreachable code",
        "internal error",
        "generated Rust",
    ] {
        assert!(!stderr.contains(leaked), "`{leaked}` leaked:\n{stderr}");
    }

    let dir = isolated_cwd("check_fixed_comptime_size");
    fs::write(
        dir.join("mixed.jet"),
        "use core.mem\nfn fixed_size() => Int { return 32 }\nfn bad(size: Int) {\n fixed :: mem.Fixed.new(size: size)\n close(^fixed)\n}\nfn run() {\n fixed :: mem.Fixed.new(size: fixed_size())\n close(^fixed)\n}\n",
    )
    .unwrap();
    let mixed = Command::new(jet())
        .args(["check", "mixed.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(mixed.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&mixed.stderr);
    assert!(stderr.contains("Error [E0103]"), "{stderr}");
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("internal error"),
        "{stderr}"
    );

    fs::write(
        dir.join("compare_chain.jet"),
        "fn helper() => Int { return 1 }\nfn run() {\n $if 0 < helper() < 2 {\n  print(\"reachable\")\n }\n}\n",
    )
    .unwrap();
    let compare_chain = Command::new(jet())
        .args(["check", "compare_chain.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        compare_chain.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&compare_chain.stderr)
    );

    fs::write(
        dir.join("higher_order_valid.jet"),
        "use core.mem\nfn apply(f: fn() => Int) => Int { return f() }\nfn fixed_size() => Int { return 32 }\nfn run() {\n fixed :: mem.Fixed.new(size: apply(fixed_size))\n close(^fixed)\n}\n",
    )
    .unwrap();
    let higher_order_valid = Command::new(jet())
        .args(["check", "higher_order_valid.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        higher_order_valid.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&higher_order_valid.stderr)
    );

    fs::write(
        dir.join("higher_order_isolation.jet"),
        "use core.mem\nfn apply(f: fn() => Int) => Int { return f() }\nfn fixed_size() => Int { return 32 }\nfn bad(size: Int) {\n fixed :: mem.Fixed.new(size: size)\n close(^fixed)\n}\nfn run() {\n fixed :: mem.Fixed.new(size: apply(fixed_size))\n close(^fixed)\n}\n",
    )
    .unwrap();
    let higher_order_isolation = Command::new(jet())
        .args(["check", "higher_order_isolation.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(higher_order_isolation.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&higher_order_isolation.stderr);
    assert_eq!(stderr.matches("Error [E0103]").count(), 1, "{stderr}");
    for leaked in ["panicked at", "entered unreachable code", "internal error"] {
        assert!(!stderr.contains(leaked), "`{leaked}` leaked:\n{stderr}");
    }

    fs::write(
        dir.join("lambda_value.jet"),
        "fn run() {\n $callback :: () => print(\"not called\")\n print(\"ok\")\n}\n",
    )
    .unwrap();
    let lambda_value = Command::new(jet())
        .args(["check", "lambda_value.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        lambda_value.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&lambda_value.stderr)
    );

    fs::write(
        dir.join("helper.jet"),
        "use core.mem\nfn fixed_size() => Int { return 32 }\nfn run() {\n fixed :: mem.Fixed.new(size: fixed_size())\n close(^fixed)\n}\n",
    )
    .unwrap();
    let helper = Command::new(jet())
        .args(["check", "helper.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(helper.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&helper.stderr);
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("internal error"),
        "{stderr}"
    );

    fs::write(
        dir.join("main.jet"),
        "use core.mem\nfn run() {\n fixed :: mem.Fixed.new(size: 16 + 16)\n close(^fixed)\n}\n",
    )
    .unwrap();
    let accepted = Command::new(jet())
        .args(["check", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        accepted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let stderr = String::from_utf8_lossy(&accepted.stderr);
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("internal error"),
        "{stderr}"
    );
}

#[test]
fn check_reports_soft_public_lints_without_failing() {
    let dir = isolated_cwd("check_soft_public");
    fs::write(
        dir.join("library.jet"),
        "pub fn _legacy() => Int { return 1 }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use \"library\"\nfn run() { print(library._legacy()) }\n",
    )
    .unwrap();
    let output = Command::new(jet())
        .args(["check", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("[L0601]").count(), 1, "{stderr}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("has no problems"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn ext_optional_run_resolves_dot_jet() {
    // Same resolution for `jet run`.
    let stem = std::env::temp_dir().join("jet_cli_extopt_run");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"hello-extopt\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("run").arg(&stem).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "ext-optional run should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello-extopt"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn ext_optional_missing_path_keeps_original_name() {
    // Neither `<path>` nor `<path>.jet` exists: the original name must surface
    // in the file-not-found error (resolution returns it unchanged).
    let stem = std::env::temp_dir().join("jet_cli_extopt_absent_xyz");
    let out = Command::new(jet())
        .arg("check")
        .arg(&stem)
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("jet_cli_extopt_absent_xyz"),
        "error should name the original path; stderr: {err}"
    );
}

#[test]
fn simple_exec_runs_without_a_manifest() {
    // A single file with a top-level `fn run` and no package.jet runs as an
    // executable with zero ceremony (R9 / D-ILE1).
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_exec/main.jet");
    // Isolated cwd: this fixture's stem is `main`, a common stem other tests
    // and examples also use — see `isolated_cwd`.
    let out = Command::new(jet())
        .arg("run")
        .arg(&path)
        .current_dir(isolated_cwd("simple_exec"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("simple exec, no manifest"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn passthrough_forwards_tokens_after_separator() {
    // `jet run file.jet -- --port 8080 x` — program sees 4 args: argv[0] +
    // three forwarded tokens. io.args().len() == 4.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", "--release", p.to_str().unwrap(), "--", "--port", "8080", "x"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "4",
        "expected 4 args (argv[0] + 3 forwarded), got: {stdout}"
    );
}

#[test]
fn bare_separator_gives_empty_passthrough() {
    // `jet run file.jet --` — bare `--` with nothing after; program sees 1 arg.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", "--release", p.to_str().unwrap(), "--"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "1",
        "expected 1 arg (just argv[0]), got: {stdout}"
    );
}
