//! The Jetpack ownable component kit (Tower c134 Phase 4).
//!
//! `jetpack add <Component>` copies a real, editable `.jet` source file into
//! the project's `components/` dir instead of installing an opaque package —
//! the shadcn/ui model, adapted for Jet. The canonical source for each
//! starter component lives on disk under `Jetpack/components/*.jet` (real,
//! independently readable/lintable/runnable Jet files — see each file's own
//! `main()`) and is embedded into the binary at compile time via
//! `include_str!`, the same technique the Rust prelude uses
//! (`crates/jet-codegen/src/Prelude/CoreLib.rs`).
//!
//! This is a small, fixed catalog (Button/Label/Input/Container per the
//! reactive-ui-stack plan) — not a package registry. Growing the kit means
//! adding another `.jet` file here and one entry in `STARTER_COMPONENTS`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One starter component: its catalog name (matched case-sensitively against
/// `jetpack add <name>`) and its embedded `.jet` source.
pub struct StarterComponent {
    pub name: &'static str,
    pub source: &'static str,
}

pub const STARTER_COMPONENTS: &[StarterComponent] = &[
    StarterComponent {
        name: "Button",
        source: include_str!("components/Button.jet"),
    },
    StarterComponent {
        name: "Label",
        source: include_str!("components/Label.jet"),
    },
    StarterComponent {
        name: "Input",
        source: include_str!("components/Input.jet"),
    },
    StarterComponent {
        name: "Container",
        source: include_str!("components/Container.jet"),
    },
];

/// Look up a starter component by its exact catalog name. Case-sensitive: a
/// lowercase `button` does not match `Button` — it falls through to ordinary
/// `<source>:<package>` ref classification instead of quietly guessing.
pub fn find(name: &str) -> Option<&'static StarterComponent> {
    STARTER_COMPONENTS.iter().find(|c| c.name == name)
}

/// The directory components are copied into, relative to a project root.
pub const COMPONENTS_DIR: &str = "components";

#[derive(Debug)]
pub enum ComponentError {
    Io(io::Error),
    /// The destination file already exists. Never silently clobbered — it
    /// may be a user's already-customized copy.
    AlreadyExists(PathBuf),
}

impl std::fmt::Display for ComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentError::Io(e) => write!(f, "{e}"),
            ComponentError::AlreadyExists(path) => {
                write!(f, "{} already exists", path.display())
            }
        }
    }
}

impl From<io::Error> for ComponentError {
    fn from(e: io::Error) -> Self {
        ComponentError::Io(e)
    }
}

/// Copy `component`'s embedded source into `<dir>/components/<name>.jet`.
/// Creates the `components/` dir if absent. Refuses to overwrite an existing
/// file — once copied, the file is the user's; re-running `add` on a name
/// they already own is a no-op error, not a silent reset of their edits.
pub fn add_component(dir: &Path, component: &StarterComponent) -> Result<PathBuf, ComponentError> {
    let components_dir = dir.join(COMPONENTS_DIR);
    fs::create_dir_all(&components_dir)?;
    let dest = components_dir.join(format!("{}.jet", component.name));
    if dest.exists() {
        return Err(ComponentError::AlreadyExists(dest));
    }
    fs::write(&dest, component.source)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jetpack_components_test_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_known_components_case_sensitively() {
        assert!(find("Button").is_some());
        assert!(find("Label").is_some());
        assert!(find("Input").is_some());
        assert!(find("Container").is_some());
        assert!(find("button").is_none());
        assert!(find("nixpkgs").is_none());
    }

    #[test]
    fn starter_components_use_the_canonical_ui_tree() {
        for component in STARTER_COMPONENTS {
            assert!(
                component.source.contains("use core.ui as ui"),
                "{} must import the canonical UI module",
                component.name
            );
            assert!(
                component.source.contains("UiNode"),
                "{} must expose canonical UiNode output",
                component.name
            );
            assert!(
                !component.source.contains("enum View"),
                "{} must not introduce a private render tree",
                component.name
            );
        }
    }

    #[test]
    fn copies_component_source_into_components_dir() {
        let dir = scratch_dir("copy");
        let button = find("Button").unwrap();
        let dest = add_component(&dir, button).expect("copy should succeed");
        assert_eq!(dest, dir.join("components").join("Button.jet"));
        let written = fs::read_to_string(&dest).unwrap();
        assert_eq!(written, button.source);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_to_clobber_an_existing_file() {
        let dir = scratch_dir("clobber");
        let button = find("Button").unwrap();
        add_component(&dir, button).expect("first copy should succeed");
        // Simulate a user edit.
        let dest = dir.join("components").join("Button.jet");
        fs::write(&dest, "// user-edited\n").unwrap();
        let err = add_component(&dir, button).expect_err("second copy should refuse");
        assert!(matches!(err, ComponentError::AlreadyExists(_)));
        assert_eq!(fs::read_to_string(&dest).unwrap(), "// user-edited\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
