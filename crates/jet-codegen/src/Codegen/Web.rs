//! Shared Web runtime/publication utilities.
//!
//! MIR Web owns target representation and emission. This module contains only
//! neutral assets shared by the MIR adapter, plugin guest generation, and the
//! artifact writer; it does not inspect compiler semantic state or emit a bundle.


const DOM_RUNTIME: &str = include_str!("../Prelude/DomRuntime.js");
const JS_EXECUTION_PRELUDE: &str = concat!(
    include_str!("../Prelude/Core/RuntimeStop.js"),
    include_str!("../Prelude/Core/Keep.js"),
    include_str!("../Prelude/Core/Task.js"),
    "\n",
    include_str!("../Prelude/Core/Event.js"),
    "\n",
    include_str!("../Prelude/Core/Channel.js"),
    include_str!("../Prelude/Core/DeterministicWorld.js"),
    "\n",
    include_str!("../Prelude/Core/Time.js"),
    "\n",
    include_str!("../Prelude/Core/Stream.js"),
    "\n",
    include_str!("../Prelude/Core/Realtime.js"),
    "\n",
    include_str!("../Prelude/Core/Option.js"),
    "\n",
    include_str!("../Prelude/Core/FixedList.js"),
    "\n",
    include_str!("../Prelude/Core/Collections.js"),
    "\n",
    include_str!("../Prelude/Core/Text.js"),
    "\n",
    include_str!("../Prelude/Core/Data.js"),
    "\n",
    include_str!("../Prelude/Core/TestingHistory.js"),
    "\n",
    "\n",
    include_str!("../Prelude/Core/StringConcat.js"),
    "\n",
    include_str!("../Prelude/Core/ViewCopy.js"),
    "\n",
    include_str!("../Prelude/Core/Values.js"),
    "\n",
    include_str!("../Prelude/Core/Gc.js"),
    "\n",
    include_str!("../Prelude/Core/TypedText.js"),
    "\n",
    include_str!("../Prelude/Core/Expiring.js"),
    "\n",
    include_str!("../Prelude/Core/ExactNumbers.js"),
    "\n",
    include_str!("../Prelude/Core/NumericWeb.js"),
    "\n",
    include_str!("../Prelude/Core/Measurement.js"),
    "\n",
    include_str!("../Prelude/Core/Shared.js"),
    "\n",
    include_str!("../Prelude/Core/Host.js"),
    "\n",
    include_str!("../Prelude/Core/Ui.js"),
    "\n",
    include_str!("../Prelude/Core/HttpRouter.js"),
    include_str!("../Prelude/Core/OpenAPI.js"),
    "\n",
    "\n",
    include_str!("../Prelude/Core/Authority.js"),
    "\n",
    "\n",
    include_str!("../Prelude/Core/InlineRange.js"),
    "\n",
    include_str!("../Prelude/Core/ComputeWebGpu.js"),
    "\n",
    include_str!("../Prelude/Core/Raylib.js"),
    "\n",
    include_str!("../Prelude/Core/Game.js"),
);

const JS_TASK_GROUP_PRELUDE: &str = r#"
function jet_task_group_body_failed() {
  return { tag: "Err", values: [{ tag: "Panicked", values: ["task body failed"] }] };
}

async function jet_task_all(tasks, resultCarriers = []) {
  const values = [];
  for (let index = 0; index < tasks.length; index += 1) {
    const joined = await jet_task_join(tasks[index]);
    if (joined?.tag !== "Ok") return joined;
    let value = joined.values[0];
    const resultCarrier = resultCarriers.length === 1
      ? resultCarriers[0]
      : resultCarriers[index];
    if (resultCarrier) {
      if (value?.tag !== "Ok") return jet_task_group_body_failed();
      value = value.values[0];
    }
    values.push(value);
  }
  return { tag: "Ok", values: [values] };
}

async function jet_task_all_named(tasks, authoredFields, resultFields, resultCarriers = []) {
  const values = Object.create(null);
  for (let index = 0; index < authoredFields.length; index += 1) {
    const field = authoredFields[index];
    const joined = await jet_task_join(tasks[field]);
    if (joined?.tag !== "Ok") return joined;
    let value = joined.values[0];
    if (resultCarriers[index]) {
      if (value?.tag !== "Ok") return jet_task_group_body_failed();
      value = value.values[0];
    }
    values[field] = value;
  }
  const result = {};
  for (const field of resultFields) result[field] = values[field];
  return { tag: "Ok", values: [result] };
}
"#;

