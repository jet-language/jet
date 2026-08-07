//! D-MIGRATE1 (ratified 2026-06-22): published-schema snapshot — records the
//! field layout of a `#PublishedSchema` struct at release time.
//!
//! Format (std-only, lockfile-style — no serde, I6):
//!
//! ```text
//! schema_version = 1
//! type = UserRecord
//! published_version = 1.2.0
//! field name: String
//! field email: String
//! ```
//!
//! Lives at `.jet/cache/schema/<TypeName>.snapshot` (committed, durable contract).

use crate::Syntax;
use crate::AST::StructDef;

pub const SNAPSHOT_VERSION: u32 = 1;

/// One field entry in a schema snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotField {
    pub name: String,
    pub ty: String,
}

/// The full schema snapshot for one `#PublishedSchema` struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaSnapshot {
    pub schema_version: u32,
    pub type_name: String,
    pub published_version: String,
    /// D-MIGRATE2C: set by `jet inspect schema squash --before <ver>` to re-baseline this
    /// record. When present, the snapshot's `fields` are the *current* authoritative
    /// shape and migration ops for any version `< squashed_before` are no longer
    /// required — the diff treats this shape as the new baseline. `None` for an
    /// ordinary publish-time snapshot.
    pub squashed_before: Option<String>,
    pub fields: Vec<SnapshotField>,
}

impl SchemaSnapshot {
    /// Serialise to the lockfile-style text format.
    pub fn write(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("schema_version = {}\n", self.schema_version));
        out.push_str(&format!("type = {}\n", self.type_name));
        out.push_str(&format!("published_version = {}\n", self.published_version));
        if let Some(before) = &self.squashed_before {
            out.push_str(&format!("squashed_before = {}\n", before));
        }
        for f in &self.fields {
            out.push_str(&format!("field {}: {}\n", f.name, f.ty));
        }
        out
    }

    /// Parse from the lockfile-style text format.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut schema_version: Option<u32> = None;
        let mut type_name: Option<String> = None;
        let mut published_version: Option<String> = None;
        let mut squashed_before: Option<String> = None;
        let mut fields = Vec::new();

        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("schema_version = ") {
                schema_version = Some(
                    rest.parse()
                        .map_err(|_| format!("invalid schema_version: {}", rest))?,
                );
            } else if let Some(rest) = line.strip_prefix("type = ") {
                type_name = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("published_version = ") {
                published_version = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("squashed_before = ") {
                squashed_before = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("field ") {
                // "field name: TypeStr"
                let colon = rest
                    .find(':')
                    .ok_or_else(|| format!("malformed field line: {}", line))?;
                let name = rest[..colon].trim().to_string();
                let ty = rest[colon + 1..].trim().to_string();
                fields.push(SnapshotField { name, ty });
            } else {
                return Err(format!("unknown line in schema snapshot: {}", line));
            }
        }

        Ok(SchemaSnapshot {
            schema_version: schema_version.ok_or("missing schema_version")?,
            type_name: type_name.ok_or("missing type")?,
            published_version: published_version.ok_or("missing published_version")?,
            squashed_before,
            fields,
        })
    }
}

/// Build a snapshot from a `StructDef` at a given version string.
pub fn snapshot_from_struct(s: &StructDef, version: &str) -> SchemaSnapshot {
    SchemaSnapshot {
        schema_version: SNAPSHOT_VERSION,
        type_name: s.name.clone(),
        published_version: version.to_string(),
        squashed_before: None,
        fields: s
            .fields
            .iter()
            .map(|f| SnapshotField {
                name: f.name.clone(),
                ty: f.ty.name(),
            })
            .collect(),
    }
}

/// Load a snapshot from disk. Checks `JET_SCHEMA_CACHE_DIR` env var first
/// (for tests), then `<project_root>/<SOURCE_ROOT_DIR>/<SCHEMA_CACHE_SUBDIR>/`.
pub fn load_snapshot(project_root: &std::path::Path, type_name: &str) -> Option<SchemaSnapshot> {
    let base = if let Ok(override_dir) = std::env::var("JET_SCHEMA_CACHE_DIR") {
        std::path::PathBuf::from(override_dir)
    } else {
        project_root
            .join(Syntax::SOURCE_ROOT_DIR)
            .join(Syntax::SCHEMA_CACHE_SUBDIR)
    };
    let path = base.join(format!("{}.snapshot", type_name));
    let raw = std::fs::read_to_string(&path).ok()?;
    SchemaSnapshot::parse(&raw).ok()
}

