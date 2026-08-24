//! Native Nix evaluator internals (E4-JP9 / D-JPK-NIXENGINE1=D).
//!
//! The bounded devShell and derivation projections are product paths. Filesystem
//! imports use a private project-root authority; unsupported evaluator stages
//! remain private and are recorded as explicit inventory skips.

mod Authority;
mod Boundary;
/// E4-JP10 — native execution of an evaluated derivation.
#[path = "../NixBuilder.rs"]
pub(crate) mod NixBuilder;

pub(crate) use Authority::ProjectImportAuthority;
pub(crate) use Boundary::NativeDerivationEvaluation;

use std::rc::Rc;

#[cfg(test)]
mod Tests;

#[allow(dead_code)]
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

pub(crate) fn evaluate_devshell_output_with_import_authority(
    source: &str,
    system: &str,
    output: &str,
    import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
) -> Result<jet_nix_eval::DevShellEvaluation, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluate_devshell_output_with_import_authority(
        source,
        system,
        output,
        import_authority,
    )
}

pub(crate) fn evaluator_identity(system: &str) -> Result<String, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluator_identity(system)
}

pub(crate) fn pinned_inventory(
) -> Result<&'static [jet_nix_eval::InventoryEntry], jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded().map(|boundary| boundary.pinned_inventory())
}

#[allow(dead_code)] // The private evaluator stages consume this seam in order.
pub(crate) fn evaluate_derivation(
    source: &str,
    system: &str,
) -> Result<NativeDerivationEvaluation, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluate_derivation(source, system)
}

#[allow(dead_code)]
pub(crate) fn evaluate_derivation_with_import_authority(
    source: &str,
    system: &str,
    import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
) -> Result<NativeDerivationEvaluation, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluate_derivation_with_import_authority(
        source,
        system,
        import_authority,
    )
}

#[allow(dead_code)]
pub(crate) fn evaluate_derivation_output(
    source: &str,
    system: &str,
    attribute: &str,
) -> Result<NativeDerivationEvaluation, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluate_derivation_output(source, system, attribute)
}

pub(crate) fn evaluate_derivation_output_with_import_authority(
    source: &str,
    system: &str,
    attribute: &str,
    import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
) -> Result<NativeDerivationEvaluation, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluate_derivation_output_with_import_authority(
        source,
        system,
        attribute,
        import_authority,
    )
}
