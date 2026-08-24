//! D-DBG3 step 2 (dap-debugger): the rust-line <-> jet-line table for the native
//! backend. Built by scanning generated Rust text for the `// jet:line N` markers
//! `TStmt::LineMarker` emits (only present when the bundle was compiled through
//! `jet::Codegen::emit_bundle_dbg(.., true)` — the native `jet debug` build path).
//!
//! I2: this is the ONLY place the native backend is allowed to reason about
//! generated-Rust line numbers directly; everywhere else (the `(jet)` prompt, the
//! DAP adapter) works in Jet lines, translated through this table.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::Path;

use jet_foundation::JSON::{json_escape, json_get, json_int, json_str, parse_json};
use jet_foundation::SHA256::{sha256_file_hex, sha256_hex};

const MAP_SCHEMA_VERSION: i64 = 1;

pub(crate) struct LineMap {
    /// Generated-Rust line -> Jet line, for every line a marker introduces.
    rust_to_jet: BTreeMap<usize, usize>,
    /// Jet line -> the FIRST rust line that begins it (used to place a
    /// `breakpoint set -f <file> -l <rust_line>` for a user's `break <jet_line>`).
    jet_to_rust: HashMap<usize, usize>,
    /// The generated entry wrapper begins after the Jet body. Lines there are
    /// Rust plumbing, not Jet source, even though the final Jet marker would
    /// otherwise remain the nearest preceding marker.
    jet_body_end: Option<usize>,
}

impl LineMap {
    /// Scan `rust_src` for `// jet:line N` markers (one per lowered `Stmt`,
    /// `crates/jet-codegen/src/Codegen/TIR/emit.rs`'s `TStmt::LineMarker` arm). The
    /// marker sits on its own line immediately before the statement's generated
    /// Rust, so the table maps that FOLLOWING rust line (not the comment's own
    /// line) to the Jet line.
    pub(crate) fn build(rust_src: &str) -> LineMap {
        let mut rust_to_jet = BTreeMap::new();
        let mut jet_to_rust = HashMap::new();
        let mut pending: Option<usize> = None;
        for (i, line) in rust_src.lines().enumerate() {
            let rust_line = i + 1;
            if let Some(n) = pending.take() {
                rust_to_jet.insert(rust_line, n);
                jet_to_rust.entry(n).or_insert(rust_line);
            }
            if let Some(rest) = line.trim_start().strip_prefix("// jet:line ") {
                pending = rest.trim().parse::<usize>().ok();
            }
        }
        let jet_body_end = if rust_src
            .lines()
            .any(|line| line.trim_start().starts_with("pub fn __jet_run("))
        {
            let mut in_jet_entry = false;
            rust_src.lines().enumerate().find_map(|(index, line)| {
                if line.trim_start().starts_with("pub fn __jet_run(") {
                    in_jet_entry = true;
                    return None;
                }
                (in_jet_entry && line.starts_with("fn main(")).then_some(index + 1)
            })
        } else {
            None
        };
        LineMap {
            rust_to_jet,
            jet_to_rust,
            jet_body_end,
        }
    }

    /// The Jet line a stopped rust frame belongs to: the marker at or immediately
    /// before `rust_line` (a multi-line Rust statement stops anywhere inside its
    /// span, not just the marker's own next line). `None` means the frame has no
    /// Jet line at all (prelude/generated glue) — the caller steps over it (I2).
    pub(crate) fn jet_line_for(&self, rust_line: usize) -> Option<usize> {
        if self
            .jet_body_end
            .is_some_and(|jet_body_end| rust_line >= jet_body_end)
        {
            return None;
        }
        self.rust_to_jet
            .range(..=rust_line)
            .next_back()
            .map(|(_, v)| *v)
    }

    /// Translate a frame only when LLDB's file identity agrees with the
    /// generated source recorded in the sidecar.  A line number without its
    /// file is not a safe source location: unrelated Rust/library frames can
    /// reuse the same line number.
    pub(crate) fn jet_line_for_file(
        &self,
        rust_file: &str,
        expected_rust_file: &str,
        rust_line: usize,
    ) -> Option<usize> {
        source_file_matches(rust_file, expected_rust_file)
            .then(|| self.jet_line_for(rust_line))
            .flatten()
    }

