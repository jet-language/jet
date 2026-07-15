//! Jetpack-private integration for the pure native evaluator seam.
//!
//! Partial stages require a permit whose witness exists only in unit tests.
//! JP11 must add a distinct verified product entry point after corpus parity.

#![allow(dead_code)] // B-F consume this seam as they land; product use stays forbidden.

pub(in crate::NixEval) use jet_nix_eval::{BoundaryError, ValidatedOracleManifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::NixEval) enum InternalStage {
    Syntax,
    Values,
    Evaluation,
    Authority,
    Derivation,
    Flakes,
}

#[derive(Debug)]
pub(in crate::NixEval) struct InternalTestHarness {
    private: (),
}

impl InternalTestHarness {
    pub(in crate::NixEval) fn engine(&self) -> &'static str {
        "native-jetpack"
    }
}

#[derive(Debug)]
pub(in crate::NixEval) struct InternalStagePermit {
    stage: InternalStage,
}

impl InternalStagePermit {
    pub(in crate::NixEval) fn stage(&self) -> InternalStage {
        self.stage
    }
}

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

    #[cfg(test)]
    pub(in crate::NixEval) fn internal_test_harness(&self) -> InternalTestHarness {
        InternalTestHarness { private: () }
    }

    pub(in crate::NixEval) fn authorize_internal(
        &self,
        _harness: &InternalTestHarness,
        stage: InternalStage,
    ) -> InternalStagePermit {
        InternalStagePermit { stage }
    }

    pub(in crate::NixEval) fn product_ready(&self) -> bool {
        self.manifest.product_ready()
    }
}