/// The shared DOM runtime source used by MIR Web publication and assets.
pub(crate) fn dom_runtime_source() -> &'static str {
    DOM_RUNTIME
}

/// Assemble the JavaScript runtime shared by MIR Web emission.
pub(crate) fn shared_js_prelude() -> Result<String, std::io::Error> {
    let mut out = String::from(JS_EXECUTION_PRELUDE);
    out.push_str(&canonical_web_harfbuzz_asset()?);
    out.push_str(&format!(
        "\nconst JET_GAME_DEFAULT_FRAME_BUDGET = {};\nconst JET_GAME_FRAME_BUDGET_ERROR = {};\n",
        jet_foundation::Game::JetGameFrameBudget::DEFAULT_HEADLESS_FRAMES,
        json_quote(jet_foundation::Game::JetGameFrameBudget::FRAME_BUDGET_ERROR),
    ));
    out.push_str(&js_time_zone_prelude()?);
    out.push_str(JS_TASK_GROUP_PRELUDE);
    out.push_str(&js_runtime_stop_metadata());
    Ok(out)
}


/// Embed the canonical HarfBuzz Wasm adapter. Ui.js initializes it through
/// `jet_ui_web_harfbuzz_imports()` and supplies its low-level functions to app.wasm.
/// Missing, malformed, or oversized artifacts fail web code generation rather
/// than leaving a callback or metric approximation in the generated bundle.
fn canonical_web_harfbuzz_asset() -> Result<String, std::io::Error> {
    use std::fmt::Write as _;

    const ARTIFACT_PATH: &str = "wasm/jet_harfbuzz.wasm";
    const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
    const BYTES: &[u8] = include_bytes!("../../../../site/assets/wasm/jet_harfbuzz.wasm");
    if BYTES.len() > MAX_ARTIFACT_BYTES
        || BYTES.len() < 8
        || &BYTES[..4] != b"\0asm"
        || BYTES[4..8] != [1, 0, 0, 0]
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid canonical HarfBuzz Wasm artifact: {ARTIFACT_PATH}"),
        ));
    }
    let digest = jet_foundation::SHA256::sha256_hex(BYTES);
    let mut out = String::from(
        "\n// D-FOUND-PLATFORM1=A: the checked HarfBuzz bridge travels with the app.\n\
         const __jetCanonicalHarfBuzzWasmAsset = Object.freeze({path:",
    );
    out.push_str(&json_quote(ARTIFACT_PATH));
    out.push_str(",sha256:");
    out.push_str(&json_quote(&digest));
    out.push_str(",bytes:new Uint8Array([");
    for (index, byte) in BYTES.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        write!(out, "{byte}").expect("writing canonical HarfBuzz Wasm byte");
    }
    out.push_str(
        "])});\n\
         globalThis.__jetCanonicalHarfBuzzWasm = __jetCanonicalHarfBuzzWasmAsset;\n",
    );
    Ok(out)
}


