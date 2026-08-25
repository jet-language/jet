//! Jetpack migration coverage reports.
//!
//! The inventory is explicit. A report never treats an observed package as
//! coverage without a denominator, so nativeization progress cannot become a
//! false green from counting only realized objects.

use crate::JSON;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const SCHEMA: &str = "jetpack-migration-coverage-v1";

/// One package identity in the migration inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoveragePackage {
    pub name: String,
    pub version: String,
    pub reference: String,
}

impl CoveragePackage {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        reference: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            reference: reference.into(),
        }
    }

    fn key(&self) -> String {
        format!("{}\0{}\0{}", self.name, self.version, self.reference)
    }

    fn validate(&self) -> Result<(), CoverageError> {
        for (field, value) in [
            ("name", self.name.as_str()),
            ("reference", self.reference.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(CoverageError::EmptyField {
                    field,
                    package: self.name.clone(),
                });
            }
        }
        Ok(())
    }
}

/// The four package buckets. `NotYetImported` is computed from the explicit
/// inventory; callers cannot put a package there by assertion alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageBucket {
    NativeRecipe,
    CentrallyImportedRecipe,
    LocalImport,
    NotYetImported,
}

impl CoverageBucket {
    pub fn label(self) -> &'static str {
        match self {
            Self::NativeRecipe => "native recipe",
            Self::CentrallyImportedRecipe => "centrally imported recipe",
            Self::LocalImport => "local import",
            Self::NotYetImported => "not-yet-imported",
        }
    }
}

/// Inputs supplied by a catalog producer, local importer, and native recipe
/// registry. Every listed package must also occur in `inventory`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageInput {
    pub inventory: Vec<CoveragePackage>,
    pub native_recipes: Vec<CoveragePackage>,
    pub centrally_imported_recipes: Vec<CoveragePackage>,
    pub local_imports: Vec<CoveragePackage>,
}

/// Nativeization counts over the complete inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeizationProgress {
    pub total: usize,
    pub nativeized: usize,
    pub remaining: usize,
    /// Whole percentage, rounded down for deterministic CI output.
    pub percent: u8,
}

/// A deterministic, disjoint migration coverage report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    pub native_recipes: Vec<CoveragePackage>,
    pub centrally_imported_recipes: Vec<CoveragePackage>,
    pub local_imports: Vec<CoveragePackage>,
    pub not_yet_imported: Vec<CoveragePackage>,
    pub nativeization: NativeizationProgress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageError {
    EmptyField {
        field: &'static str,
        package: String,
    },
    DuplicateInventory(String),
    DuplicateBucket {
        bucket: CoverageBucket,
        package: String,
    },
    PackageOutsideInventory {
        bucket: CoverageBucket,
        package: String,
    },
    PackageInMultipleBuckets {
        package: String,
        first: CoverageBucket,
        second: CoverageBucket,
    },
}

impl fmt::Display for CoverageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField { field, package } => {
                write!(f, "coverage package `{package}` has empty `{field}`")
            }
            Self::DuplicateInventory(package) => {
                write!(f, "coverage inventory repeats `{package}`")
            }
            Self::DuplicateBucket { bucket, package } => {
                write!(f, "{} bucket repeats `{package}`", bucket.label())
            }
            Self::PackageOutsideInventory { bucket, package } => write!(
                f,
                "{} bucket contains package outside inventory `{package}`",
                bucket.label()
            ),
            Self::PackageInMultipleBuckets {
                package,
                first,
                second,
            } => write!(
                f,
                "package `{package}` occurs in both {} and {} buckets",
                first.label(),
                second.label()
            ),
        }
    }
}

impl std::error::Error for CoverageError {}

impl CoverageReport {
    pub fn from_input(input: CoverageInput) -> Result<Self, CoverageError> {
        let inventory = unique_packages(input.inventory, None)?;
        let inventory_keys = inventory.keys().cloned().collect::<BTreeSet<_>>();
        let mut assigned = BTreeMap::<String, CoverageBucket>::new();

        let native_recipes = assign_bucket(
            input.native_recipes,
            CoverageBucket::NativeRecipe,
            &inventory_keys,
            &mut assigned,
        )?;
        let centrally_imported_recipes = assign_bucket(
            input.centrally_imported_recipes,
            CoverageBucket::CentrallyImportedRecipe,
            &inventory_keys,
            &mut assigned,
        )?;
        let local_imports = assign_bucket(
            input.local_imports,
            CoverageBucket::LocalImport,
            &inventory_keys,
            &mut assigned,
        )?;
        let not_yet_imported = inventory
            .into_iter()
            .filter_map(|(key, package)| (!assigned.contains_key(&key)).then_some(package))
            .collect::<Vec<_>>();
        let total = native_recipes.len()
            + centrally_imported_recipes.len()
            + local_imports.len()
            + not_yet_imported.len();
        let nativeized = native_recipes.len();
        let percent = if total == 0 {
            0
        } else {
            ((nativeized as u128 * 100) / total as u128) as u8
        };

        Ok(Self {
            native_recipes,
            centrally_imported_recipes,
            local_imports,
            not_yet_imported,
            nativeization: NativeizationProgress {
                total,
                nativeized,
                remaining: total - nativeized,
                percent,
            },
        })
    }

