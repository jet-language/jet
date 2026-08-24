//! Jetpack — Jet's package manager engine and Phase 1 CLI (D-JPK*).
//!
//! Phase 1 is the Nix-`shell`/`devenv`-class temporary environment:
//! `jetpack run <package>@<source>` resolves a ref through a provider, realizes
//! it into the Jetpack store, composes an env, and drops the user into a pretty
//! subshell that `exit` leaves cleanly. Jetpack owns the package lifecycle; Nix
//! is a compatibility provider (D-JPK5).
//!
//! Built std-only (I6) and independent from the `jet` binary (D-JPK1). The
//! consolidated plan lives in `docs/plans/epoch-5/README.md`.

#![allow(non_snake_case)]
#![deny(warnings)]

pub use jet_codegen::{
    Codegen, Comptime, Diagnostics, Lexer, Parser, Sema, Syntax, AST, SHA256,
};

// Card #367 / D-PRODUCT-SPLIT1=C: the read-only package/config data model
// (manifest/lock/store-listing/ref/FFI-binding/script-dep parsing), plus the
// pure effect-budget/lint-policy computation slice 3 also moved out (neither
// touches network/provider/shell), lives in `jet-pkg-model` so `jet-driver`
// (and now `jet` itself) can depend on it without pulling in this crate's
// provider/network/shell engine. Re-exported under their historical paths so
// every internal call site in this crate (`crate::Package`,
// `super::RefSpec`, `crate::EffectBudget`, etc.) is unchanged.
pub use jet_pkg_model::{
    AdaBind, CBind, CFFI, ComBind, CppBind, DartBind, DotNetBind, EffectBudget, Envelope, FFI, FortranBind,
    JavaBind, JetLib, JSON, LintPolicy, Lock, Manifest, Package, PascalBind, Platform,
    PowerShellBind, RefSpec, ScriptDeps, TclBind, Variant,
};
pub use jet_pkg_model::ProviderFacts::{
    ProviderConflict, ProviderFactValue, ProviderFacts, ProviderLoss, ProviderSelector,
};
// Card #367 slice 5: WorkspacePlan/WorkspaceMember + WorkspaceLock read path
// now live in jet-pkg-model (L1). WorkspaceFile eval lives in jet-env-model
// (L2). jetpack re-exports under the historical module paths via the thin
// shims WorkspaceFile.rs and WorkspaceLock.rs in this crate.

pub mod Bridge;
pub mod BrowserLock;
pub mod BuildDebug;
pub mod CLI;
pub mod Components;
pub mod Discovery;
pub mod Doctor;
pub mod EnvFile;
pub mod EnvFiles;
pub mod EnvHook;
pub mod Foreign;
pub mod Image;
pub mod JetOS;
pub mod JetPin;
pub mod MemberSelect;
pub mod MigrationImport;
pub(crate) mod NixIndex;
#[cfg(feature = "test-seam")]
pub mod test_nix_index {
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::BTreeMap;

    pub use crate::NixIndex::NixIndexError;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TestIndexRecord {
        pub attrpath: Vec<String>,
        pub version: String,
        pub drv_path: String,
        pub outputs: BTreeMap<String, String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TestSignedIndex {
        pub index_bytes: Vec<u8>,
        pub index_signature: Vec<u8>,
        pub manifest_bytes: Vec<u8>,
        pub manifest_signature: Vec<u8>,
        pub target_url: String,
        pub target_signature_url: String,
        pub public_key: [u8; 32],
    }

    pub fn signed(
        seed: [u8; 32],
        key_id: &str,
        endpoint: &str,
        channel: &str,
        revision: &str,
        system: &str,
        released_unix: u64,
        generation: u64,
        issued_unix: u64,
        expires_unix: u64,
        records: Vec<TestIndexRecord>,
        not_indexed: Vec<(Vec<String>, String)>,
    ) -> Result<TestSignedIndex, NixIndexError> {
        let signing_key = SigningKey::from_bytes(&seed);
        let record_count = records.len() as u64;
        let records = records
            .into_iter()
            .map(|record| crate::NixIndex::IndexRecord {
                attrpath: record.attrpath,
                version: record.version,
                drv_path: record.drv_path,
                outputs: record.outputs,
            })
            .collect::<Vec<_>>();
        let (decoded, compressed) = crate::NixIndex::canonical_test_index(
            channel,
            revision,
            system,
            released_unix,
            records,
            not_indexed,
        )?;
        let index_signature = signing_key
            .sign(&crate::NixIndex::producer_signature_request(&decoded))
            .to_bytes();
        let index_signature = crate::NixIndex::signature_sidecar_for_test(key_id, &index_signature);
        let digest = crate::SHA256::sha256_hex(&compressed);
        let target_url = format!("{endpoint}/index-v1/{revision}/{system}/{digest}.json.zst");
        let target_signature_url = format!("{target_url}.sig.json");
        let target = crate::NixIndex::index_target_for_test(
            revision,
            system,
            endpoint,
            &compressed,
            &decoded,
            &index_signature,
            record_count,
        )?;
        let manifest_bytes = crate::NixIndex::canonical_manifest_for_test(
            channel,
            generation,
            issued_unix,
            expires_unix,
            vec![target],
        )?;
        let manifest_signature = signing_key
            .sign(&crate::NixIndex::manifest_signature_request(
                &manifest_bytes,
            ))
            .to_bytes();
        let manifest_signature =
            crate::NixIndex::signature_sidecar_for_test(key_id, &manifest_signature);
        Ok(TestSignedIndex {
            index_bytes: compressed,
            index_signature,
            manifest_bytes,
            manifest_signature,
            target_url,
            target_signature_url,
            public_key: signing_key.verifying_key().to_bytes(),
        })
    }

    pub fn sign(seed: [u8; 32], bytes: &[u8]) -> Vec<u8> {
        SigningKey::from_bytes(&seed)
            .sign(bytes)
            .to_bytes()
            .to_vec()
    }

    pub fn public_key(seed: [u8; 32]) -> [u8; 32] {
        SigningKey::from_bytes(&seed).verifying_key().to_bytes()
    }
}
// Card #367 slice 4: `ModuleEval` (the computed-modules evaluator + plan
// types) now lives in `jet-env-model` (L2, pure eval) — both realizers,
// jetpack's env-runtime and JetOS realization, name it directly
// (`jet_env_model::ModuleEval`) instead of sharing it by living in the same
// crate. No re-export here; that was the step-2 shim, now dropped.
/// E4-JP8 — native Nix `.drv` / path calculus (internal compat surface).
pub mod NixDrv;
/// E4-JP9 — native evaluator internals and the bounded typed devShell entry.
pub(crate) mod NixEval;
pub mod Output;
pub mod Overlay;
pub mod PackageGraph;
pub mod Provider;
pub mod ProviderGraph;
pub mod Recipe;
/// E4-JP7 — package-facing view of the canonical remote builder scheduler.
pub mod Remote;
pub mod Replacement;
pub mod RuntimePolicy;
pub mod ScriptLock;
pub mod Secrets;
pub mod SemanticLock;
pub mod Services;
pub mod Shell;
pub mod Store;
pub mod Toolchain;
pub mod Trust;
pub mod TrustRoot;
pub mod Transition;
pub mod WorkspaceFile;
pub mod WorkspaceLock;

/// Process entry point used by the `jetpack` binary.
pub fn run(args: Vec<String>) -> i32 {
    CLI::main(args)
}