fn web_tzdb_files() -> Result<Vec<(String, Vec<u8>)>, std::io::Error> {
    fn collect(
        root: &std::path::Path,
        dir: &std::path::Path,
        files: &mut Vec<(String, Vec<u8>)>,
    ) -> Result<(), std::io::Error> {
        let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                collect(root, &path, files)?;
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            if !bytes.starts_with(b"TZif") {
                continue;
            }
            let name = path
                .strip_prefix(root)
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "TZif path must be below the TZif root",
                    )
                })?
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .collect::<Vec<_>>()
                .join("/");
            files.push((name, bytes));
        }
        Ok(())
    }

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corelib/tzdb");
    let mut files = Vec::new();
    collect(&root, &root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn js_time_zone_prelude() -> Result<String, std::io::Error> {
    use std::fmt::Write as _;

    let mut out =
        String::from("\n// D-TIMEDEPTH1: exact repository TZif inputs for browser named zones.\n");
    for (name, bytes) in web_tzdb_files()? {
        write!(
            out,
            "JET_TIME_ZONE_DATA[{}] = new Uint8Array([",
            json_quote(&name)
        )
        .expect("writing generated TZif JavaScript");
        for byte in bytes {
            write!(out, "{byte},").expect("writing generated TZif JavaScript byte");
        }
        out.push_str("]);\n");
    }
    Ok(out)
}

/// Build a Wasm Source Map v3 from rustc DWARF rows and generated Jet markers.
pub fn build_wasm_jet_source_map(
    wasm: &[u8],
    rust_src: &str,
    source_names: &[String],
    source_contents: &[String],
) -> Result<String, String> {
    let code_off = jet_foundation::WasmDebug::code_section_payload_offset(wasm)
        .map_err(|e| format!("wasm code section: {e:?}"))?
        .ok_or_else(|| "wasm module has no Code section".to_string())?;
    let dwarf = jet_foundation::WasmDebug::parse_debug_line(wasm)
        .map_err(|e| format!("wasm .debug_line: {e:?}"))?;
    let rust_to_jet = rust_marker_table(rust_src);
    let name_index: std::collections::HashMap<&str, usize> = source_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    let mut mappings = Vec::new();
    let mut last_offset = None;
    for row in &dwarf {
        if !row.is_stmt || row.end_sequence || row.line == 0 {
            continue;
        }
        // Only rows that land in our generated guest Rust.
        if !row.file.ends_with("app_wasm.rs") && !row.file.contains("app_wasm.rs") {
            // rustc may record just the basename or a staging path.
            let base = std::path::Path::new(&row.file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(row.file.as_str());
            if base != "app_wasm.rs" {
                continue;
            }
        }
        let Some((source_name, jet_line)) = rust_to_jet.jet_for_rust_line(row.line as usize) else {
            continue;
        };
        let Some(&source) = name_index.get(source_name.as_str()) else {
            continue;
        };
        let file_offset = code_off
            .checked_add(row.address as usize)
            .ok_or_else(|| "wasm mapping offset overflow".to_string())?;
        if last_offset == Some(file_offset) {
            continue;
        }
        last_offset = Some(file_offset);
        mappings.push(JSMapping {
            generated_line: 0,
            generated_column: file_offset,
            source,
            original_line: jet_line.saturating_sub(1),
        });
    }
    mappings.sort_by_key(|m| {
        (
            m.generated_line,
            m.generated_column,
            m.source,
            m.original_line,
        )
    });
    let encoded = encode_source_mappings(&mappings);
    let names = source_names
        .iter()
        .map(|n| json_quote(n))
        .collect::<Vec<_>>()
        .join(",");
    let contents = source_contents
        .iter()
        .map(|c| json_quote(c))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"version\":3,\"file\":\"app.wasm\",\"sources\":[{names}],\"sourcesContent\":[{contents}],\"names\":[],\"mappings\":{}}}\n",
        json_quote(&encoded)
    ))
}

struct RustMarkerTable {
    /// rust line -> (source_name, jet line)
    rows: std::collections::BTreeMap<usize, (String, usize)>,
}

impl RustMarkerTable {
    fn jet_for_rust_line(&self, rust_line: usize) -> Option<(String, usize)> {
        self.rows
            .range(..=rust_line)
            .next_back()
            .map(|(_, v)| v.clone())
    }
}

fn rust_marker_table(rust_src: &str) -> RustMarkerTable {
    let mut rows = std::collections::BTreeMap::new();
    let mut pending_source: Option<String> = None;
    let mut pending_line: Option<usize> = None;
    for (i, line) in rust_src.lines().enumerate() {
        let rust_line = i + 1;
        let trim = line.trim_start();
        if let Some(name) = trim.strip_prefix("// jet:source-map source=") {
            pending_source = Some(name.trim().to_string());
            continue;
        }
        if let Some(n) = trim.strip_prefix("// jet:line ") {
            pending_line = n.trim().parse().ok();
            continue;
        }
        if let (Some(source), Some(jet_line)) = (pending_source.as_ref(), pending_line.take()) {
            rows.entry(rust_line)
                .or_insert_with(|| (source.clone(), jet_line));
        }
    }
    RustMarkerTable { rows }
}

fn json_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct JSMapping {
    generated_line: usize,
    generated_column: usize,
    source: usize,
    original_line: usize,
}

