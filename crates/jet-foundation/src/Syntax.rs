//! OWNER-CONTROLLED SURFACE.
//!
//! Every keyword, sigil, and built-in name a user can type lives in this
//! file and nowhere else (invariant I7). Each constant maps to a decision
//! ID in docs/spec/syntax-decisions.md. Changing a provisional choice means:
//! change it here, update docs/spec/syntax-decisions.md, re-bless the ui snapshots. Done.
//!
//! Agents: do NOT add an entry here without a decision ID approved by the
//! owner in docs/spec/syntax-decisions.md.
// Marker-plane reconciliation anchors: MARKER_PUB_FILE, ATTR_TARGET,
// ATTR_LAYOUT, ATTR_CODABLE, CONTRACT_MARKERS, KW_CAPS, KW_GRANT,
// KW_COMPTIME, KW_DERIVE, ATTR_TRACK. Constants live in the split include
// files below; keep this root file mentioning them so I7 audits can check one
// canonical surface entrypoint.

include!("Syntax/core_surface.rs");
include!("Syntax/math_layout.rs");
include!("Syntax/effects_tests.rs");
include!("Syntax/jetpack_config.rs");
include!("Syntax/package_files.rs");
include!("Syntax/markers.rs");
include!("Syntax/highlights.rs");
include!("Syntax/predicates.rs");
