// core.game asset imports are a typed boundary over the shared asset pipeline.
//
// Hosts provide root identities, watch events, source identities, and checked
// importer outcomes. This adapter never observes the filesystem, hashes bytes,
// infers an importer, or invents a cache key. The shared Prelude owns every
// validation, normalization, dependency, and report rule.

#[derive(Clone, Debug)]
pub(crate) struct GameAssetImportAdapter {
    roots: JetGameAssetRootSet,
    graph: JetGameAssetDependencyGraph,
}

impl GameAssetImportAdapter {
    pub(crate) fn new(roots: JetGameAssetRootSet) -> Self {
        let graph = JetGameAssetDependencyGraph::new(roots.clone());
        Self { roots, graph }
    }

    pub(crate) fn with_roots(
        roots: Vec<JetGameAssetRootIdentity>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self::new(JetGameAssetRootSet::new(roots)?))
    }

    pub(crate) fn roots(&self) -> &JetGameAssetRootSet {
        &self.roots
    }

    pub(crate) fn graph(&self) -> &JetGameAssetDependencyGraph {
        &self.graph
    }

    pub(crate) fn graph_mut(&mut self) -> &mut JetGameAssetDependencyGraph {
        &mut self.graph
    }

    pub(crate) fn watch_facts(
        &self,
        events: &[JetGameAssetWatchEvent],
    ) -> Result<JetGameAssetWatchFacts, JetGameAssetPipelineError> {
        JetGameAssetWatchFacts::coalesce(&self.roots, events)
    }

    pub(crate) fn add_source(
        &mut self,
        source: JetGameAssetSourceIdentity,
    ) -> Result<JetGameAssetNodeIdentity, JetGameAssetPipelineError> {
        self.graph.add_source(source)
    }

    pub(crate) fn set_dependencies(
        &mut self,
        source: &JetGameAssetSourceIdentity,
        dependencies: Vec<JetGameAssetDependencyIdentity>,
    ) -> Result<(), JetGameAssetPipelineError> {
        self.graph.set_dependencies(source, dependencies)
    }

    pub(crate) fn import_plan(
        &self,
        source: JetGameAssetSourceIdentity,
        recipe: JetGameAssetRecipeIdentity,
        tool: JetGameAssetToolIdentity,
        version: JetGameAssetVersionIdentity,
        dependencies: Vec<JetGameAssetDependencyIdentity>,
    ) -> Result<JetGameAssetImportPlan, JetGameAssetPipelineError> {
        JetGameAssetImportPlan::new(source, recipe, tool, version)?.with_dependencies(dependencies)
    }

    pub(crate) fn ready_outcome(
        &self,
        plan: &JetGameAssetImportPlan,
        output_hash: impl Into<String>,
    ) -> Result<JetGameAssetImportOutcome, JetGameAssetPipelineError> {
        JetGameAssetImportOutcome::ready(plan, output_hash)
    }
    pub(crate) fn runtime_fact(
        &self,
        plan: &JetGameAssetImportPlan,
        outcome: &JetGameAssetImportOutcome,
        bytes: Vec<u8>,
        metadata: impl Into<String>,
    ) -> Result<JetGameAssetRuntimeFact, JetGameAssetPipelineError> {
        JetGameAssetRuntimeFact::from_ready(plan, outcome, bytes, metadata)
    }


    pub(crate) fn failed_outcome(
        &self,
        plan: &JetGameAssetImportPlan,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<JetGameAssetImportOutcome, JetGameAssetPipelineError> {
        JetGameAssetImportOutcome::failed(plan, code, message)
    }

    pub(crate) fn report_from_watch(
        &self,
        events: &[JetGameAssetWatchEvent],
    ) -> Result<JetGameAssetImportReport, JetGameAssetPipelineError> {
        Ok(JetGameAssetImportReport::from_watch(&self.watch_facts(events)?))
    }
    pub(crate) fn import_report(
        &self,
        facts: &JetGameAssetWatchFacts,
    ) -> JetGameAssetImportReport {
        let mut report = JetGameAssetImportReport::from_watch(facts);
        let imported = report.imported.clone();
        for node in imported.iter().cloned() {
            for dependency in self.graph.dependencies_for(&node) {
                report.record_dependency(dependency);
            }
        }

        // The report is a projection of the declared graph, not a directory
        // scan. A registered source untouched by this watch batch is an
        // explicit skip, which lets hosts distinguish "not considered" from
        // "considered and unchanged" without inventing a second status store.
        let imported = imported.into_iter().collect::<std::collections::BTreeSet<_>>();
        let deleted = report
            .deleted
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let sources = self.graph.nodes().cloned().collect::<Vec<_>>();
        for node in sources {
            if self.graph.source(&node).is_some()
                && !imported.contains(&node)
                && !deleted.contains(&node)
            {
                report.record_skipped(node);
            }
        }
        report
    }
}