    /// Stable machine-readable report for CI and migration dashboards.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\":{},\"native_recipes\":{},\"centrally_imported_recipes\":{},\"local_imports\":{},\"not_yet_imported\":{},\"nativeization\":{{\"total\":{},\"nativeized\":{},\"remaining\":{},\"percent\":{}}}}}",
            JSON::quote(SCHEMA),
            packages_json(&self.native_recipes),
            packages_json(&self.centrally_imported_recipes),
            packages_json(&self.local_imports),
            packages_json(&self.not_yet_imported),
            self.nativeization.total,
            self.nativeization.nativeized,
            self.nativeization.remaining,
            self.nativeization.percent,
        )
    }

    /// Compact human-readable projection of the same report.
    pub fn to_human(&self) -> String {
        format!(
            "native recipes: {}\ncentrally imported recipes: {}\nlocal imports: {}\nnot-yet-imported packages: {}\nnativeization: {}/{} ({}%)\n",
            self.native_recipes.len(),
            self.centrally_imported_recipes.len(),
            self.local_imports.len(),
            self.not_yet_imported.len(),
            self.nativeization.nativeized,
            self.nativeization.total,
            self.nativeization.percent,
        )
    }
}

fn unique_packages(
    packages: Vec<CoveragePackage>,
    bucket: Option<CoverageBucket>,
) -> Result<BTreeMap<String, CoveragePackage>, CoverageError> {
    let mut unique = BTreeMap::new();
    for package in packages {
        package.validate()?;
        let key = package.key();
        if unique.insert(key, package.clone()).is_some() {
            return match bucket {
                Some(bucket) => Err(CoverageError::DuplicateBucket {
                    bucket,
                    package: package.name,
                }),
                None => Err(CoverageError::DuplicateInventory(package.name)),
            };
        }
    }
    Ok(unique)
}

fn assign_bucket(
    packages: Vec<CoveragePackage>,
    bucket: CoverageBucket,
    inventory: &BTreeSet<String>,
    assigned: &mut BTreeMap<String, CoverageBucket>,
) -> Result<Vec<CoveragePackage>, CoverageError> {
    let packages = unique_packages(packages, Some(bucket))?;
    let mut output = Vec::with_capacity(packages.len());
    for (key, package) in packages {
        if !inventory.contains(&key) {
            return Err(CoverageError::PackageOutsideInventory {
                bucket,
                package: package.name,
            });
        }
        if let Some(first) = assigned.insert(key, bucket) {
            return Err(CoverageError::PackageInMultipleBuckets {
                package: package.name,
                first,
                second: bucket,
            });
        }
        output.push(package);
    }
    Ok(output)
}

fn packages_json(packages: &[CoveragePackage]) -> String {
    let values = packages
        .iter()
        .map(|package| {
            format!(
                "{{\"name\":{},\"version\":{},\"reference\":{}}}",
                JSON::quote(&package.name),
                JSON::quote(&package.version),
                JSON::quote(&package.reference),
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

#[cfg(test)]
mod tests {
    use super::{CoverageError, CoverageInput, CoveragePackage, CoverageReport};

    fn package(name: &str) -> CoveragePackage {
        CoveragePackage::new(name, "1.0.0", format!("{name}@jetpack"))
    }

    #[test]
    fn coverage_report_separates_sources_and_measures_nativeization() {
        let report = CoverageReport::from_input(CoverageInput {
            inventory: vec![
                package("omp"),
                package("ripgrep"),
                package("fd"),
                package("jq"),
            ],
            native_recipes: vec![package("omp")],
            centrally_imported_recipes: vec![package("ripgrep")],
            local_imports: vec![package("fd")],
        })
        .expect("valid migration coverage");

        assert_eq!(report.native_recipes.len(), 1);
        assert_eq!(report.centrally_imported_recipes.len(), 1);
        assert_eq!(report.local_imports.len(), 1);
        assert_eq!(report.not_yet_imported, vec![package("jq")]);
        assert_eq!(report.nativeization.total, 4);
        assert_eq!(report.nativeization.nativeized, 1);
        assert_eq!(report.nativeization.remaining, 3);
        assert_eq!(report.nativeization.percent, 25);
        assert_eq!(
            report.to_human(),
            "native recipes: 1\ncentrally imported recipes: 1\nlocal imports: 1\nnot-yet-imported packages: 1\nnativeization: 1/4 (25%)\n"
        );
        assert!(report.to_json().contains("\"not_yet_imported\":[{"));
        assert!(report.to_json().contains("\"nativeization\":{\"total\":4"));
    }

    #[test]
    fn coverage_report_rejects_unknown_and_overlapping_packages() {
        let unknown = CoverageReport::from_input(CoverageInput {
            inventory: vec![package("ripgrep")],
            native_recipes: vec![package("fd")],
            ..CoverageInput::default()
        })
        .expect_err("unknown package must not inflate coverage");
        assert!(matches!(
            unknown,
            CoverageError::PackageOutsideInventory { .. }
        ));

        let overlap = CoverageReport::from_input(CoverageInput {
            inventory: vec![package("ripgrep")],
            native_recipes: vec![package("ripgrep")],
            centrally_imported_recipes: vec![package("ripgrep")],
            ..CoverageInput::default()
        })
        .expect_err("one package must have one source bucket");
        assert!(matches!(
            overlap,
            CoverageError::PackageInMultipleBuckets { .. }
        ));
    }
}
