use std::fs;
use std::process::Command;

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

#[test]
fn cpp_clang_binder_compiles_links_and_runs_owned_surface() {
    if !have("clang++") || !have("ar") || !have("rustc") {
        return;
    }
    let root = std::env::temp_dir().join(format!("jet_cpp_systems_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".jet/bindings/cpp")).unwrap();
    let header = root.join("counter.hpp");
    let header_source = r#"#include <cstdint>

class Counter {
public:
    explicit Counter(int64_t start) : value(start) {}
    int64_t add(int64_t amount) { value += amount; return value; }
    int64_t add(double factor) { value += static_cast<int64_t>(factor); return value; }
    int64_t operator+(int64_t amount) { return value + amount; }
    int64_t fail_if_negative(int64_t value) { if (value < 0) throw 1; return value; }
private:
    int64_t value;
};

inline int64_t apply(int32_t (*callback)(int32_t), int64_t value) { return callback(static_cast<int32_t>(value)); }
template <typename T> T twice(T value) { return value + value; }
template int64_t twice<int64_t>(int64_t value);
"#;
    fs::write(&header, header_source).unwrap();
    let cache = root.join(".jet/bindings/cpp");
    let result = jet::CppBind::bind(&header, header_source, "counter", &cache).unwrap();
    fs::write(cache.join("counter.jet"), &result.source).unwrap();
    fs::write(cache.join("counter.provenance"), &result.provenance).unwrap();
    assert!(result.archive.is_file());
    assert!(
        result.provenance.contains("schema=jet-cpp-bind-v1")
            && result.provenance.contains("clang=")
    );
    let main = root.join("main.jet");
    let source = r#"use cpp.counter as cpp
use c.jet_cpp_counter as raw

fn increment(value: I32) --[]-> I32 { return value + 1 }

fn run() {
    counter := cpp.new_counter(10) ?? panic("constructor")
    print(counter.add_amount(5) ?? panic("method"))
    print(counter.add_factor(2.0) ?? panic("overload"))
    print(counter.add(3) ?? panic("operator"))
    print(raw.apply(increment, 41))
    print(cpp.twice_int(21) ?? panic("template"))
    print(counter.fail_if_negative(-1) ?? -1)
    cpp.close_counter(^counter)
}
"#;
    fs::write(&main, source).unwrap();
    let output = jet::compile_with_path(source, main.to_str().unwrap()).unwrap_or_else(|diags| {
        panic!(
            "{}",
            jet::render_diagnostics(main.to_str().unwrap(), source, &diags)
        )
    });
    let rust = root.join("main.rs");
    let binary = root.join("main_bin");
    fs::write(&rust, output.rust).unwrap();
    let mut command = Command::new("rustc");
    command
        .args(["--edition", "2021"])
        .arg(&rust)
        .arg("-o")
        .arg(&binary);
    for arg in output
        .clinks
        .into_iter()
        .chain(jet::resolve_c_links(main.to_str().unwrap()).unwrap())
    {
        command.arg(arg);
    }
    let built = command.output().unwrap();
    assert!(
        built.status.success(),
        "I2: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(&binary).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "15\n17\n20\n42\n42\n-1\n"
    );
    let _ = fs::remove_dir_all(root);
}
