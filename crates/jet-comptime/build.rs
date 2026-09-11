use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    write_web_kernel_std(&manifest);
    write_native_scheduler(&manifest);
    write_clock_rt(&manifest);
    write_pool_rt(&manifest);
}

fn write_pool_rt(manifest: &Path) {
    let source = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs");
    println!("cargo:rerun-if-changed={}", source.display());
    let pool = extract_pool(&std::fs::read_to_string(source).expect("read canonical Pool kernel"));
    let mut body = String::from(
        "pub mod jet_std {\n\
         use jet_foundation::Outcome::{JetAbsent, JetOutcome};\n",
    );
    body.push_str(&pool);
    body.push_str("\n}\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("pool_rt.rs");
    std::fs::write(out, body).expect("write pool_rt.rs");
}
fn write_clock_rt(manifest: &PathBuf) {
    let types = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs");
    let operations = manifest.join("../jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs");
    println!("cargo:rerun-if-changed={}", types.display());
    println!("cargo:rerun-if-changed={}", operations.display());
    let types = std::fs::read_to_string(types).expect("read canonical Clock types");
    let start = types.find("#[derive(Debug)]\npub enum ClockState {").expect("ClockState");
    let end = types.find("#[derive(Clone, Debug, PartialEq)]\npub struct Rng {").expect("Rng");
    let mut body = String::from("pub mod jet_std {\n");
    body.push_str(&types[start..end]);
    body.push_str("\n}\n");
    let operations = std::fs::read_to_string(operations).expect("read canonical Clock operations");
    let start = operations.find("fn jet_std_clock_new(").expect("Clock.new");
    let end = operations.find("fn jet_clock_wait(").expect("Clock.wait");
    body.push_str(&operations[start..end].replace("fn jet_", "pub fn jet_"));
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("clock_rt.rs");
    std::fs::write(out, body).expect("write clock_rt.rs");
}

fn write_web_kernel_std(manifest: &Path) {
    let reactive_src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs");
    let encoding_types_src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/EncodingTypes.rs");
    let json_codec_src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/JSONCodec.rs");
    let math_task_src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs");
    for source in [&reactive_src, &encoding_types_src, &json_codec_src, &math_task_src] {
        println!("cargo:rerun-if-changed={}", source.display());
    }

    let reactive =
        extract_reactive(&std::fs::read_to_string(&reactive_src).expect("read ReactiveEventWatch.rs"));
    let encoding_types =
        unindent(&std::fs::read_to_string(&encoding_types_src).expect("read EncodingTypes.rs"));
    let json_codec = unindent(
        &std::fs::read_to_string(&json_codec_src).expect("read JSONCodec.rs"),
    );
    let native_task =
        extract_native_task(&std::fs::read_to_string(&math_task_src).expect("read MathTaskMem.rs"));
    let shared =
        extract_shared(&std::fs::read_to_string(&math_task_src).expect("read MathTaskMem.rs"));

    let mut body = String::with_capacity(
        native_task.len() + shared.len() + reactive.len() + encoding_types.len() + json_codec.len() + 4,
    );
    body.push_str("use jet_foundation::Outcome::{JetAbsent, JetOutcome};\n");
    body.push_str(&native_task);
    body.push('\n');
    body.push_str(&shared);
    body.push('\n');
    body.push_str(&reactive);
    body.push('\n');
    body.push_str(&encoding_types);
    body.push('\n');
    body.push_str(&json_codec);

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("web_kernel_std.rs");
    std::fs::write(&out, body).expect("write web_kernel_std.rs");
}
fn write_native_scheduler(manifest: &Path) {
    let runtime_control_src =
        manifest.join("../jet-codegen/src/Prelude/Core/RuntimeControl.rs");
    let sources = [
        (manifest.join("../jet-codegen/src/Prelude/Deadline.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/WorkflowWait.rs"), false),
        (manifest.join("../jet-codegen/src/SchedulerHost.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Core/TimeMonotonic.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Core/TimeInstant.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Core/Realtime.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/CoreLib/Top/TimeSleep.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Scheduler.rs"), true),
        (manifest.join("../jet-codegen/src/Prelude/CoreLib/Top/WorkflowSleep.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Stream.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Core/Duration.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Core/Time.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Core/Stream.rs"), false),
        (manifest.join("../jet-codegen/src/Prelude/Observe.rs"), false),
    ];
    println!("cargo:rerun-if-changed={}", runtime_control_src.display());
    let mut body = String::new();
    for (source, native_cfg) in sources {
        println!("cargo:rerun-if-changed={}", source.display());
        let mut text = std::fs::read_to_string(&source)
            .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
        if native_cfg {
            text = text.replace("feature = \"jet_native_io\"", "all()");
        }
        body.push_str(&text);
        body.push_str("\n\n");
    }
    body.push_str(&extract_stm(
        &std::fs::read_to_string(&runtime_control_src)
            .unwrap_or_else(|error| panic!("read {}: {error}", runtime_control_src.display())),
    ));
    body.push_str("\n\n");

    let out =
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("web_kernel_native_scheduler.rs");
    std::fs::write(&out, body).expect("write web_kernel_native_scheduler.rs");
}

fn unindent(source: &str) -> String {
    source
        .lines()
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_reactive(raw: &str) -> String {
    let start = raw
        .find("// ── D-REACT1=B + D-DATARACE1=C")
        .expect("reactive marker");
    let end_sync = raw
        .find("    pub struct JetAsyncPolicy")
        .expect("JetAsyncPolicy marker");
    let start_hooks = raw
        .find("    struct JetHookListener<")
        .expect("JetHookListener marker");
    let end_hooks = raw
        .find("    pub struct WatchHandle")
        .expect("WatchHandle marker");

    let mut body = unindent(&raw[start..end_sync]);
    strip_orphan_derives(&mut body);
    body.push('\n');
    body.push_str(&unindent(&raw[start_hooks..end_hooks]));
    strip_orphan_derives(&mut body);
    body
}

fn extract_native_task(raw: &str) -> String {
    let start = raw
        .find("    struct JetTaskState<T: Send + 'static> {")
        .expect("JetTaskState marker");
    let end = raw
        .find("    fn jet_task_entries<T: Send + 'static>(")
        .expect("jet_task_entries marker");
    unindent(&raw[start..end])
}
fn extract_pool(raw: &str) -> String {
    let pool_start = raw
        .find("    enum JetPoolSlot<T>")
        .expect("JetPoolSlot marker");
    let pool_end = pool_start
        + raw[pool_start..]
            .find("    // D-MEM1 S6: an opaque-handle placeholder")
            .expect("JetPool display marker");
    let id_start = raw.find("    pub struct JetId<T>").expect("JetId marker");
    let id_end = raw
        .find("    impl<T> super::__jet_Equatable")
        .expect("JetId semantic adapter marker");

    let mut body = unindent(&raw[pool_start..pool_end]);
    body.push('\n');
    body.push_str(&unindent(&raw[id_start..id_end]));
    body
}

fn extract_shared(raw: &str) -> String {
    let start = raw
        .find(
            "    #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n    pub enum JetSharedRevisionError",
        )
        .expect("JetSharedRevisionError marker");
    let end = raw
        .find("    enum JetPoolSlot<T>")
        .expect("JetPoolSlot marker");
    unindent(&raw[start..end]).replace("crate::", "super::super::")
}

fn extract_stm(raw: &str) -> String {
    let start = raw.find("mod jet_stm {\n").expect("jet_stm marker");
    let end = raw[start..]
        .find("\n}\ntrait __jet_Serialize")
        .map(|offset| start + offset + 2)
        .expect("jet_stm end marker");
    raw[start..end].replace("crate::", "super::super::")
}

fn strip_orphan_derives(body: &mut String) {
    loop {
        let trimmed = body.trim_end();
        let orphan = trimmed.ends_with("#[derive(Clone)]")
            || trimmed.ends_with("#[derive(Clone, Copy)]")
            || trimmed.ends_with("#[derive(Clone, Copy, Debug, Eq, PartialEq)]")
            || trimmed.ends_with("#[derive(Clone, Copy, Debug, PartialEq, Eq)]");
        if !orphan {
            break;
        }
        if let Some(index) = body.rfind("#[derive") {
            body.truncate(index);
            *body = body.trim_end().to_string();
        } else {
            break;
        }
    }
}
