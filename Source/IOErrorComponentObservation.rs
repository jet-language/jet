//! Private IO11 component fixture preparation. Attached only by the reviewed
//! CmdCompile cfg-test composition hunk; never a production guest observer.

use super::{write_plugin_artifacts, OutputMode, PluginBuildError, PluginBuildPaths};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_COMPONENT_BYTES: u64 = 16 * 1024 * 1024;
const RESOURCE: &str = "io11-original-resource-positive9";
const CAUSE: &str = "io11-original-cause-positive9";

const GUEST_SOURCE: &str = r#"use core.files as files

#Export(c) pub fn probe(path: String) -> String {
    text :: files.read(path) ?? {
        return if err == {
            .AlreadyExists(context) -> {
                resource := context.resource ?? ""
                cause := context.cause ?? ""
                if context.operation == .Read && resource == "io11-original-resource-positive9" && context.os_code == None && cause == "io11-original-cause-positive9" {
                    return "AlreadyExists|original-context-preserved"
                }
                "AlreadyExists|wrong-context"
            }
            else -> "unexpected-error"
        }
    }
    "unexpected-ok"
}

fn run() {}
"#;

struct PreparedFixture {
    paths: PluginBuildPaths,
    component: Vec<u8>,
    component_digest: String,
    compiler_digest: String,
    source_closure: Vec<(PathBuf, String)>,
    uninstrumented_guest_digest: String,
    instrumented_guest_digest: String,
    wit_digest: String,
    helper_digests: [String; 3],
}

// The private test below uses the existing Driver/CLI default gates and dev
// profile. The existing Driver snapshots the real entry/import closure once.
fn prepare_positive9_fixture(
    directory: &Path,
    gates: jet::Policy::GateSet,
    profile: &str,
    mode: OutputMode,
) -> PreparedFixture {
    std::fs::create_dir_all(directory).expect("owned fixture directory");
    let entry = directory.join("io11_positive9.jet");
    std::fs::write(&entry, GUEST_SOURCE).expect("write registered fixture source");
    std::fs::write(
        directory.join("package.jet"),
        "name: \"io11_positive9\"\nversion: \"0.1.0\"\nauthority: { needs: [FS.Read], holds: { allow: [FS.Read, IO, Mem.Alloc] } }\n",
    )
    .expect("write actual guest package authority");
    let entry_name = entry.to_str().expect("fixture source path");
    let source_closure =
        jet::Driver::load_immutable_source_closure(entry_name, &[(&entry, GUEST_SOURCE)])
            .expect("snapshot actual fixture and import sources");
    let output = jet::Driver::compile_bundle_path_opts_with_source_closure(
        entry_name,
        jet::Sema::CompileMode::Check,
        false,
        gates,
        false,
        true,
        false,
        false,
        Some(jet::Syntax::TARGET_SANDBOX),
        None,
        profile,
        &BTreeMap::new(),
        false,
        None,
        &source_closure,
    )
    .expect("selected compiler checks and emits the real sandbox guest");
    let mut plugin = output.plugin.expect("actual checked PluginArtifacts");
    let uninstrumented_guest_digest =
        jet_foundation::SHA256::sha256_hex(plugin.guest_rust.as_bytes());
    let wit_digest = jet_foundation::SHA256::sha256_hex(plugin.wit.as_bytes());
    let helper_digests = instrument_positive9_fixture(&mut plugin);
    let instrumented_guest_digest =
        jet_foundation::SHA256::sha256_hex(plugin.guest_rust.as_bytes());
    let paths = write_plugin_artifacts(entry_name, &plugin, false, directory, mode)
        .unwrap_or_else(|error| match error {
            PluginBuildError::ToolFailure(reason) => {
                panic!("canonical writer tool failure: {reason}")
            }
            PluginBuildError::GeneratedCodeRejected(reason) => {
                panic!("canonical writer generated-code rejection: {reason}")
            }
        });
    let expected_digest = jet_foundation::SHA256::sha256_file_hex(&paths.component_wasm)
        .expect("hash canonical writer component");
    let file = std::fs::File::open(&paths.component_wasm).expect("open actual writer output");
    let metadata = file.metadata().expect("opened component metadata");
    assert!(metadata.is_file() && metadata.len() <= MAX_COMPONENT_BYTES);
    let mut component = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_COMPONENT_BYTES + 1)
        .read_to_end(&mut component)
        .expect("bounded read of actual component bytes");
    assert!(!component.is_empty() && component.len() as u64 <= MAX_COMPONENT_BYTES);
    let component_digest = jet_foundation::SHA256::sha256_hex(&component);
    assert_eq!(component_digest, expected_digest);
    assert_eq!(
        jet_foundation::SHA256::sha256_file_hex(&paths.component_wasm)
            .expect("revalidate canonical component after read"),
        component_digest,
    );
    let compiler_digest = jet_foundation::SHA256::sha256_file_hex(
        &std::env::current_exe().expect("actual selected compiler test executable"),
    )
    .expect("hash actual compiler executable");
    println!(
        "io11 fixture compiler={compiler_digest} guest={uninstrumented_guest_digest} instrumented={instrumented_guest_digest} wit={wit_digest} component={component_digest} helpers={helper_digests:?} source_closure_items={}",
        source_closure.len(),
    );
    PreparedFixture {
        paths,
        component,
        component_digest,
        compiler_digest,
        source_closure,
        uninstrumented_guest_digest,
        instrumented_guest_digest,
        wit_digest,
        helper_digests,
    }
}

