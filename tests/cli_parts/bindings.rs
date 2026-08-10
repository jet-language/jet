use super::*;

/// A query that looks like a diagnostic code renders the verbatim I4 essay —
/// byte-identical to `jet explain <CODE>`, since both go through
/// `jet::Explain::render` over the same registry (single source of truth).
#[test]
fn question_mark_code_query_matches_explain_verbatim() {
    let via_help = Command::new(jet())
        .args(["?", "E0102"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let via_explain = Command::new(jet())
        .args(["explain", "E0102"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(via_help.status.code(), Some(0));
    assert_eq!(via_explain.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&via_help.stdout),
        String::from_utf8_lossy(&via_explain.stdout),
        "`jet ? E0102` must render the same verbatim essay as `jet explain E0102` (I4)"
    );
}

/// A multi-word task/outcome phrase still resolves to a real command line —
/// the owner-modified default (2026-07-08): keywords are aliases on command
/// entries, never a separate goal menu, but they must still be findable.
#[test]
fn question_mark_task_phrase_resolves_to_a_real_command() {
    let out = Command::new(jet())
        .args(["?", "add", "a", "dependency"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("jet add"), "expected `add` to surface, got:\n{}", stdout);
}

#[test]
fn question_mark_observe_slow_program_routes_to_observation_guide() {
    let out = Command::new(jet())
        .args(["?", "why", "is", "my", "program", "slow"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    for surface in ["Live scheduler", "GC promotions", "Wall-clock session", "Browser rows"] {
        assert!(stdout.contains(surface), "observation guide missing {surface}: {stdout}");
    }
    check_snapshot("observability_guide.txt", &stdout);
}

#[test]
fn file_sugar_runs_without_run_subcommand() {
    let stem = std::env::temp_dir().join("jet_cli_file_sugar");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"file-sugar\");\n}\n").unwrap();
    let out = Command::new(jet()).arg(&file).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "jet <file> sugar should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("file-sugar"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn file_sugar_ext_optional() {
    let stem = std::env::temp_dir().join("jet_cli_file_sugar_extopt");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"ext-sugar\");\n}\n").unwrap();
    let out = Command::new(jet()).arg(&stem).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "jet <stem> sugar should resolve .jet; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ext-sugar"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn file_sugar_missing_jet_file_errors() {
    let missing = std::env::temp_dir().join("jet_cli_file_sugar_absent.jet");
    let _ = fs::remove_file(&missing);
    let out = Command::new(jet())
        .arg(&missing)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        combined.contains("jet_cli_file_sugar_absent"),
        "missing file should be named in output: {combined}"
    );
}

#[test]
fn did_you_mean_golden() {
    let out = Command::new(jet())
        .arg("buld")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("did_you_mean.txt", &stderr);
}

#[test]
fn unknown_flag_is_e2102() {
    let p = std::env::temp_dir().join("jet_cli_ok2.jet");
    fs::write(&p, "fn run() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--jsn")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "unknown flag should exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2102"), "should cite E2102:\n{}", stderr);
    assert!(
        stderr.contains("--json"),
        "should suggest --json:\n{}",
        stderr
    );
}

#[test]
fn doctor_ok_golden() {
    // On a CI/dev box rustc is present; the report is deterministic except for
    // machine-specific paths and the rustc version, which we scrub.
    let out = Command::new(jet())
        .args(["self", "doctor"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Doctor must never emit ANSI when piped.
    assert!(
        !s.contains('\x1b'),
        "doctor output must be ANSI-free when piped"
    );
    // Structural assertions (a full golden would be machine-specific).
    assert!(s.contains("doctor"), "missing header:\n{}", s);
    assert!(s.contains("rustc"), "missing rustc check:\n{}", s);
    assert!(s.contains("pkg-config"), "missing C-FFI section:\n{}", s);
    assert!(s.contains("hangar"), "missing hangar check:\n{}", s);
}

#[test]
fn doctor_failure_is_l2101_snapshot() {
    let out = Command::new(jet())
        .args(["self", "doctor"])
        .env("PATH", "")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let start = stdout.find("Warning [L2101]:").expect("L2101 diagnostic");
    check_snapshot("doctor_l2101.txt", &stdout[start..]);
}

#[test]
fn fetch_without_git_is_e1203_snapshot() {
    let dir = isolated_cwd("fetch_no_git");
    fs::write(
        dir.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\njet: \">=0.1.0\"\ndescription: \"\"\nlicense: \"MIT\"\npackages: { app: executable }\ndeps: { tool: { git: \"https://example.invalid/tool.git\", tag: \"v1\" } }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .args(["fetch"])
        .current_dir(&dir)
        .env("PATH", "")
        .env("HOME", &dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    let start = stderr.find("Error [E1203]:").expect("E1203 diagnostic");
    check_snapshot("fetch_no_git_e1203.txt", &stderr[start..]);
}

#[test]
fn bind_missing_header_is_e3208() {
    let missing = std::env::temp_dir().join("jet_missing_bind_header.h");
    let _ = fs::remove_file(&missing);
    let out = Command::new(jet())
        .args(["inspect", "bind"])
        .arg(&missing)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E3208]:"), "missing bind diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E3208 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E3208 fix:\n{stderr}");
    check_snapshot("bind_missing_e3208.txt", &scrub(&stderr, &missing));
}

#[test]
fn fortran_bind_compiles_and_runs_iso_c_binding_scalar() {
    let dir = isolated_cwd("fortran_bind_scalar");
    let source = dir.join("scalar.f90");
    fs::write(
        &source,
        r#"module scalar_math
  use iso_c_binding
contains
  function add_i64(a, b) result(value) bind(C, name="add_i64")
    integer(c_int64_t), value :: a
    integer(c_int64_t), value :: b
    integer(c_int64_t) :: value
    value = a + b
  end function add_i64
end module scalar_math
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "fortran"])
        .arg(&source)
        .args(["--pkg", "scalar"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Fortran bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    assert!(dir.join(".jet/bindings/fortran/scalar.jet").is_file());
    assert!(dir.join(".jet/bindings/fortran/libjet_fortran_scalar.a").is_file());

    fs::write(
        dir.join("main.jet"),
        "use fortran.scalar as scalar\n\nfn run() { print(scalar.add_i64(20, 22)) }\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Fortran binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn fortran_bind_runs_checked_column_major_array() {
    let dir = isolated_cwd("fortran_bind_array");
    let source = dir.join("matrix.f90");
    fs::write(
        &source,
        r#"module matrix_math
  use iso_c_binding
contains
  function probe(a) result(value) bind(C, name="probe_column_major")
    real(c_double), intent(in) :: a(2,3)
    real(c_double) :: value
    value = 100.0_c_double * a(1,2) + a(2,1)
  end function probe
end module matrix_math
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "fortran"])
        .arg(&source)
        .args(["--pkg", "matrix"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Fortran array bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    let generated = fs::read_to_string(dir.join(".jet/bindings/fortran/matrix.jet")).unwrap();
    assert!(generated.contains("fortran-layout probe.a: column-major 2x3"));
    assert!(generated.contains("a.len() != 6"));
    assert!(generated.contains("=[Fortran]=>"));
    assert!(String::from_utf8_lossy(&bind.stdout).contains("layout: probe.a column-major 2x3"));

    fs::write(
        dir.join("main.jet"),
        "use fortran.matrix as matrix\n\nfn run() { print(matrix.probe([1.0, 2.0, 3.0, 4.0, 5.0, 6.0])) }\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Fortran array binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    // Fortran sees the flat input in column-major order: a(1,2)=3, a(2,1)=2.
    assert_eq!(String::from_utf8_lossy(&run.stdout), "302.0\n");

    fs::write(
        dir.join("bad.jet"),
        "use fortran.matrix as matrix\n\nfn run() { print(matrix.probe([1.0, 2.0, 3.0, 4.0, 5.0])) }\n",
    )
    .unwrap();
    let bad = Command::new(jet())
        .args(["run", "--release", "bad.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr)
            .contains("a must contain exactly 6 column-major values"),
        "missing checked array length failure:\n{}",
        String::from_utf8_lossy(&bad.stderr)
    );
}

#[test]
fn go_bind_compiles_and_runs_c_archive_scalar() {
    let dir = isolated_cwd("go_bind_scalar");
    let source = dir.join("scalar.go");
    fs::write(
        &source,
        r#"package main

/*
#include <stdint.h>
*/
import "C"

//export add_i64
func add_i64(a int64, b int64) int64 {
    return a + b
}

func main() {}
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "scalar"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Go bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    assert!(dir.join(".jet/bindings/go/scalar.jet").is_file());
    assert!(dir.join(".jet/bindings/go/libjet_go_scalar.a").is_file());

    fs::write(
        dir.join("main.jet"),
        "use go.scalar as scalar\n\nfn run() { print(scalar.add_i64(20, 22)) }\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Go binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn go_bind_compiles_and_runs_move_only_cgo_handle() {
    let dir = isolated_cwd("go_bind_handle");
    let source = dir.join("handles.go");
    fs::write(
        &source,
        r#"package main

/*
#include <stdint.h>
*/
import "C"
import "runtime/cgo"

//export new_handle
func new_handle(value int64) uintptr {
    return uintptr(cgo.NewHandle(value))
}

//export consume_handle
func consume_handle(handle uintptr) int64 {
    owned := cgo.Handle(handle)
    value := owned.Value().(int64)
    owned.Delete()
    return value
}

func main() {}
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "handles"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Go handle bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    let generated = fs::read_to_string(dir.join(".jet/bindings/go/handles.jet")).unwrap();
    assert!(generated.contains("pub struct Handle { value: Int }"));
    assert!(generated.contains("pub fn new_handle(value: Int) => Handle"));
    assert!(generated.contains("pub fn consume_handle(handle: Handle) => Int"));

    fs::write(
        dir.join("main.jet"),
        "use go.handles as handles\n\nfn run() =[Go, IO]=> {\n    handle :: handles.new_handle(42)\n    print(handles.consume_handle(handle))\n}\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Go handle binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn go_bind_launders_foreign_compiler_failure_as_e3208() {
    let dir = isolated_cwd("go_bind_failure");
    let source = dir.join("broken.go");
    fs::write(
        &source,
        r#"package main

import "C"

//export broken
func broken(a int64) int64 {
    return a +
}

func main() {}
"#,
    )
    .unwrap();

    let output = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "broken"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:"), "missing Jet diagnostic:\n{stderr}");
    assert!(stderr.contains(" Why:"), "missing reason:\n{stderr}");
    assert!(stderr.contains(" Fix:"), "missing fix:\n{stderr}");
    assert!(!stderr.contains("broken.go:"), "raw Go location leaked:\n{stderr}");
    check_snapshot("bind_go_invalid_e3208.txt", &scrub(&stderr, &source));
}

#[test]
fn java_bind_embeds_jvm_handles_methods_and_exceptions() {
    let dir = isolated_cwd("java_bind_embedded");
    let source = dir.join("Counter.java");
    fs::write(&source, r#"public class Counter {
    private long value;
    public Counter(long value) { this.value = value; }
    public long add(long amount) { value += amount; return value; }
    public long explode(long code) { if (code < 0) throw new IllegalStateException("hidden foreign detail"); return code; }
    public static double twice(double value) { return value * 2.0; }
}
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","java"]).arg(&source).args(["--pkg","counter"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Java bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/java/libjet_java_counter.a").is_file());
    assert!(dir.join(".jet/bindings/java/counter.classes/Counter.class").is_file());
    assert!(dir.join(".jet/bindings/java/counter.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use java.counter as counter

fn run() =[Java, IO]=> {
    handle :: counter.new(40) ?? panic("JVM create failed")
    print(counter.add(handle, 2) ?? -1)
    print(counter.twice(2.5) ?? -1.0)
    print(counter.explode(handle, -1) ?? -7)
    counter.close(^handle)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"embedded JVM binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n5.0\n-7\n");
    assert!(!String::from_utf8_lossy(&run.stderr).contains("hidden foreign detail"));
}

#[test]
fn java_bind_launders_javac_failure_as_e3208() {
    let dir=isolated_cwd("java_bind_failure"); let source=dir.join("Broken.java");
    fs::write(&source,"public class Broken { public Broken(long n) { this. = n; } public long value() { return 1; } }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","java"]).arg(&source).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success()); let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:")); assert!(stderr.contains(" Why:")); assert!(stderr.contains(" Fix:"));
    assert!(!stderr.contains("Broken.java:"),"raw javac location leaked:\n{stderr}");
    check_snapshot("bind_java_invalid_e3208.txt", &scrub(&stderr, &source));
}

#[test]
fn dotnet_bind_embeds_coreclr_state_calls_and_errors(){
    let dir=isolated_cwd("dotnet_bind_embedded");let source=dir.join("Counter.cs");fs::write(&source,r#"public class Counter {
    private long value;
    public Counter(long value) { this.value = value; }
    public long add(long amount) { value += amount; return value; }
    public long explode(long code) { if (code < 0) throw new System.InvalidOperationException("hidden managed detail"); return code; }
    public static double twice(double value) { return value * 2.0; }
}
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","cs"]).arg(&source).args(["--pkg","counter"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),".NET bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/cs/libjet_cs_counter.a").is_file());assert!(dir.join(".jet/bindings/cs/counter.dotnet/JetBinding.dll").is_file());assert!(dir.join(".jet/bindings/cs/counter.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use cs.counter as counter

fn run() =[DotNet, IO]=> {
    handle :: counter.new(40) ?? panic("CoreCLR create failed")
    print(counter.add(handle, 2) ?? -1)
    print(counter.twice(2.5) ?? -1.0)
    print(counter.explode(handle, -1) ?? -7)
    counter.close(^handle)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"embedded CoreCLR binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n5.0\n-7\n");assert!(!String::from_utf8_lossy(&run.stderr).contains("hidden managed detail"));
}

#[test]
fn dotnet_bind_launders_compiler_failure_as_e3208(){let dir=isolated_cwd("dotnet_bind_failure");let source=dir.join("Broken.cs");fs::write(&source,"public class Broken { public Broken(long n) { this. = n; } public long value() => 1; }\n").unwrap();let output=Command::new(jet()).args(["inspect","bind","cs"]).arg(&source).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));assert!(!stderr.contains("Broken.cs:"),"raw C# source frame leaked:\n{stderr}");check_snapshot("bind_dotnet_invalid_e3208.txt",&scrub(&stderr,&source));}

#[test]
fn tcl_bind_runs_one_shot_and_persistent_typed_sessions() {
    let dir=isolated_cwd("tcl_bind_session");let source=dir.join("eda.tcl");
    fs::write(&source,"set counter 40\n").unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","tcl"]).arg(&source).args(["--pkg","eda"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Tcl bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/tcl/libjet_tcl_eda.a").is_file());
    assert!(dir.join(".jet/bindings/tcl/eda.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use tcl.eda as tcl

fn run() =[Tcl, IO]=> {
    session :: tcl.open() ?? panic("Tcl open failed")
    print(tcl.eval_int(session, "incr counter 2") ?? -1)
    print(tcl.eval_int(session, "incr counter 1") ?? -1)
    print(tcl.eval_once("expr 6 * 7") ?? "bad")
    print(tcl.eval_float(session, "expr 5.0 / 2") ?? -1.0)
    print(tcl.eval(session, "error \"foreign stack secret\"") ?? "tcl-error")
    tcl.close(^session)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"embedded Tcl binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n43\n42\n2.5\ntcl-error\n");
    assert!(!String::from_utf8_lossy(&run.stderr).contains("foreign stack secret"));
}

#[test]
fn ada_bind_compiles_runs_and_rejects_range_before_call() {
    let dir=isolated_cwd("ada_bind_range");let spec=dir.join("geodesy.ads");let body=dir.join("geodesy.adb");
    fs::write(&spec,r#"with Interfaces.C;
use type Interfaces.C.double;
package Geodesy is
   subtype Latitude is Interfaces.C.double range -90.0 .. 90.0;
   function Double_Lat (Lat : Latitude) return Interfaces.C.double
     with Export, Convention => C, External_Name => "geo_double";
   function Calls (Unused : Interfaces.C.long_long) return Interfaces.C.long_long
     with Export, Convention => C, External_Name => "geo_calls";
end Geodesy;
"#).unwrap();
    fs::write(&body,r#"with Interfaces.C;
use type Interfaces.C.double;
use type Interfaces.C.long_long;
package body Geodesy is
   Calls_Count := Interfaces.C.long_long.{ 0 };
   function Double_Lat (Lat : Latitude) return Interfaces.C.double is
   begin
      Calls_Count := Calls_Count + 1;
      return Lat * 2.0;
   end Double_Lat;
   function Calls (Unused : Interfaces.C.long_long) return Interfaces.C.long_long is
   begin
      return Calls_Count + Unused;
   end Calls;
end Geodesy;
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","ada"]).arg(&spec).args(["--pkg","geodesy"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Ada bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/ada/libjet_ada_geodesy.a").is_file());
    assert!(dir.join(".jet/bindings/ada/geodesy.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use ada.geodesy as geo

fn run() =[Ada, IO]=> {
    print(geo.double_lat(95.0) ?? -1.0)
    print(geo.calls(0) ?? -1)
    print(geo.double_lat(21.0) ?? -1.0)
    print(geo.calls(0) ?? -1)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"generated Ada binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout),"-1.0\n0\n42.0\n1\n");
}

#[test]
fn pascal_bind_runs_scalar_and_owned_class_lifecycle() {
    let dir=isolated_cwd("pascal_bind_lifecycle");let source=dir.join("inventory.pas");
    fs::write(&source,r#"library inventory;
type
  TCounter = class
  private
    FValue: Int64;
  public
    constructor Create(Value: Int64);
    function Add(Delta: Int64): Int64;
    destructor Destroy; override;
  end;
var Destroyed: Int64 = 0;
constructor TCounter.Create(Value: Int64);
begin inherited Create; FValue := Value; end;
function TCounter.Add(Delta: Int64): Int64;
begin FValue := FValue + Delta; Result := FValue; end;
destructor TCounter.Destroy;
begin Destroyed := Destroyed + 1; inherited Destroy; end;
function add_scalar(A, B: Int64): Int64; cdecl;
begin Result := A + B; end;
function counter_new(Value: Int64): Pointer; cdecl;
begin Result := Pointer(TCounter.Create(Value)); end;
function counter_add(Handle: Pointer; Delta: Int64): Int64; cdecl;
begin Result := TCounter(Handle).Add(Delta); end;
procedure counter_free(Handle: Pointer); cdecl;
begin TCounter(Handle).Free; end;
function destroyed_count(): Int64; cdecl;
begin Result := Destroyed; end;
exports add_scalar, counter_new, counter_add, counter_free, destroyed_count;
begin
end.
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","pascal"]).arg(&source).args(["--pkg","inventory"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Pascal bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    let cache=dir.join(".jet/bindings/pascal");assert!(cache.join("libjet_pascal_inventory.a").is_file());assert!(cache.join("libjet_pascal_inventory_runtime.so").is_file());assert!(cache.join("inventory.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use pascal.inventory as inv

fn run() =[Pascal, IO]=> {
    print(inv.add_scalar(20, 22))
    handle :: inv.counter_new(40) ?? panic("Pascal constructor failed")
    print(inv.counter_add(handle, 2) ?? -1)
    print(inv.destroyed_count())
    inv.close(^handle)
    print(inv.destroyed_count())
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"generated Pascal binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n42\n0\n1\n");
    fs::write(dir.join("stale.c"),r#"#include <stdint.h>
extern int64_t jet_pascal_inventory_counter_new(int64_t);
extern void jet_pascal_inventory_counter_close(int64_t);
extern int64_t jet_pascal_inventory_take_error(void);
extern int64_t jet_pascal_inventory_destroyed_count(void);
int main(void){int64_t h=jet_pascal_inventory_counter_new(1);if(!h)return 1;jet_pascal_inventory_counter_close(h);if(jet_pascal_inventory_destroyed_count()!=1)return 2;jet_pascal_inventory_counter_close(h);if(jet_pascal_inventory_take_error()!=1)return 3;if(jet_pascal_inventory_destroyed_count()!=1)return 4;return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("stale.c").args(["-L.jet/bindings/pascal","-Wl,-rpath,.jet/bindings/pascal","-l:libjet_pascal_inventory.a","-ljet_pascal_inventory_runtime","-lpthread","-ldl","-o","stale"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"stale-handle probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let stale=Command::new(dir.join("stale")).current_dir(&dir).output().unwrap();assert!(stale.status.success(),"stale close reached Pascal destructor twice: {:?}",stale.status.code());
}

#[test]
fn pascal_bind_launders_fpc_failure_as_e3208() {
    let dir=isolated_cwd("pascal_bind_failure");let source=dir.join("broken.pas");
    fs::write(&source,"library broken; type TCounter = class end; function counter_new(Value: Int64): Pointer; cdecl; begin Result := ; end; function counter_add(Handle: Pointer; Delta: Int64): Int64; cdecl; begin Result := 0; end; procedure counter_free(Handle: Pointer); cdecl; begin end; exports counter_new, counter_add, counter_free; begin end.\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","pascal"]).arg(&source).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));assert!(!stderr.contains("broken.pas("));check_snapshot("bind_pascal_invalid_e3208.txt",&scrub(&stderr,&source));
}

#[test]
fn dart_bind_runs_jet_compute_and_dart_callback_in_process() {
    if Command::new("dart").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("dart_bind_round_trip");let contract=dir.join("callbacks.dart");let compute=dir.join("compute.jet");
    fs::write(&contract,"@pragma('vm:entry-point')\nint dartDouble(int value) => value * 2;\n").unwrap();
    fs::write(&compute,"use dart.callbacks as callbacks\n\npub fn compute(value: Int) =[Dart]=> Int {\n    return callbacks.dart_double(value) ?? -1\n}\n").unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","dart"]).arg(&contract).args(["--jet",compute.to_str().unwrap(),"--pkg","callbacks"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Dart bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    let cache=dir.join(".jet/bindings/dart");let native=cache.join(if cfg!(target_os="macos"){"libjet_dart_callbacks_compute.dylib"}else if cfg!(target_os="windows"){"libjet_dart_callbacks_compute.dll"}else{"libjet_dart_callbacks_compute.so"});
    assert!(cache.join("libjet_dart_callbacks.a").is_file());assert!(native.is_file());assert!(cache.join("callbacks_host.dart").is_file());assert!(cache.join("callbacks.provenance").is_file());
    let native_path=native.to_string_lossy().replace('\\',"\\\\").replace('\'',"\\'");
    fs::write(dir.join("host.dart"),format!("import 'dart:ffi';\nimport '.jet/bindings/dart/callbacks_host.dart';\ntypedef ComputeNative = Int64 Function(Int64);\ntypedef ComputeDart = int Function(int);\nvoid main() {{ initializeJetDart('{native_path}'); final compute = jetDartLibrary.lookupFunction<ComputeNative, ComputeDart>('compute'); print(compute(21)); shutdownJetDart(); }}\n")).unwrap();
    let run=Command::new("dart").args(["run","host.dart"]).current_dir(&dir).output().unwrap();assert!(run.status.success(),"Dart host failed:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n");
}

#[test]
fn dart_bind_rejects_untyped_contract_as_e3208() {
    let dir=isolated_cwd("dart_bind_invalid");let contract=dir.join("broken.dart");fs::write(&contract,"@pragma('vm:entry-point')\nString greet(String value) => value;\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","dart"]).arg(&contract).args(["--jet","compute.jet","--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));check_snapshot("bind_dart_invalid_e3208.txt",&scrub(&stderr,&contract));
}

#[test]
fn powershell_bind_round_trips_datatree_state_and_cleans_workers() {
    if Command::new("pwsh").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("powershell_bind_round_trip");let script=dir.join("ops.ps1");
    fs::write(&script,r#"$script:Counter = 0
function Get-Stateful {
  param($InputObject)
  $script:Counter += 1
  [ordered]@{
    count = $script:Counter
    nested = $InputObject.nested
    list = @($InputObject.list)
    scalar = $InputObject.scalar
    nothing = $null
  }
}
function Fail { param($InputObject) throw 'raw secret failure detail' }
function Sleep { param($InputObject) Start-Sleep -Seconds 30; return $InputObject }
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","pwsh"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"PowerShell bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/pwsh");assert!(cache.join("libjet_pwsh_ops.a").is_file());assert!(cache.join("ops_worker.ps1").is_file());assert!(cache.join("ops.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use pwsh.ops as ops
use core.encoding.json as json

fn run() =[PowerShell, IO]=> {
    session :: ops.open() ?? panic("PowerShell open failed")
    input :: DataTree.Object(["nested": DataTree.Object(["ok": DataTree.Bool(true)]), "list": DataTree.Array([DataTree.Int(1), DataTree.Text("two")]), "scalar": DataTree.Float(3.5), "nothing": DataTree.Null])
    first :: ops.get_stateful(session, ~input, 5000) ?? panic("first call failed")
    second :: ops.get_stateful(session, ~input, 5000) ?? panic("second call failed")
    print(json.canonical(first) ?? panic("value is not canonical JSON"))
    print(json.canonical(second) ?? panic("value is not canonical JSON"))
    failed :: ops.fail(session, DataTree.Null, 5000) ?? DataTree.Text("failed")
    print(json.canonical(failed) ?? panic("value is not canonical JSON"))
    timed :: ops.sleep(session, DataTree.Int(1), 100) ?? DataTree.Text("timeout")
    print(json.canonical(timed) ?? panic("value is not canonical JSON"))
    ops.close(^session)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated PowerShell binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),"{\"count\":1,\"list\":[1,\"two\"],\"nested\":{\"ok\":true},\"nothing\":null,\"scalar\":3.5}\n{\"count\":2,\"list\":[1,\"two\"],\"nested\":{\"ok\":true},\"nothing\":null,\"scalar\":3.5}\n\"failed\"\n\"timeout\"\n");
    fs::write(dir.join("cancel.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <unistd.h>
extern int64_t jet_pwsh_ops_open(void);
extern const char* jet_pwsh_ops_invoke_sleep(int64_t,const char*,int64_t);
extern void jet_pwsh_ops_cancel(int64_t);
extern void jet_pwsh_ops_close(int64_t);
extern int64_t jet_pwsh_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_pwsh_ops_invoke_sleep(handle,"null",60000);code=jet_pwsh_ops_take_error();return 0;}
int main(void){handle=jet_pwsh_ops_open();if(!handle)return 1;pthread_t thread;if(pthread_create(&thread,0,call,0))return 2;usleep(100000);jet_pwsh_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 3;int64_t fresh=jet_pwsh_ops_open();if(!fresh)return 4;jet_pwsh_ops_close(fresh);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("cancel.c").args(["-L.jet/bindings/pwsh","-l:libjet_pwsh_ops.a","-lpthread","-o","cancel"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"PowerShell cancellation probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let cancel=Command::new(dir.join("cancel")).current_dir(&dir).output().unwrap();assert!(cancel.status.success(),"PowerShell cancellation did not clean the worker: {:?}",cancel.status.code());
}

#[test]
fn powershell_bind_launders_parse_failure_as_e3208() {
    if Command::new("pwsh").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("powershell_bind_invalid");let script=dir.join("broken.ps1");fs::write(&script,"function Broken { param($InputObject) if ( }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","pwsh"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("Unexpected token"));assert!(!stderr.contains("broken.ps1:"));check_snapshot("bind_powershell_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[test]
fn perl_bind_round_trips_datatree_state_timeout_and_cancellation() {
    if Command::new("perl").arg("-v").output().is_err(){return}
    let dir=isolated_cwd("perl_bind_round_trip");let script=dir.join("ops.pl");
    let example=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/perl");fs::copy(example.join("ops.pl"),&script).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","perl"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"Perl bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/perl");assert!(cache.join("libjet_perl_ops.a").is_file());assert!(cache.join("ops_worker.pl").is_file());assert!(cache.join("ops.provenance").is_file());
    fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated Perl binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::write(dir.join("cancel.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <unistd.h>
extern int64_t jet_perl_ops_open(void);
extern const char* jet_perl_ops_invoke_sleep(int64_t,const char*,int64_t);
extern void jet_perl_ops_cancel(int64_t);
extern void jet_perl_ops_close(int64_t);
extern int64_t jet_perl_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_perl_ops_invoke_sleep(handle,"null",60000);code=jet_perl_ops_take_error();return 0;}
int main(void){handle=jet_perl_ops_open();if(!handle)return 1;pthread_t thread;if(pthread_create(&thread,0,call,0))return 2;usleep(100000);jet_perl_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 3;int64_t fresh=jet_perl_ops_open();if(!fresh)return 4;jet_perl_ops_close(fresh);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("cancel.c").args(["-L.jet/bindings/perl","-l:libjet_perl_ops.a","-lpthread","-o","cancel"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"Perl cancellation probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let cancel=Command::new(dir.join("cancel")).current_dir(&dir).output().unwrap();assert!(cancel.status.success(),"Perl cancellation did not clean the worker: {:?}",cancel.status.code());
}