/// Write a snapshot to disk under `<project_root>/.jet/cache/schema/`.
pub fn save_snapshot(project_root: &std::path::Path, snap: &SchemaSnapshot) -> Result<(), String> {
    let dir = project_root
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(Syntax::SCHEMA_CACHE_SUBDIR);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create schema cache dir: {}", e))?;
    let path = dir.join(format!("{}.snapshot", snap.type_name));
    std::fs::write(&path, snap.write())
        .map_err(|e| format!("could not write schema snapshot: {}", e))
}

/// The schema cache directory for a project (`<root>/.jet/cache/schema/`),
/// honouring the `JET_SCHEMA_CACHE_DIR` test override.
pub fn schema_cache_dir(project_root: &std::path::Path) -> std::path::PathBuf {
    if let Ok(override_dir) = std::env::var("JET_SCHEMA_CACHE_DIR") {
        std::path::PathBuf::from(override_dir)
    } else {
        project_root
            .join(Syntax::SOURCE_ROOT_DIR)
            .join(Syntax::SCHEMA_CACHE_SUBDIR)
    }
}

/// Load every `<Type>.snapshot` in the project's schema cache, sorted by type
/// name. Returns `(type_name, snapshot)` pairs. Used by `jet inspect schema status`.
pub fn load_all_snapshots(project_root: &std::path::Path) -> Vec<SchemaSnapshot> {
    let dir = schema_cache_dir(project_root);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("snapshot") {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(snap) = SchemaSnapshot::parse(&raw) {
                    out.push(snap);
                }
            }
        }
    }
    out.sort_by(|a, b| a.type_name.cmp(&b.type_name));
    out
}