#[test]
#[ignore = "requires the qualified selected Source compiler and admitted canonical component materializer"]
fn materialize_positive9_component_with_original_context_allocations() {
    // TMPDIR is supplied by the existing canonical job launcher. Outputs stay
    // outside the repository and are retained for Main's frozen-byte custody.
    let temporary = std::env::temp_dir().canonicalize().expect("canonical launcher TMPDIR");
    assert!(
        !temporary.starts_with(env!("CARGO_MANIFEST_DIR")),
        "generated fixture outputs must stay outside the repository",
    );
    let directory = temporary.join(format!(
        "io11-positive9-component-{}",
        std::process::id(),
    ));
    assert!(!directory.exists(), "fresh owned materialization output");
    let fixture = prepare_positive9_fixture(
        &directory,
        jet::Policy::GateSet::default(),
        "dev",
        OutputMode {
            json: false,
            color: jet::Diagnostics::ColorChoice::Never,
            quiet: false,
        },
    );
    for (path, source) in &fixture.source_closure {
        println!(
            "io11 checked-input path={:?} bytes={} sha256={}",
            path,
            source.len(),
            jet_foundation::SHA256::sha256_hex(source.as_bytes()),
        );
    }
    println!(
        "io11 materialized-component path={:?} bytes={} sha256={} compiler={} guest={} instrumented={} wit={} helpers={:?}",
        fixture.paths.component_wasm,
        fixture.component.len(),
        fixture.component_digest,
        fixture.compiler_digest,
        fixture.uninstrumented_guest_digest,
        fixture.instrumented_guest_digest,
        fixture.wit_digest,
        fixture.helper_digests,
    );
}