    /// The rust line to set a native breakpoint on for `break <jet_line>`.
    pub(crate) fn rust_line_for(&self, jet_line: usize) -> Option<usize> {
        self.jet_to_rust.get(&jet_line).copied()
    }

    /// The first marked rust line AT OR AFTER `rust_line` — used to place the
    /// initial breakpoint at the generated `__jet_run` body's first real
    /// statement by `-f -l` (line-based) rather than `-n main` (name-based,
    /// which can resolve to more than one symbol and land with no source line
    /// at all).
    pub(crate) fn first_at_or_after(&self, rust_line: usize) -> Option<usize> {
        self.rust_to_jet.range(rust_line..).next().map(|(k, _)| *k)
    }

    /// The rust line to set the INITIAL breakpoint on: `__jet_run`'s first real
    /// statement. Rust still has a tiny `fn main` wrapper, but the Jet user's
    /// code lives in `__jet_run`, which carries the line markers. Shared by the terminal session
    /// (`Native.rs`) and the DAP `launch` handler (`Dap.rs`) so both use the
    /// same file:line breakpoint, never the ambiguous `-n main`.
    pub(crate) fn main_entry_line(&self, rust_src: &str) -> Option<usize> {
        let entry_header = rust_src
            .lines()
            .position(|l| l.trim_start().starts_with("pub fn __jet_run("))
            .or_else(|| {
                rust_src
                    .lines()
                    .position(|l| l.trim_start().starts_with("fn main("))
            })
            .map(|i| i + 1)
            .unwrap_or(1);
        self.first_at_or_after(entry_header)
    }

    /// Persist the exact source-to-generated table used by a native debug
    /// build. The hashes make stale binaries and stale generated sources a
    /// hard failure at the adapter boundary instead of a plausible wrong
    /// breakpoint.
    pub(crate) fn write_artifact(
        path: &Path,
        jet_file: &str,
        jet_src: &str,
        rust_file: &str,
        rust_src: &str,
        binary: &Path,
    ) -> io::Result<()> {
        let map = Self::build(rust_src);
        let binary_sha256 = sha256_file_hex(binary)?;
        let entries = map
            .rust_to_jet
            .iter()
            .map(|(rust, jet)| format!("{{\"rust\":{rust},\"jet\":{jet}}}"))
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(
            "{{\"schema_version\":{MAP_SCHEMA_VERSION},\"jet_file\":\"{}\",\"rust_file\":\"{}\",\"jet_sha256\":\"{}\",\"rust_sha256\":\"{}\",\"binary_sha256\":\"{}\",\"entries\":[{}]}}\n",
            json_escape(jet_file),
            json_escape(rust_file),
            sha256_hex(jet_src.as_bytes()),
            sha256_hex(rust_src.as_bytes()),
            binary_sha256,
            entries,
        );
        static MAP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = MAP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = path.with_extension(format!("jetmap.tmp-{}-{counter}", std::process::id()));
        if let Err(error) = std::fs::write(&temporary, json.as_bytes()) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&temporary, path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }

