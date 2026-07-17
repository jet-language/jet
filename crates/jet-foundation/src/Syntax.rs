//! OWNER-CONTROLLED SURFACE.
//!
//! Every keyword, sigil, and built-in name a user can type lives in this
//! file and nowhere else (invariant I7). Each constant maps to a decision
//! ID in docs/spec/syntax-decisions.md. Changing a provisional choice means:
//! change it here, update docs/spec/syntax-decisions.md, re-bless the ui snapshots. Done.
//!
//! Agents: do NOT add an entry here without a decision ID approved by the
//! owner in docs/spec/syntax-decisions.md.
// Marker-plane reconciliation anchors: MARKER_PUB_FILE, MARKER_NO_PRELUDE, ATTR_TARGET,
// ATTR_LAYOUT, ATTR_CODABLE, CONTRACT_MARKERS, KW_CAPS, KW_GRANT,
// KW_COMPTIME, KW_DERIVE, ATTR_TRACK. Constants live in the private modules
// below; keep this root file mentioning them so I7 audits can check one
// canonical surface entrypoint.
//
// D-SHAPE-CLI1 reuses the existing `fn run` / `@Cli` surface: a resolved
// entry-parameter type owns typed shell inputs, while zero-parameter `fn run()`
// stays valid. D-SHAPE6 adds no Jet source token; grouped tool commands remain
// owned by the single registry in crates/jet-cli/src/CLI.rs.
// D-ECO-DECL1=A adds no spelling: ecosystem entries reuse ordinary named
// fields and D-DOTCTOR1 `Type.{ ... }` construction. D-ECO-ROOTNAME1 still
// owns the root noun; #560 owns executable source and tooling behavior.
// D-MEM-VIEWRET1=B adds no token, sigil, lifetime spelling, or grammar rule.
// It reuses the existing named-type spellings `View`, `ViewMut`, and the
// restricted `str` element spelling at public string-view boundaries; sema
// infers and publishes their owner provenance.

mod core_surface;
pub use core_surface::*;
mod math_layout;
pub use math_layout::*;
mod effects_surface;
pub use effects_surface::*;
mod jetpack_config;
pub use jetpack_config::*;
mod package_files;
pub use package_files::*;
mod markers;
pub use markers::*;
mod highlights;
pub use highlights::*;
mod predicates;
pub use predicates::*;