fn encode_source_mappings(mappings: &[JSMapping]) -> String {
    let mut out = String::new();
    let mut generated_line = 0usize;
    let mut generated_column = 0i64;
    let mut source = 0i64;
    let mut original_line = 0i64;
    let mut original_column = 0i64;
    let mut first_segment = true;

    for mapping in mappings {
        while generated_line < mapping.generated_line {
            out.push(';');
            generated_line += 1;
            generated_column = 0;
            first_segment = true;
        }
        if !first_segment {
            out.push(',');
        }
        let next_generated_column = mapping.generated_column as i64;
        let next_source = mapping.source as i64;
        let next_original_line = mapping.original_line as i64;
        encode_base64_vlq(next_generated_column - generated_column, &mut out);
        encode_base64_vlq(next_source - source, &mut out);
        encode_base64_vlq(next_original_line - original_line, &mut out);
        encode_base64_vlq(-original_column, &mut out);
        generated_column = next_generated_column;
        source = next_source;
        original_line = next_original_line;
        original_column = 0;
        first_segment = false;
    }
    out
}

fn encode_base64_vlq(value: i64, out: &mut String) {
    const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut value = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    };
    loop {
        let mut digit = (value & 31) as usize;
        value >>= 5;
        if value != 0 {
            digit |= 32;
        }
        out.push(BASE64[digit] as char);
        if value == 0 {
            break;
        }
    }
}

fn js_runtime_stop_metadata() -> String {
    let report_template = |report: jet_foundation::Outcome::JetRuntimeDiagnostic| {
        let rich_location = format!(
            "  --> __JET_RUNTIME_FILE__:{} in __JET_RUNTIME_FUNCTION__\n",
            u32::MAX
        );
        let plain_location = format!("  --> __JET_RUNTIME_FILE__:{}\n", u32::MAX);
        report
            .rendered
            .replace(&rich_location, "")
            .replace(&plain_location, "")
            .replace(&u32::MAX.to_string(), "__JET_RUNTIME_LINE__")
            .replace("\n Why:", "\n__JET_RUNTIME_CONTEXT__\n Why:")
    };
    let default_report = jet_foundation::Outcome::jet_render_runtime_stop(
        "__JET_RUNTIME_STOP_CODE__",
        "__JET_RUNTIME_FILE__",
        u32::MAX,
        "__JET_RUNTIME_FUNCTION__",
        "",
        1,
        1,
        "__JET_RUNTIME_MESSAGE__",
        "",
    );
    let default_source = default_report.source;
    let default_exit_code = default_report.exit_code;
    let default_rendered = report_template(default_report);
    let mut out = format!(
        "const JET_RUNTIME_STACK = [];\n\
         const JET_RUNTIME_STACK_LIMIT = {};\n\
         const JET_STACK_OVERFLOW_MESSAGE = {};\n\
         const JET_RUNTIME_STOP_DEFAULT = Object.freeze({{ source: {}, exit_code: {}, rich_context: false, rendered: {} }});\n\
         const JET_RUNTIME_STOP_METADATA = Object.freeze({{\n",
        jet_foundation::Outcome::JET_RUNTIME_STACK_LIMIT,
        json_quote(&jet_foundation::Outcome::jet_stack_overflow_message(
            "__JET_RUNTIME_FUNCTION__",
        )),
        json_quote(&default_source),
        default_exit_code,
        json_quote(&default_rendered),
    );
    for row in jet_foundation::Registry::diagnostic_rows()
        .iter()
        .filter(|row| {
            row.stage == "runtime"
                && row.status == jet_foundation::Registry::DiagnosticStatus::Active
        })
    {
        let message = if row.template_holes.iter().any(|hole| *hole == "type") {
            "__JET_RUNTIME_TODO_PREFIX__ — expected __JET_RUNTIME_TODO_TYPE__"
        } else {
            "__JET_RUNTIME_MESSAGE__"
        };
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            row.code,
            "__JET_RUNTIME_FILE__",
            u32::MAX,
            "__JET_RUNTIME_FUNCTION__",
            "",
            1,
            1,
            message,
            "",
        );
        let source = report.source;
        let exit_code = report.exit_code;
        let rich_context = jet_foundation::Outcome::jet_runtime_stop_has_context(report.code);
        let rendered = report_template(report);
        out.push_str(&format!(
            "  {}: Object.freeze({{ source: {}, exit_code: {}, rich_context: {}, rendered: {} }}),\n",
            json_quote(row.code),
            json_quote(&source),
            exit_code,
            rich_context,
            json_quote(&rendered),
        ));
    }
    out.push_str("});\n\n");
    out
}