    /// Load and verify a sidecar before it is allowed to translate a native
    /// stop. The generated source remains the source of truth for the caller,
    /// but the sidecar is the build identity check for editor/attach flows.
    pub(crate) fn load_verified(
        path: &Path,
        jet_file: &str,
        jet_src: &str,
        rust_file: &str,
        rust_src: &str,
        binary: &Path,
    ) -> Result<LineMap, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read debugger map {}: {error}", path.display()))?;
        let root = parse_json(&text).map_err(|()| "debugger map is not valid JSON".to_string())?;
        let schema = json_get(&root, "schema_version")
            .and_then(json_int)
            .ok_or_else(|| "debugger map has no schema version".to_string())?;
        if schema != MAP_SCHEMA_VERSION {
            return Err(format!("unsupported debugger map schema {schema}"));
        }
        let jet_sha256 = sha256_hex(jet_src.as_bytes());
        let rust_sha256 = sha256_hex(rust_src.as_bytes());
        for (field, expected) in [
            ("jet_file", jet_file),
            ("rust_file", rust_file),
            ("jet_sha256", jet_sha256.as_str()),
            ("rust_sha256", rust_sha256.as_str()),
        ] {
            let actual = json_get(&root, field)
                .and_then(json_str)
                .ok_or_else(|| format!("debugger map has no {field}"))?;
            if field.ends_with("_file") {
                let expected_path = std::fs::canonicalize(expected)
                    .ok()
                    .map(|path| path.display().to_string());
                if actual != expected
                    && expected_path.as_deref() != Some(actual)
                    && !(field == "rust_file"
                        && Path::new(actual).file_name() == Path::new(expected).file_name())
                {
                    return Err(format!("debugger map {field} does not match the target"));
                }
            } else if actual != expected {
                return Err(format!("debugger map {field} does not match the target"));
            }
        }
        let expected_binary = sha256_file_hex(binary)
            .map_err(|error| format!("cannot hash debugger binary: {error}"))?;
        let actual_binary = json_get(&root, "binary_sha256")
            .and_then(json_str)
            .ok_or_else(|| "debugger map has no binary_sha256".to_string())?;
        if actual_binary != expected_binary {
            return Err("debugger map does not match the debug binary".to_string());
        }
        let entries = json_get(&root, "entries")
            .and_then(|value| match value {
                jet_foundation::JSON::JSONValue::Array(values) => Some(values),
                _ => None,
            })
            .ok_or_else(|| "debugger map has no entries".to_string())?;
        let mut rust_to_jet = BTreeMap::new();
        let mut jet_to_rust = HashMap::new();
        for entry in entries {
            let rust = json_get(entry, "rust")
                .and_then(json_int)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| "debugger map has an invalid rust line".to_string())?;
            let jet = json_get(entry, "jet")
                .and_then(json_int)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| "debugger map has an invalid Jet line".to_string())?;
            if rust_to_jet.insert(rust, jet).is_some() {
                return Err("debugger map has duplicate line entries".to_string());
            }
            // Several generated statements may originate from one Jet line.
            // Preserve the first stoppable Rust line, matching `build` and
            // keeping source breakpoints deterministic.
            jet_to_rust.entry(jet).or_insert(rust);
        }
        // The hashes prove that this sidecar names the current build, but they
        // do not prove that its table was produced from that build. Rebuild the
        // table from the generated source and reject any edited mapping before
        // it can influence a breakpoint or frame projection.
        let expected = Self::build(rust_src);
        if rust_to_jet != expected.rust_to_jet || jet_to_rust != expected.jet_to_rust {
            return Err("debugger map entries do not match the generated source".to_string());
        }
        Ok(LineMap {
            rust_to_jet,
            jet_to_rust,
            jet_body_end: expected.jet_body_end,
        })
    }
}