fn instrument_positive9_fixture(plugin: &mut jet::Codegen::PluginArtifacts) -> [String; 3] {
    use jet_foundation::Names::{mangle, mangle_generated};
    assert_eq!(plugin.exported_fns, ["probe"]);
    assert!(!plugin.guest_rust.contains("#[global_allocator]"));
    assert!(!plugin.guest_rust.contains("io_boundary_allocation_witness"));
    let allocator = mangle_generated("plugin_cabi_realloc_impl");
    let allocator_export = mangle_generated("plugin_cabi_realloc");
    let return_text = mangle_generated("plugin_return_text");
    let post_text = mangle_generated("plugin_post_text");
    let post_export = mangle("plugin_post_probe");
    let read_io = mangle_generated("plugin_read_io_error");
    let free_io = mangle_generated("plugin_free_io_error");
    let helpers = [&read_io, &free_io, &allocator];
    let helpers_before = helpers.map(|name| {
        jet_foundation::SHA256::sha256_hex(
            exact_fixture_function(&plugin.guest_rust, name).as_bytes(),
        )
    });
    let original_export = format!(
        "#[export_name = \"cabi_realloc\"]\npub extern \"C\" fn {allocator_export}(\n    old_ptr: i32,\n    old_len: i32,\n    align: i32,\n    new_len: i32,\n) -> i32 {{\n    {allocator}(old_ptr, old_len, align, new_len)\n}}"
    );
    let observed_export = format!(
        "#[export_name = \"cabi_realloc\"]\npub extern \"C\" fn {allocator_export}(\n    old_ptr: i32,\n    old_len: i32,\n    align: i32,\n    new_len: i32,\n) -> i32 {{\n    let pointer = {allocator}(old_ptr, old_len, align, new_len);\n    io_boundary_allocation_witness::canonical_result(old_ptr, old_len, align, new_len, pointer);\n    pointer\n}}"
    );
    replace_fixture_once(&mut plugin.guest_rust, &original_export, &observed_export);
    let callback = format!("jet_ffi_callback_boundary(|| {{ {return_text}(");
    let observed_callback = format!(
        "io_boundary_allocation_witness::begin(Some({RESOURCE:?}.as_bytes()), Some({CAUSE:?}.as_bytes())); jet_ffi_callback_boundary(|| {{ {return_text}(io_ownership_fixture_result("
    );
    assert_eq!(plugin.guest_rust.matches(&callback).count(), 1);
    let start = plugin.guest_rust.find(&callback).expect("actual scalar Text callback");
    let ending = ") }) }\n";
    let end = start
        + plugin.guest_rust[start..]
            .find(ending)
            .expect("actual scalar Text callback ending");
    plugin.guest_rust.replace_range(end..end + ending.len(), ")) }) }\n");
    replace_fixture_once(&mut plugin.guest_rust, &callback, &observed_callback);
    let original_post = format!(
        "#[export_name = \"cabi_post_probe\"]\npub extern \"C\" fn {post_export}(ret_ptr: i32) {{ {post_text}(ret_ptr) }}"
    );
    let observed_post = format!(
        "#[export_name = \"cabi_post_probe\"]\npub extern \"C\" fn {post_export}(ret_ptr: i32) {{ {post_text}(ret_ptr); io_boundary_allocation_witness::finish(); }}"
    );
    replace_fixture_once(&mut plugin.guest_rust, &original_post, &observed_post);
    plugin.guest_rust.push_str(ALLOCATION_OBSERVER_SOURCE);
    plugin.guest_rust.push_str(
        "\nfn io_ownership_fixture_result(value: String) -> String {\n    let events = io_boundary_allocation_witness::released_events();\n    format!(\"{}|ownership-events={:?}\", value, events)\n}\n",
    );
    let helpers_after = helpers.map(|name| {
        jet_foundation::SHA256::sha256_hex(
            exact_fixture_function(&plugin.guest_rust, name).as_bytes(),
        )
    });
    assert_eq!(helpers_before, helpers_after, "actual decode/free/allocator bodies unchanged");
    helpers_before
}

fn replace_fixture_once(source: &mut String, from: &str, to: &str) {
    let mut matches = source.match_indices(from);
    let (start, _) = matches.next().expect("one actual wrapper");
    assert!(matches.next().is_none(), "exact registered fixture wrapper");
    source.replace_range(start..start + from.len(), to);
}

fn exact_fixture_function<'a>(source: &'a str, name: &str) -> &'a str {
    let signature = format!("fn {name}(");
    let mut matches = source.match_indices(&signature);
    let (start, _) = matches.next().expect("one generated helper");
    assert!(matches.next().is_none(), "one exact generated helper");
    let tail = &source[start..];
    let end = tail.find("\n}\n").expect("fixed generated helper closing line") + 3;
    &tail[..end]
}

