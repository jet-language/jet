//! Jetpack-private integration for the pure native evaluator seam.
//!
//! The pure seam owns partial-stage permits and exposes no evaluation entry
//! point. JP11 must add a distinct verified product entry after corpus parity.

#![allow(dead_code)] // B-F consume this seam as they land; product use stays forbidden.

use jet_nix_eval::{BoundaryError, ValidatedOracleManifest};

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
}
