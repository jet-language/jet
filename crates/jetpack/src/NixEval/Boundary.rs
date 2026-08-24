//! Jetpack-private integration for the pure native evaluator seam.
//!
//! The product entry is intentionally bounded: it evaluates the
//! non-executing, typed devShell projection and the private derivation
//! materializer. The pinned Stage A fixtures keep values, rejection, lock,
//! and output identities reproducible.

#![allow(dead_code)] // B-F consume this seam as they land; product use stays forbidden.

use jet_nix_eval::{
    BoundaryError, DerivationEvaluation, DevShellEvaluation, ValidatedOracleManifest,
};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(in crate::NixEval) struct NativeBoundary {
    manifest: ValidatedOracleManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeDerivationEvaluation {
    name: String,
    drv_path: String,
    outputs: BTreeMap<String, String>,
    output_specs: BTreeMap<String, (String, String)>,
    builder: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    input_sources: BTreeSet<String>,
}

impl NativeDerivationEvaluation {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn drv_path(&self) -> &str {
        &self.drv_path
    }

    pub(crate) fn outputs(&self) -> &BTreeMap<String, String> {
        &self.outputs
    }

    pub(crate) fn output_specs(&self) -> &BTreeMap<String, (String, String)> {
        &self.output_specs
    }

    pub(crate) fn builder(&self) -> &str {
        &self.builder
    }

    pub(crate) fn args(&self) -> &[String] {
        &self.args
    }

    pub(crate) fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    pub(crate) fn input_sources(&self) -> &BTreeSet<String> {
        &self.input_sources
    }
}

impl NativeBoundary {
    pub(in crate::NixEval) fn embedded() -> Result<Self, BoundaryError> {
        let manifest = ValidatedOracleManifest::embedded()?;
        if !manifest.product_ready() {
            return Err(BoundaryError::Manifest(
                "pinned Nix oracle is not product-ready".to_string(),
            ));
        }
        Ok(Self { manifest })
    }

    pub(in crate::NixEval) fn product_ready(&self) -> bool {
        self.manifest.product_ready()
    }

    pub(in crate::NixEval) fn pinned_inventory(&self) -> &'static [jet_nix_eval::InventoryEntry] {
        jet_nix_eval::pinned_inventory()
    }

    pub(in crate::NixEval) fn evaluate_devshell(
        &self,
        source: &str,
        system: &str,
    ) -> Result<DevShellEvaluation, BoundaryError> {
        self.evaluate_devshell_output(source, system, "default")
    }

    pub(in crate::NixEval) fn evaluate_devshell_output(
        &self,
        source: &str,
        system: &str,
        output: &str,
    ) -> Result<DevShellEvaluation, BoundaryError> {
        jet_nix_eval::evaluate_devshell_output(source, system, output)
            .map_err(|error| BoundaryError::Evaluation(error.to_string()))
    }

    pub(in crate::NixEval) fn evaluate_devshell_with_import_authority(
        &self,
        source: &str,
        system: &str,
        import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
    ) -> Result<DevShellEvaluation, BoundaryError> {
        self.evaluate_devshell_output_with_import_authority(
            source,
            system,
            "default",
            import_authority,
        )
    }

    pub(in crate::NixEval) fn evaluate_devshell_output_with_import_authority(
        &self,
        source: &str,
        system: &str,
        output: &str,
        import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
    ) -> Result<DevShellEvaluation, BoundaryError> {
        jet_nix_eval::evaluate_devshell_output_with_import_authority(
            source,
            system,
            output,
            import_authority,
        )
        .map_err(|error| BoundaryError::Evaluation(error.to_string()))
    }

    pub(in crate::NixEval) fn evaluate_derivation(
        &self,
        source: &str,
        system: &str,
    ) -> Result<NativeDerivationEvaluation, BoundaryError> {
        let evaluation = jet_nix_eval::evaluate_derivation(source, system)
            .map_err(|error| BoundaryError::Evaluation(error.to_string()))?;
        materialize_derivation(&evaluation)
            .map_err(|error| BoundaryError::Evaluation(error.to_string()))
    }

    pub(in crate::NixEval) fn evaluate_derivation_with_import_authority(
        &self,
        source: &str,
        system: &str,
        import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
    ) -> Result<NativeDerivationEvaluation, BoundaryError> {
        let evaluation = jet_nix_eval::evaluate_derivation_with_import_authority(
            source,
            system,
            import_authority,
        )
        .map_err(|error| BoundaryError::Evaluation(error.to_string()))?;
        materialize_derivation(&evaluation)
            .map_err(|error| BoundaryError::Evaluation(error.to_string()))
    }

    pub(in crate::NixEval) fn evaluate_derivation_output(
        &self,
        source: &str,
        system: &str,
        attribute: &str,
    ) -> Result<NativeDerivationEvaluation, BoundaryError> {
        let evaluation = jet_nix_eval::evaluate_derivation_output(source, system, attribute)
            .map_err(|error| BoundaryError::Evaluation(error.to_string()))?;
        materialize_derivation(&evaluation)
            .map_err(|error| BoundaryError::Evaluation(error.to_string()))
    }

    pub(in crate::NixEval) fn evaluate_derivation_output_with_import_authority(
        &self,
        source: &str,
        system: &str,
        attribute: &str,
        import_authority: Option<Rc<dyn Fn(&str) -> Result<String, String>>>,
    ) -> Result<NativeDerivationEvaluation, BoundaryError> {
        let evaluation = jet_nix_eval::evaluate_derivation_output_with_import_authority(
            source,
            system,
            attribute,
            import_authority,
        )
        .map_err(|error| BoundaryError::Evaluation(error.to_string()))?;
        materialize_derivation(&evaluation)
            .map_err(|error| BoundaryError::Evaluation(error.to_string()))
    }

    pub(in crate::NixEval) fn evaluator_identity(
        &self,
        system: &str,
    ) -> Result<String, BoundaryError> {
        self.manifest.evaluator_identity(system)
    }
}