// ──────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snap(type_name: &str, version: &str, fields: &[(&str, &str)]) -> SchemaSnapshot {
        SchemaSnapshot {
            schema_version: SNAPSHOT_VERSION,
            type_name: type_name.to_string(),
            published_version: version.to_string(),
            squashed_before: None,
            fields: fields
                .iter()
                .map(|(n, t)| SnapshotField {
                    name: n.to_string(),
                    ty: t.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn schema_round_trip() {
        let snap = make_snap("UserRecord", "1.2.0", &[("name", "String"), ("age", "Int")]);
        let text = snap.write();
        let parsed = SchemaSnapshot::parse(&text).expect("should parse");
        assert_eq!(parsed, snap);
    }

    #[test]
    fn schema_parse_known_text() {
        let text = "schema_version = 1\ntype = Foo\npublished_version = 0.1.0\nfield x: Int\n";
        let snap = SchemaSnapshot::parse(text).unwrap();
        assert_eq!(snap.type_name, "Foo");
        assert_eq!(snap.published_version, "0.1.0");
        assert_eq!(snap.fields.len(), 1);
        assert_eq!(snap.fields[0].name, "x");
        assert_eq!(snap.fields[0].ty, "Int");
    }

    #[test]
    fn schema_diff_detects_removed_field() {
        let old = make_snap("T", "1.0.0", &[("name", "String"), ("age", "Int")]);
        let new = make_snap("T", "1.1.0", &[("display_name", "String"), ("age", "Int")]);
        let removed: Vec<_> = old
            .fields
            .iter()
            .filter(|f| !new.fields.iter().any(|nf| nf.name == f.name))
            .collect();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].name, "name");
    }

    #[test]
    fn schema_rename_with_type_match_unblocks() {
        let old = make_snap("T", "1.0.0", &[("name", "String")]);
        let new_fields = vec![SnapshotField {
            name: "display_name".to_string(),
            ty: "String".to_string(),
        }];
        // with rename name -> display_name, type matches → unblocked
        let old_field = old.fields.iter().find(|f| f.name == "name").unwrap();
        let new_field = new_fields
            .iter()
            .find(|f| f.name == "display_name")
            .unwrap();
        assert_eq!(
            old_field.ty, new_field.ty,
            "type must match for rename to unblock"
        );
    }

    #[test]
    fn schema_rename_with_type_mismatch_stays_blocked() {
        let old = make_snap("T", "1.0.0", &[("name", "String")]);
        let new_field = SnapshotField {
            name: "display_name".to_string(),
            ty: "Int".to_string(),
        };
        let old_field = old.fields.iter().find(|f| f.name == "name").unwrap();
        assert_ne!(
            old_field.ty, new_field.ty,
            "type mismatch — rename does not unblock"
        );
    }

    #[test]
    fn schema_empty_snapshot_parses() {
        let text = "schema_version = 1\ntype = Empty\npublished_version = 1.0.0\n";
        let snap = SchemaSnapshot::parse(text).unwrap();
        assert!(snap.fields.is_empty());
    }

    #[test]
    fn schema_squashed_before_round_trips() {
        // D-MIGRATE2C: `squashed_before` survives write→parse.
        let mut snap = make_snap("Rec", "2.0.0", &[("name", "String")]);
        snap.squashed_before = Some("2.0.0".to_string());
        let text = snap.write();
        assert!(text.contains("squashed_before = 2.0.0\n"), "got:\n{text}");
        let parsed = SchemaSnapshot::parse(&text).unwrap();
        assert_eq!(parsed.squashed_before.as_deref(), Some("2.0.0"));
        assert_eq!(parsed, snap);
    }

    #[test]
    fn schema_no_squash_omits_line() {
        let snap = make_snap("Rec", "1.0.0", &[("name", "String")]);
        let text = snap.write();
        assert!(
            !text.contains("squashed_before"),
            "ordinary snapshot must omit the line"
        );
        assert_eq!(SchemaSnapshot::parse(&text).unwrap().squashed_before, None);
    }

    /// Guard: `snapshot_from_struct` must write canonical type names (e.g. "String", not
    /// "String (text)"). If the wrong method is used, the snapshot format will diverge from
    /// what `load_snapshot` expects and the diff pass will silently mis-compare types.
    #[test]
    fn snapshot_from_struct_writes_canonical_type_names() {
        use crate::Diagnostics::Span;
        use crate::AST::{Field, StructDef, Type};
        let zero = Span::new(0, 0);
        let s = StructDef {
            span: zero,
            is_pub: false,
            is_package_pub: false,
            name: "Rec".to_string(),
            name_span: zero,
            type_params: vec![],
            fields: vec![
                Field {
                    is_pub: false,
                    is_package_pub: false,
                    name: "label".to_string(),
                    name_span: zero,
                    ty: Type::String,
                    ty_span: zero,
                    serde_markers: Vec::new(),
                    redact: false,
                    computed: None,
                    default: None,
            default_ct: None,
                },
                Field {
                    is_pub: false,
                    is_package_pub: false,
                    name: "count".to_string(),
                    name_span: zero,
                    ty: Type::Int,
                    ty_span: zero,
                    serde_markers: Vec::new(),
                    redact: false,
                    computed: None,
                    default: None,
            default_ct: None,
                },
            ],
            methods: vec![],
            trait_impls: vec![],
            derives: vec![],
            auto_derive_default: true,
            is_published_schema: true,
            published_schema_span: None,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            layout: None,
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
            validate_block: Vec::new(),
            validate_span: None,
        };
        let snap = snapshot_from_struct(&s, "2.0.0");
        let text = snap.write();
        // The canonical names must appear verbatim in the snapshot text.
        assert!(
            text.contains("field label: String\n"),
            "expected `field label: String` in snapshot, got:\n{text}"
        );
        assert!(
            text.contains("field count: Int\n"),
            "expected `field count: Int` in snapshot, got:\n{text}"
        );
        // Must also round-trip cleanly.
        let parsed = SchemaSnapshot::parse(&text).unwrap();
        assert_eq!(parsed.fields[0].ty, "String");
        assert_eq!(parsed.fields[1].ty, "Int");
    }
}
