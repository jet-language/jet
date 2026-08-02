//! Jetpack-private integration for the pure native evaluator seam.
//!
//! The product entry is intentionally bounded: it evaluates only the
//! non-executing, typed devShell projection. The pinned Stage A fixture keeps
//! its values, rejection, lock, and output identities reproducible.

#![allow(dead_code)] // B-F consume this seam as they land; product use stays forbidden.

use jet_nix_eval::{BoundaryError, DevShellEvaluation, ValidatedOracleManifest};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(in crate::NixEval) struct NativeBoundary {
    manifest: ValidatedOracleManifest,
}

impl NativeBoundary {
    pub(in crate::NixEval) fn embedded() -> Result<Self, BoundaryError> {
        Ok(Self {
            manifest: ValidatedOracleManifest::embedded()?,
        })
    }

    pub(in crate::NixEval) fn product_ready(&self) -> bool {
        self.manifest.product_ready()
    }

    pub(in crate::NixEval) fn evaluate_devshell(
        &self,
        source: &str,
        system: &str,
    ) -> Result<DevShellEvaluation, BoundaryError> {
        jet_nix_eval::evaluate_devshell(source, system)
            .map_err(|error| BoundaryError::Evaluation(error.to_string()))
    }

    pub(in crate::NixEval) fn evaluate_devshell_with_import_authority(
        &self,
        source: &str,
        system: &str,
        import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
    ) -> Result<DevShellEvaluation, BoundaryError> {
        jet_nix_eval::evaluate_devshell_with_import_authority(
            source,
            system,
            import_authority,
        )
        .map_err(|error| BoundaryError::Evaluation(error.to_string()))
    }

    pub(in crate::NixEval) fn evaluator_identity(
        &self,
        system: &str,
    ) -> Result<String, BoundaryError> {
        self.manifest.evaluator_identity(system)
    }
}