fn materialize_derivation(
    evaluation: &DerivationEvaluation,
) -> Result<NativeDerivationEvaluation, crate::NixDrv::NixDrvError> {
    let store_dir = crate::NixDrv::DEFAULT_STORE_DIR;
    let mut outputs = BTreeMap::new();
    for output in evaluation.outputs() {
        let _ = outputs.insert(
            output.name().to_string(),
            crate::NixDrv::DerivationOutput {
                name: output.name().to_string(),
                path: String::new(),
                method_algo: output.method_algo().to_string(),
                hash_hex: output.hash_hex().to_string(),
            },
        );
    }
    let mut input_srcs = BTreeSet::new();
    input_srcs.extend(evaluation.input_sources().iter().cloned());
    let mut drv = crate::NixDrv::Derivation {
        outputs,
        input_drvs: Vec::new(),
        input_srcs: input_srcs.clone(),
        platform: evaluation.system().to_string(),
        builder: evaluation.builder().to_string(),
        args: evaluation.args().to_vec(),
        env: evaluation.env().clone(),
    };

    let placeholder = format!("{store_dir}/jet-native-evaluator.drv");
    let mut store = crate::NixDrv::MapDrvStore {
        map: BTreeMap::new(),
    };
    let mut memo = BTreeMap::new();
    let modulo = crate::NixDrv::hash_derivation_modulo(
        &mut store,
        store_dir,
        &placeholder,
        &drv,
        true,
        &mut memo,
    )?;
    for (name, output) in &mut drv.outputs {
        output.path = if output.is_fixed() {
            crate::NixDrv::make_fixed_output_path(
                store_dir,
                &crate::NixDrv::output_path_name(evaluation.name(), name),
                &output.method_algo,
                &output.hash_hex,
            )
        } else {
            let hash = modulo.get(name).ok_or_else(|| {
                crate::NixDrv::NixDrvError::Path(format!(
                    "missing derivation modulo hash for output {name}"
                ))
            })?;
            crate::NixDrv::make_output_path(store_dir, name, hash, evaluation.name())
        };
    }
    for (name, output) in &drv.outputs {
        let _ = drv.env.insert(name.clone(), output.path.clone());
    }
    let aterm = crate::NixDrv::unparse_derive(&drv, false, None);
    let drv_path = crate::NixDrv::make_text_path(
        store_dir,
        &format!("{}.drv", evaluation.name()),
        aterm.as_bytes(),
        &drv.references_for_drv_path(),
    );
    crate::NixDrv::verify_drv_path(store_dir, &drv_path, aterm.as_bytes(), &drv)?;
    crate::NixDrv::verify_output_paths(&mut store, store_dir, &drv_path, &drv)?;
    let output_specs = drv
        .outputs
        .iter()
        .map(|(name, output)| {
            (
                name.clone(),
                (output.method_algo.clone(), output.hash_hex.clone()),
            )
        })
        .collect();
    let outputs = drv
        .outputs
        .iter()
        .map(|(name, output)| (name.clone(), output.path.clone()))
        .collect();
    Ok(NativeDerivationEvaluation {
        name: evaluation.name().to_string(),
        drv_path,
        outputs,
        output_specs,
        builder: evaluation.builder().to_string(),
        args: evaluation.args().to_vec(),
        env: drv.env,
        input_sources: input_srcs,
    })
}