fn source_file_matches(actual: &str, expected: &str) -> bool {
    if actual == expected {
        return true;
    }
    let actual_path = Path::new(actual);
    let expected_path = Path::new(expected);
    if std::fs::canonicalize(actual_path).ok() == std::fs::canonicalize(expected_path).ok()
        && actual_path.exists()
        && expected_path.exists()
    {
        return true;
    }
    matches!(
        (actual_path.file_name(), expected_path.file_name()),
        (Some(actual), Some(expected)) if actual == expected
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_marker_to_the_following_line() {
        let rust = "fn main() {\n    // jet:line 7\n    let x = 1;\n    // jet:line 8\n    let y = 2;\n}\n";
        let map = LineMap::build(rust);
        assert_eq!(map.jet_line_for(3), Some(7));
        assert_eq!(map.jet_line_for(5), Some(8));
        assert_eq!(map.rust_line_for(7), Some(3));
        assert_eq!(map.rust_line_for(8), Some(5));
    }

    #[test]
    fn multi_line_statement_resolves_to_the_marker_before_it() {
        let rust = "    // jet:line 12\n    let x = some_call(\n        1,\n        2,\n    );\n";
        let map = LineMap::build(rust);
        assert_eq!(map.jet_line_for(2), Some(12));
        assert_eq!(map.jet_line_for(4), Some(12));
    }

    #[test]
    fn no_marker_before_a_line_is_none() {
        let rust = "fn main() {\n    let x = 1;\n}\n";
        let map = LineMap::build(rust);
        assert_eq!(map.jet_line_for(2), None);
    }

    #[test]
    fn generated_entry_wrapper_is_not_projected_as_jet() {
        let rust = "pub fn __jet_run() {\n    // jet:line 2\n    let x = 1;\n}\nfn main() {\n    __jet_run();\n}\n";
        let map = LineMap::build(rust);
        assert_eq!(map.jet_line_for(3), Some(2));
        assert_eq!(map.jet_line_for(5), None);
    }

    #[test]
    fn file_aware_mapping_rejects_an_unrelated_frame() {
        let map = LineMap::build("fn main() {\n// jet:line 7\nlet x = 1;\n}\n");
        assert_eq!(
            map.jet_line_for_file("generated.rs", "generated.rs", 3),
            Some(7)
        );
        assert_eq!(map.jet_line_for_file("library.rs", "generated.rs", 3), None);
    }

    #[test]
    fn first_at_or_after_finds_the_next_marked_line() {
        let rust = "fn main() {\n    // jet:line 2\n    let x = 1;\n    // jet:line 3\n    let y = 2;\n}\n";
        let map = LineMap::build(rust);
        // The `fn main() {` line itself (1) has no marker on it — the search
        // should land on the first REAL statement's rust line (3), not 1.
        assert_eq!(map.first_at_or_after(1), Some(3));
        assert_eq!(map.first_at_or_after(4), Some(5));
        assert_eq!(map.first_at_or_after(6), None);
    }

    #[test]
    fn sidecar_round_trip_rejects_a_stale_binary() {
        let root = std::env::temp_dir().join(format!(
            "jet-debug-map-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::create_dir_all(&root);
        let binary = root.join("program");
        let map_path = root.join("program.jetmap");
        let jet_src = "main() {\n  print(1)\n}\n";
        let rust_src = "fn main() {\n// jet:line 2\nlet x = 1;\n}\n";
        std::fs::write(&binary, b"debug-binary").unwrap();
        LineMap::write_artifact(
            &map_path,
            "program.jet",
            jet_src,
            "program.rs",
            rust_src,
            &binary,
        )
        .unwrap();
        let loaded = LineMap::load_verified(
            &map_path,
            "program.jet",
            jet_src,
            "program.rs",
            rust_src,
            &binary,
        )
        .unwrap();
        assert_eq!(loaded.rust_line_for(2), Some(3));
        std::fs::write(&binary, b"stale-binary").unwrap();
        assert!(LineMap::load_verified(
            &map_path,
            "program.jet",
            jet_src,
            "program.rs",
            rust_src,
            &binary,
        )
        .is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sidecar_round_trip_rejects_edited_entries_with_matching_hashes() {
        let root = std::env::temp_dir().join(format!(
            "jet-debug-map-entry-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::create_dir_all(&root);
        let binary = root.join("program");
        let map_path = root.join("program.jetmap");
        let jet_src = "main() {\n  print(1)\n}\n";
        let rust_src = "fn main() {\n// jet:line 2\nlet x = 1;\n}\n";
        std::fs::write(&binary, b"debug-binary").unwrap();
        LineMap::write_artifact(
            &map_path,
            "program.jet",
            jet_src,
            "program.rs",
            rust_src,
            &binary,
        )
        .unwrap();
        let map = std::fs::read_to_string(&map_path).unwrap();
        let edited = map.replace("\"rust\":3,\"jet\":2", "\"rust\":3,\"jet\":99");
        assert_ne!(edited, map, "fixture must contain the expected mapping");
        std::fs::write(&map_path, edited).unwrap();
        let result = LineMap::load_verified(
            &map_path,
            "program.jet",
            jet_src,
            "program.rs",
            rust_src,
            &binary,
        );
        let error = match result {
            Ok(_) => panic!("edited sidecar entries must not be trusted"),
            Err(error) => error,
        };
        assert!(error.contains("entries do not match"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }
}
