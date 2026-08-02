//! Native Nix evaluator internals (E4-JP9 / D-JPK-NIXENGINE1=D).
//!
//! The bounded devShell projection is a product path. Filesystem imports use a
//! private project-root authority; arbitrary evaluator stages remain private
//! until their own differential proof lands.

mod Boundary;
mod Authority;

pub(crate) use Authority::ProjectImportAuthority;

use std::rc::Rc;

#[cfg(test)]
mod Tests;

pub(crate) fn evaluate_devshell_with_import_authority(
    source: &str,
    system: &str,
    import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
) -> Result<jet_nix_eval::DevShellEvaluation, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluate_devshell_with_import_authority(
        source,
        system,
        import_authority,
    )
}

pub(crate) fn evaluator_identity(
    system: &str,
) -> Result<String, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluator_identity(system)
}
