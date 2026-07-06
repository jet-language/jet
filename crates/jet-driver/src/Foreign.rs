//! D-FFI-UNIFY1 shared binder registry.
//!
//! This is the narrow dispatch spine all foreign-language binders hang from.
//! C is active today; rust/py/js/swift are registered as ratified namespace
//! mounts so later cards add binder depth without inventing a second namespace
//! model. The shipped S50 `extern rust` block stays on its existing path until
//! the `rust.*` binder migrates it under D-FFI-UNIFY1.

use crate::Syntax;
use crate::AST::ForeignLanguage;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderSurface {
    /// `use <lang>.<lib>` / `#Extern module <lang>.<lib>` / generated cache.
    Namespace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderStatus {
    Active,
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinderDescriptor {
    pub language: ForeignLanguage,
    pub surface: BinderSurface,
    pub status: BinderStatus,
}

pub const BINDERS: &[BinderDescriptor] = &[
    BinderDescriptor {
        language: ForeignLanguage::C,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
    },
    BinderDescriptor {
        language: ForeignLanguage::Rust,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Planned,
    },
    BinderDescriptor {
        language: ForeignLanguage::Py,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Planned,
    },
    BinderDescriptor {
        language: ForeignLanguage::Js,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Planned,
    },
    BinderDescriptor {
        language: ForeignLanguage::Swift,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Planned,
    },
];

pub fn binder_for(language: ForeignLanguage) -> Option<&'static BinderDescriptor> {
    BINDERS.iter().find(|b| b.language == language)
}

pub fn binding_cache_dir(project_root: &Path, language: ForeignLanguage) -> PathBuf {
    project_root
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
}

pub fn binding_cache_file(project_root: &Path, language: ForeignLanguage, lib: &str) -> PathBuf {
    binding_cache_dir(project_root, language).join(format!("{}.{}", lib, Syntax::FILE_EXT))
}
