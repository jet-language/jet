//! Native Nix evaluator internals (E4-JP9 / D-JPK-NIXENGINE1=D).
//!
//! The bounded devShell projection is a product path. Arbitrary evaluator
//! stages remain private until their own differential proof lands.

mod Boundary;

#[cfg(test)]
mod Tests;

pub(crate) fn evaluate_devshell(
    source: &str,
    system: &str,
) -> Result<jet_nix_eval::DevShellEvaluation, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluate_devshell(source, system)
}

pub(crate) fn evaluator_identity(
    system: &str,
) -> Result<String, jet_nix_eval::BoundaryError> {
    Boundary::NativeBoundary::embedded()?.evaluator_identity(system)
}