const ALLOCATION_OBSERVER_SOURCE: &str = r#"
mod io_boundary_allocation_witness {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::Mutex;
    const CAPACITY: usize = 512;
    #[derive(Clone, Copy)]
    struct Allocation {
        generation: u64, pointer: usize, size: usize, alignment: usize,
        live: bool, canonical: bool, role: u8, releases: u8,
    }
    const EMPTY: Allocation = Allocation {
        generation: 0, pointer: 0, size: 0, alignment: 0,
        live: false, canonical: false, role: 0, releases: 0,
    };
    struct Ledger {
        allocations: [Allocation; CAPACITY], generation: u64, active: bool,
        resource: Option<&'static [u8]>, cause: Option<&'static [u8]>,
    }
    static LEDGER: Mutex<Ledger> = Mutex::new(Ledger {
        allocations: [EMPTY; CAPACITY], generation: 0, active: false,
        resource: None, cause: None,
    });
    fn invalid() -> ! { core::arch::wasm32::unreachable() }
    fn locked() -> std::sync::MutexGuard<'static, Ledger> {
        match LEDGER.lock() { Ok(ledger) => ledger, Err(_) => invalid() }
    }
    fn allocated(pointer: *mut u8, layout: Layout) {
        if pointer.is_null() { return; }
        let mut ledger = locked();
        if ledger.allocations.iter().any(|entry| entry.live && entry.pointer == pointer as usize) {
            invalid();
        }
        let slot = match ledger.allocations.iter().position(|entry| !entry.live && !entry.canonical) {
            Some(slot) => slot, None => invalid(),
        };
        ledger.generation = match ledger.generation.checked_add(1) {
            Some(generation) => generation, None => invalid(),
        };
        ledger.allocations[slot] = Allocation {
            generation: ledger.generation, pointer: pointer as usize,
            size: layout.size(), alignment: layout.align(), live: true,
            canonical: false, role: 0, releases: 0,
        };
    }
    fn live_slot(ledger: &Ledger, pointer: *mut u8, layout: Layout) -> usize {
        match ledger.allocations.iter().position(|entry| {
            entry.live && entry.pointer == pointer as usize
                && entry.size == layout.size() && entry.alignment == layout.align()
        }) { Some(slot) => slot, None => invalid() }
    }
    unsafe fn releasing(pointer: *mut u8, layout: Layout) {
        let mut ledger = locked();
        let slot = live_slot(&ledger, pointer, layout);
        let entry = ledger.allocations[slot];
        let role = if entry.canonical {
            let bytes = unsafe { std::slice::from_raw_parts(pointer, layout.size()) };
            if ledger.resource == Some(bytes) { 1 }
            else if ledger.cause == Some(bytes) { 2 }
            else { invalid() }
        } else { 0 };
        ledger.allocations[slot].role = role;
        ledger.allocations[slot].releases = match entry.releases.checked_add(1) {
            Some(count) if count == 1 => count, _ => invalid(),
        };
        ledger.allocations[slot].live = false;
    }
    pub struct Observer;
    unsafe impl GlobalAlloc for Observer {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            allocated(pointer, layout);
            pointer
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            allocated(pointer, layout);
            pointer
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { releasing(pointer, layout) };
            unsafe { System.dealloc(pointer, layout) };
        }
        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let slot = {
                let ledger = locked();
                let slot = live_slot(&ledger, pointer, layout);
                if ledger.allocations[slot].canonical { invalid(); }
                slot
            };
            let resized = unsafe { System.realloc(pointer, layout, size) };
            if !resized.is_null() {
                locked().allocations[slot].live = false;
                let new_layout = match Layout::from_size_align(size, layout.align()) {
                    Ok(layout) => layout, Err(_) => invalid(),
                };
                allocated(resized, new_layout);
            }
            resized
        }
    }
    pub fn begin(resource: Option<&'static [u8]>, cause: Option<&'static [u8]>) {
        let mut ledger = locked();
        if ledger.active || resource.filter(|bytes| !bytes.is_empty()) == cause.filter(|bytes| !bytes.is_empty())
            && resource.is_some_and(|bytes| !bytes.is_empty()) { invalid(); }
        for entry in &mut ledger.allocations {
            if entry.canonical {
                if entry.live || entry.releases != 1 { invalid(); }
                *entry = EMPTY;
            }
        }
        ledger.resource = resource;
        ledger.cause = cause;
        ledger.active = true;
    }
    pub fn canonical_result(old_pointer: i32, old_size: i32, alignment: i32, size: i32, pointer: i32) {
        let mut ledger = locked();
        if !ledger.active { return; }
        if old_pointer != 0 || old_size != 0 || size < 0 || alignment <= 0 { invalid(); }
        if size == 0 {
            if pointer != 0 { invalid(); }
            return;
        }
        let layout = match Layout::from_size_align(size as usize, alignment as usize) {
            Ok(layout) => layout, Err(_) => invalid(),
        };
        let slot = live_slot(&ledger, pointer as usize as *mut u8, layout);
        if ledger.allocations[slot].canonical { invalid(); }
        ledger.allocations[slot].canonical = true;
    }
    pub fn released_events() -> [(u8, u64, usize, usize, usize, u8); 2] {
        let ledger = locked();
        let mut events = [(0, 0, 0, 0, 0, 0); 2];
        for entry in &ledger.allocations {
            if !entry.canonical { continue; }
            if entry.live || entry.releases != 1 || entry.alignment != 1 || !(1..=2).contains(&entry.role) { invalid(); }
            let slot = entry.role as usize - 1;
            if events[slot].0 != 0 { invalid(); }
            events[slot] = (entry.role, entry.generation, entry.pointer, entry.size, entry.alignment, entry.releases);
        }
        for (index, expected) in [ledger.resource, ledger.cause].into_iter().enumerate() {
            let needs_storage = expected.is_some_and(|bytes| !bytes.is_empty());
            if (events[index].0 != 0) != needs_storage { invalid(); }
        }
        events
    }
    pub fn finish() {
        let _ = released_events();
        let mut ledger = locked();
        if !ledger.active { invalid(); }
        ledger.active = false;
    }
}
#[global_allocator]
static IO_BOUNDARY_ALLOCATOR: io_boundary_allocation_witness::Observer = io_boundary_allocation_witness::Observer;
"#;
