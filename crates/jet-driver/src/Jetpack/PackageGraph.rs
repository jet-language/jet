//! Epoch 4 package visibility core (D-WD3).
//!
//! User-facing catalog syntax is still gated. This module is the strict graph
//! substrate: packages see declared deps only, plus catalog edges already
//! selected by a caller-owned surface.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageNode {
    pub name: String,
    pub direct_deps: Vec<String>,
    pub transitive_deps: Vec<String>,
}

impl PackageNode {
    pub fn new(name: impl Into<String>) -> PackageNode {
        PackageNode {
            name: name.into(),
            direct_deps: Vec::new(),
            transitive_deps: Vec::new(),
        }
    }

    pub fn with_deps(mut self, deps: &[&str]) -> PackageNode {
        self.direct_deps = deps.iter().map(|d| d.to_string()).collect();
        self
    }

    pub fn with_transitives(mut self, deps: &[&str]) -> PackageNode {
        self.transitive_deps = deps.iter().map(|d| d.to_string()).collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub logical_name: String,
    pub provider_ref: String,
    pub version_rule: String,
    pub allowed_packages: Vec<String>,
    pub owner_workspace: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibleEdgeKind {
    DirectDep,
    Catalog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleEdge {
    pub owner_package: String,
    pub dependency: String,
    pub provider_ref: String,
    pub kind: VisibleEdgeKind,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingDependency {
    pub owner_package: String,
    pub requested: String,
    pub reason: String,
    pub fix: MissingDependencyFix,
}

impl MissingDependency {
    pub fn fix_text(&self) -> String {
        match &self.fix {
            MissingDependencyFix::DirectAddPath { dep, path } => {
                format!("run `jet add {dep} --path {path}`")
            }
            MissingDependencyFix::DirectAddGit { dep, url, tag } => {
                format!("run `jet add {dep} --git {url} --tag {tag}`")
            }
            MissingDependencyFix::CatalogData {
                logical_name,
                packages,
            } => format!(
                "add catalog data for `{logical_name}` and allow packages: {}",
                packages.join(", ")
            ),
            MissingDependencyFix::DisambiguateMember { candidates } => {
                format!("choose one workspace member: {}", candidates.join(", "))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingDependencyFix {
    DirectAddPath {
        dep: String,
        path: String,
    },
    DirectAddGit {
        dep: String,
        url: String,
        tag: String,
    },
    CatalogData {
        logical_name: String,
        packages: Vec<String>,
    },
    DisambiguateMember {
        candidates: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageGraph {
    packages: BTreeMap<String, PackageNode>,
    catalogs: Vec<CatalogEntry>,
    workspace_members: BTreeMap<String, Vec<String>>,
}

impl PackageGraph {
    pub fn new() -> PackageGraph {
        PackageGraph::default()
    }

    pub fn add_package(&mut self, pkg: PackageNode) {
        self.packages.insert(pkg.name.clone(), pkg);
    }

    pub fn add_catalog(&mut self, entry: CatalogEntry) {
        self.catalogs.push(entry);
    }

    pub fn add_workspace_member(&mut self, name: impl Into<String>, path: impl Into<String>) {
        self.workspace_members
            .entry(name.into())
            .or_default()
            .push(path.into());
    }

    pub fn visible_edges(&self, owner_package: &str) -> Vec<VisibleEdge> {
        let mut out = Vec::new();
        if let Some(pkg) = self.packages.get(owner_package) {
            for dep in &pkg.direct_deps {
                out.push(VisibleEdge {
                    owner_package: owner_package.to_string(),
                    dependency: dep.clone(),
                    provider_ref: dep.clone(),
                    kind: VisibleEdgeKind::DirectDep,
                    rationale: "declared direct dependency".to_string(),
                });
            }
        }
        for cat in &self.catalogs {
            if cat.allowed_packages.iter().any(|p| p == owner_package) {
                out.push(VisibleEdge {
                    owner_package: owner_package.to_string(),
                    dependency: cat.logical_name.clone(),
                    provider_ref: cat.provider_ref.clone(),
                    kind: VisibleEdgeKind::Catalog,
                    rationale: cat.rationale.clone(),
                });
            }
        }
        out.sort_by(|a, b| a.dependency.cmp(&b.dependency));
        out
    }

    pub fn check_visible(
        &self,
        owner_package: &str,
        requested: &str,
    ) -> Result<VisibleEdge, MissingDependency> {
        if let Some(edge) = self
            .visible_edges(owner_package)
            .into_iter()
            .find(|edge| edge.dependency == requested)
        {
            return Ok(edge);
        }

        if let Some(candidates) = self.workspace_members.get(requested) {
            if candidates.len() > 1 {
                return Err(MissingDependency {
                    owner_package: owner_package.to_string(),
                    requested: requested.to_string(),
                    reason: format!(
                        "`{owner_package}` asked for `{requested}`, but that name is ambiguous"
                    ),
                    fix: MissingDependencyFix::DisambiguateMember {
                        candidates: candidates.clone(),
                    },
                });
            }
        }

        let packages_using = self.packages_using_hidden_dep(requested);
        let fix = if packages_using.len() > 1 {
            MissingDependencyFix::CatalogData {
                logical_name: requested.to_string(),
                packages: packages_using,
            }
        } else {
            MissingDependencyFix::DirectAddPath {
                dep: requested.to_string(),
                path: format!("../{requested}"),
            }
        };

        Err(MissingDependency {
            owner_package: owner_package.to_string(),
            requested: requested.to_string(),
            reason: format!(
                "`{owner_package}` can use only direct deps or selected catalog deps; `{requested}` is not visible"
            ),
            fix,
        })
    }

    fn packages_using_hidden_dep(&self, dep: &str) -> Vec<String> {
        let mut packages = BTreeSet::new();
        for pkg in self.packages.values() {
            if pkg.transitive_deps.iter().any(|d| d == dep) {
                packages.insert(pkg.name.clone());
            }
        }
        packages.into_iter().collect()
    }
}
