// core.game asset runtime state is the shared checked replacement store.
//
// This adapter owns no cache map, lock, revision counter, validation, or
// rollback policy. It only presents the CoreLib boundary while the canonical
// Prelude store owns atomic publication and receipts.

#[derive(Clone, Debug)]
pub(crate) struct GameAssetRuntimeAdapter {
    store: JetGameAssetStore,
}

impl GameAssetRuntimeAdapter {
    pub(crate) fn new() -> Self {
        Self {
            store: JetGameAssetStore::new(),
        }
    }

    pub(crate) fn with_roots(
        roots: JetGameAssetRootSet,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self {
            store: JetGameAssetStore::with_roots(roots)?,
        })
    }

    pub(crate) fn revision(&self) -> u64 {
        self.store.revision()
    }

    pub(crate) fn ready(
        &self,
        source: &JetGameAssetSourceIdentity,
    ) -> Option<JetGameAssetArtifactIdentity> {
        self.store.ready(source)
    }

    pub(crate) fn ready_for_node(
        &self,
        node: &JetGameAssetNodeIdentity,
    ) -> Option<JetGameAssetArtifactIdentity> {
        self.store.ready_for_node(node)
    }

    pub(crate) fn ready_assets(&self) -> Vec<JetGameAssetArtifactIdentity> {
        self.store.ready_assets()
    }

    pub(crate) fn latest(
        &self,
        source: &JetGameAssetSourceIdentity,
    ) -> Option<JetGameAssetReloadReceipt> {
        self.store.latest(source)
    }

    pub(crate) fn begin_transaction(&self) -> JetGameAssetReloadTransaction {
        self.store.begin_transaction()
    }

    pub(crate) fn begin_at(&self, revision: u64) -> JetGameAssetReloadTransaction {
        self.store.begin_at(revision)
    }

    pub(crate) fn apply(
        &self,
        plan: JetGameAssetImportPlan,
        outcome: JetGameAssetImportOutcome,
    ) -> JetGameAssetTransactionReceipt {
        self.store.apply(plan, outcome)
    }

    pub(crate) fn apply_at(
        &self,
        revision: u64,
        plan: JetGameAssetImportPlan,
        outcome: JetGameAssetImportOutcome,
    ) -> JetGameAssetTransactionReceipt {
        self.store.apply_at(revision, plan, outcome)
    }
    pub(crate) fn apply_transaction(
        &self,
        entries: Vec<(JetGameAssetImportPlan, JetGameAssetImportOutcome)>,
    ) -> Result<JetGameAssetTransactionReceipt, String> {
        let mut transaction = self.store.begin_transaction();
        for (plan, outcome) in entries {
            if let Err(error) = transaction.stage(plan, outcome) {
                let _ = transaction.rollback();
                return Err(error.to_string());
            }
        }
        Ok(transaction.commit())
    }


    pub(crate) fn store(&self) -> &JetGameAssetStore {
        &self.store
    }
}
