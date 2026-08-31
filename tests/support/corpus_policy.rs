//! Semantic ratchet for the maintained Jet corpus.
//!
//! This is deliberately a test-support adapter. It is not a compiler lint and
//! it is never consulted for source outside the checked-in corpus manifest.
//! The manifest owns classification and provenance; the AST pass owns the
//! recurrence rules; the domain tests decide when to call this adapter.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use jet::AST::{
    AccessConvention, BinOp, BindPattern, CallArg, Expr, ForKind, Item, LambdaBody, OrFallback,
    Pattern, Program, Stmt, StrFormat, StrPart, StructPatField, Type, UnOp,
};
use jet::Diagnostics::Span;

pub const MANIFEST_PATH: &str = "tests/corpus_policy.tsv";
const MANIFEST: &str = include_str!("../corpus_policy.tsv");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceRole {
    CanonicalTeaching,
    ExpertLesson,
    NegativeDiagnostic,
    RawProtocol,
    FixtureHarnessBoundary,
    GeneratedOutput,
    NonTeachingTestData,
}

impl SourceRole {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "canonical-teaching" => Ok(Self::CanonicalTeaching),
            "expert-lesson" => Ok(Self::ExpertLesson),
            "negative-diagnostic" => Ok(Self::NegativeDiagnostic),
            "raw-protocol" => Ok(Self::RawProtocol),
            "fixture-harness-boundary" => Ok(Self::FixtureHarnessBoundary),
            "generated-output" => Ok(Self::GeneratedOutput),
            "non-teaching-test-data" => Ok(Self::NonTeachingTestData),
            _ => Err(format!("unknown corpus source role `{value}`")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRow {
    pub selector: String,
    pub role: SourceRole,
    pub profile: String,
    pub decision: String,
    pub proof: String,
    pub owner: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerRow {
    pub selector: String,
    pub artifact: String,
    pub protocol: String,
    pub owner: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRow {
    pub selector: String,
    pub producer: String,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingRow {
    pub id: String,
    pub rule: String,
    pub owner: String,
    pub proof: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionRow {
    pub rule: String,
    pub selector: String,
    pub site: String,
    pub expected: usize,
    pub protocol: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleScopeRow {
    pub rule: String,
    pub selector: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorpusManifest {
    pub sources: Vec<SourceRow>,
    pub scopes: Vec<RuleScopeRow>,
    pub producers: Vec<ProducerRow>,
    pub artifacts: Vec<ArtifactRow>,
    pub findings: Vec<FindingRow>,
    pub exceptions: Vec<ExceptionRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleSpec {
    pub name: &'static str,
    pub why: &'static str,
    pub replacement: &'static str,
}

/// The complete recurrence vocabulary. Every finding row must point at one of
/// these entries. Product-owned rules remain here even when their primary
/// executable proof lives in a compiler or domain test; this keeps the 38-row
/// audit ledger total and prevents an audit-only finding.
pub fn rule_registry() -> &'static [RuleSpec] {
    &[
        RuleSpec { name: "raw-cli-fixed-shape", why: "maintained beginner programs must use the typed entry shape", replacement: "declare a typed #CLI entry struct and fn run(args: Args)" },
        RuleSpec { name: "raw-cli-builder-shape", why: "a maintained CLI recipe must not rebuild the parser beside its typed entry", replacement: "use the resolved typed #CLI entry or the one approved args builder" },
        RuleSpec { name: "fixture-cli-role", why: "a diagnostic fixture may retain parser syntax only as an explicitly classified fixture", replacement: "keep the exact negative or fixture role and do not copy its shape into teaching code" },
        RuleSpec { name: "dogfood-stdlib-recipes", why: "the maintained dogfood recipe must use the canonical typed helper", replacement: "call the resolved Core helper instead of a local reimplementation" },
        RuleSpec { name: "dogfood-path-containment", why: "path containment must use the typed path relation", replacement: "call Path.is_within for the normalized path values" },
        RuleSpec { name: "dogfood-json", why: "dogfood wire values must use the typed JSON seam", replacement: "encode the typed report with the resolved JSON helper" },
        RuleSpec { name: "dogfood-url-query", why: "URL query semantics must stay in the URL parser", replacement: "use the resolved URL query API" },
        RuleSpec { name: "dogfood-list-equality", why: "typed lists already provide the canonical equality operation", replacement: "compare the lists with the resolved equality operation" },
        RuleSpec { name: "dogfood-directory-setup", why: "directory creation must be idempotent", replacement: "use the idempotent directory setup helper" },
        RuleSpec { name: "dogfood-ascii", why: "ASCII case folding must use the canonical ASCII helper", replacement: "call the resolved ASCII case-fold helper" },
        RuleSpec { name: "datatree-domain-policy", why: "Tower must not duplicate generic DataTree equality or truthiness machinery", replacement: "use DataTree.equal_unordered for mechanical equality and keep named Tower policy helpers" },
        RuleSpec { name: "dogfood-json-url-list", why: "dogfood JSON, URL, and list operations have one typed path", replacement: "use the typed JSON, URL, or list API" },
        RuleSpec { name: "raw-cli-process-boundary", why: "a canonical program must not repeatedly reconstruct the process boundary", replacement: "use the one typed entry boundary or one explicit expert boundary" },
        RuleSpec { name: "product-parser-regression", why: "the parser boundary must reject the recorded invalid shape", replacement: "keep the shared parser regression fixture and assertion" },
        RuleSpec { name: "entry-implicit", why: "the first-contact program should be its top-level body", replacement: "move the ordinary first-contact body to top level" },
        RuleSpec { name: "process_args_view", why: "the maintained corpus should use the canonical process argument view", replacement: "replace process.argv().skip(1) with process.args()" },
        RuleSpec { name: "message_text", why: "the maintained corpus should use the message-level HTTP text default", replacement: "replace body().text(default_limit) with text()" },
        RuleSpec { name: "redundant_fixed_cleanup", why: "the maintained corpus should keep plain Fixed output free of grouping cleanup", replacement: "delete the grouping-separator cleanup" },
        RuleSpec { name: "unit-scalar-rewrap", why: "a scalar conversion must not unwrap and immediately rewrap the same unit", replacement: "use the linear unit operation directly" },
        RuleSpec { name: "unit_scalar_rewrap", why: "the maintained corpus should use direct scalar unit arithmetic", replacement: "use unit * scalar, scalar * unit, or unit / scalar" },
        RuleSpec { name: "path_containment_string_prefix", why: "the maintained corpus should use typed path containment", replacement: "replace the string prefix guard with Path.is_within()" },
        RuleSpec { name: "complete_ascii_case_ladder", why: "the maintained corpus should use the direct ASCII case conversion", replacement: "replace the complete ladder with to_ascii_lower() or to_ascii_upper()" },
        RuleSpec { name: "compiler-loop-fact", why: "the compiler-owned loop fact must be proved by sema", replacement: "retain the typed positive and negative loop fixtures" },
        RuleSpec { name: "first-hour-doc-recipe", why: "the onboarding command must use the canonical runner", replacement: "run the example with `jet run`" },
        RuleSpec { name: "build-root-fact", why: "the selected build entry is a liveness root", replacement: "retain the build-root and neighboring-lint proof" },
        RuleSpec { name: "error-identity", why: "an unchanged fallible error should propagate through the entry", replacement: "use the bare fallible call" },
        RuleSpec { name: "readonly-copy", why: "a proven read-only argument does not need an explicit copy sigil", replacement: "pass the read-only expression without the copy sigil" },
        RuleSpec { name: "task-one-child", why: "one-child task groups add no scheduling policy", replacement: "call task.race or task.any directly" },
        RuleSpec { name: "effect-row-inference", why: "positive effects already inferred by the body are redundant", replacement: "remove the redundant positive effect row" },
        RuleSpec { name: "codable-structural", why: "structurally eligible records receive the canonical Codable behavior", replacement: "remove the bare #Codable marker" },
        RuleSpec { name: "duration-constant-safe", why: "constant durations should use the checked unit literal", replacement: "write the constant as a duration literal such as `2h`" },
        RuleSpec { name: "http-wrapper-json", why: "the maintained HTTP recipe should use the typed message and JSON seams", replacement: "use the message-text, typed-JSON, and request-free wrapper" },
        RuleSpec { name: "http-message-text", why: "reading an HTTP response as text should use the message-level default", replacement: "call the response message text projection" },
        RuleSpec { name: "http-typed-json", why: "HTTP JSON requests should use the typed request body seam", replacement: "call the typed JSON request helper" },
        RuleSpec { name: "http-unused-request", why: "a request parameter unused by a route should not be carried through the wrapper", replacement: "use the request-free route form" },
        RuleSpec { name: "crypto-naming-fact", why: "digest names must use the canonical typed surface", replacement: "use the typed digest API and its explicit projection" },
        RuleSpec { name: "indexed-sequence", why: "sequence loops can carry the index as a fact", replacement: "write `loop (index, item) in items`" },
        RuleSpec { name: "delimited-reader-config", why: "configured delimited input owns row boundaries and line numbers", replacement: "use the configured delimited reader" },
        RuleSpec { name: "walk_files_filter", why: "the maintained corpus should use file-only traversal", replacement: "replace fs.walk with fs.walk_files" },
        RuleSpec { name: "walk-files-filter", why: "file-only traversal should use the configured walk filter", replacement: "use the file-only walk helper or configured traversal" },
        RuleSpec { name: "plain-format-fact", why: "plain fixed formatting must not be followed by grouping cleanup", replacement: "use the plain Fixed format projection" },
        RuleSpec { name: "regex-capture-recipe", why: "the maintained filename recipe needs one anchored match and one capture", replacement: "match once with an anchored capture and use the captured value" },
        RuleSpec { name: "bindgen-one-decoder", why: "one binding renderer emits one response decoder per protocol", replacement: "route all generated response calls through the single decoder" },
        RuleSpec { name: "generated-protocol-override", why: "a generated protocol exception must be named and structurally distinct from the ordinary envelope", replacement: "declare an override identity with differs-from-ordinary-envelope proof" },
        RuleSpec { name: "fixture-role-classification", why: "fixture semantics are classified before teaching-surface rules run", replacement: "add the exact fixture role and occurrence exception to the manifest" },
        RuleSpec { name: "corpus-inventory", why: "every maintained source and generated producer needs an explicit manifest boundary", replacement: "add one exact manifest row with its role, owner, reason, and provenance" },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticViolation {
    pub file: String,
    pub rule: String,
    pub site: String,
    pub why: String,
    pub replacement: String,
}

impl SemanticViolation {
    pub fn new(file: &str, rule: &str, site: String) -> Self {
        let spec = rule_registry()
            .iter()
            .find(|spec| spec.name == rule)
            .unwrap_or_else(|| panic!("unregistered corpus rule: {rule}"));
        Self {
            file: file.to_string(),
            rule: rule.to_string(),
            site,
            why: spec.why.to_string(),
            replacement: spec.replacement.to_string(),
        }
    }
}

impl std::fmt::Display for SemanticViolation {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            out,
            "file={}; rule={}; site={}; why={}; replacement={}",
            self.file, self.rule, self.site, self.why, self.replacement
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryEntry {
    pub path: String,
    pub role: SourceRole,
    pub producer: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactInventory {
    pub files: Vec<InventoryEntry>,
    pub producers: Vec<String>,
    pub artifacts: Vec<String>,
    pub provenance: Vec<ProvenanceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceEntry {
    pub producer: String,
    pub artifact: String,
    pub protocol: String,
}

#[derive(Debug, Clone)]
pub struct CorpusPolicy {
    root: PathBuf,
    manifest: CorpusManifest,
}

impl CorpusPolicy {
    pub fn load() -> Result<Self, String> {
        Self::from_manifest(MANIFEST, PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    }

    pub fn from_manifest(text: &str, root: impl Into<PathBuf>) -> Result<Self, String> {
        let manifest = CorpusManifest::parse(text)?;
        manifest.validate()?;
        let root = root.into();
        validate_manifest_paths(&manifest, &root)?;
        Ok(Self { root, manifest })
    }

    pub fn manifest(&self) -> &CorpusManifest {
        &self.manifest
    }

    pub fn validate(&self) -> Result<(), String> {
        self.manifest.validate()
    }

    /// Discover every checked-in Jet source and every checked-in generated Jet
    /// artifact. A new path is not silently outside the policy: it must match
    /// one exact file or the most-specific manifest root.
    pub fn inventory(&self) -> Result<ArtifactInventory, String> {
        let mut paths = Vec::new();
        discover_manifest_files(&self.root, &self.manifest, &mut paths)?;
        paths.sort();
        paths.dedup();

        let mut errors = Vec::new();
        let mut files = Vec::new();
        for path in paths {
            let Some(row) = self.source_row(&path) else {
                errors.push(inventory_error(
                    "unclassified eligible source",
                    &path,
                    "inventory:source",
                ));
                continue;
            };
            let producer = self
                .manifest
                .artifacts
                .iter()
                .find(|artifact| artifact.selector == format!("path:{path}"))
                .map(|artifact| artifact.producer.clone());
            if row.role == SourceRole::GeneratedOutput && producer.is_none() {
                errors.push(inventory_error(
                    "generated source has no producer artifact provenance",
                    &path,
                    "inventory:provenance",
                ));
            }
            files.push(InventoryEntry {
                path,
                role: row.role,
                producer,
            });
        }

        for artifact in &self.manifest.artifacts {
            if let Some(path) = artifact.selector.strip_prefix("path:") {
                if !self.root.join(path).is_file() {
                    errors.push(format!("manifest artifact is missing: {path}"));
                }
            }
        }

        for artifact in &self.manifest.artifacts {
            let Some(path) = artifact.selector.strip_prefix("path:") else {
                continue;
            };
            if Path::new(path).extension().and_then(|ext| ext.to_str()) != Some("jet") {
                continue;
            }
            match self.source_row(path) {
                Some(row) if row.role == SourceRole::GeneratedOutput => {}
                Some(row) => errors.push(format!(
                    "generated artifact {path} is classified as {:?}, not generated-output",
                    row.role
                )),
                None => errors.push(format!(
                    "generated artifact has no source classification: {path}"
                )),
            }
        }

        let producers = self
            .manifest
            .producers
            .iter()
            .map(|producer| producer.selector.clone())
            .collect::<Vec<_>>();
        let artifacts = self
            .manifest
            .artifacts
            .iter()
            .map(|artifact| artifact.selector.clone())
            .collect::<Vec<_>>();
        let provenance = self
            .manifest
            .artifacts
            .iter()
            .filter_map(|artifact| {
                self.manifest
                    .producers
                    .iter()
                    .find(|producer| producer.selector == artifact.producer)
                    .map(|producer| ProvenanceEntry {
                        producer: producer.selector.clone(),
                        artifact: artifact.selector.clone(),
                        protocol: producer.protocol.clone(),
                    })
            })
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(ArtifactInventory {
                files,
                producers,
                artifacts,
                provenance,
            })
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Check an externally discovered inventory. Domain gates use this for
    /// producer families and tests use it to prove a synthetic path cannot
    /// enter the corpus without a manifest row.
    pub fn audit_inventory(&self, files: &[&str], producers: &[&str]) -> Result<(), String> {
        let mut errors = Vec::new();
        for &file in files {
            if self.source_row(file).is_none() {
                errors.push(inventory_error(
                    "unclassified eligible source",
                    file,
                    "inventory:source",
                ));
            }
        }
        let declared = self
            .manifest
            .producers
            .iter()
            .map(|producer| producer.selector.as_str())
            .collect::<BTreeSet<_>>();
        for &producer in producers {
            if !declared.contains(producer) {
                errors.push(inventory_error(
                    "unclassified generated-source producer",
                    producer,
                    "inventory:producer",
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Check the ratified CLI recipe inventory. This is a closed-corpus gate:
    /// only manifest rows with an explicit CLI profile participate. It never
    /// diagnoses arbitrary user programs or changes compiler behavior.
    pub fn check_cli_recipe_inventory(&self) -> Result<(), String> {
        self.validate()?;
        self.check_fixture_inventory()?;
        let inventory = self.inventory()?;
        let mut errors = Vec::new();
        let mut checked = BTreeSet::new();

        for entry in &inventory.files {
            let Some(row) = self.source_row(&entry.path) else {
                continue;
            };
            let Some(recipe) = cli_recipe_profile(&row.profile) else {
                continue;
            };
            let path = self.root.join(&entry.path);
            if !is_jet_program(&path) && recipe != CliRecipe::TypedDoc {
                continue;
            }
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("read {}: {error}", path.display()))?;
            let has_run_command = has_jet_run_command(&source);
            let sources = if Path::new(&entry.path).extension().and_then(|ext| ext.to_str()) == Some("md") {
                jet_fences(&source)
            } else {
                vec![source]
            };
            let programs = sources
                .iter()
                .map(|source| parse_program(source))
                .collect::<Result<Vec<_>, _>>()?;
            let valid = match recipe {
                CliRecipe::Typed => !programs.is_empty() && programs.iter().all(has_typed_cli_entry),
                CliRecipe::TypedDoc => {
                    programs.iter().any(has_typed_cli_entry) && has_run_command
                }
                CliRecipe::Builder => programs.iter().any(has_args_builder),
                CliRecipe::Raw => programs.iter().any(has_raw_process_boundary),
            };
            if !valid {
                errors.push(format!(
                    "CLI recipe profile `{}` is not proved by the AST for {}",
                    row.profile, entry.path
                ));
            }
            checked.insert(entry.path.clone());
        }

        if checked.is_empty() {
            errors.push("manifest has no checked CLI recipe profiles".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Check the role boundary before any teaching-surface rule runs. Fixture
    /// roots are intentionally allowed to contain syntax that teaching code
    /// must not copy, but the allowance is only valid when the manifest says
    /// so and the profile agrees with the role.
    pub fn check_fixture_inventory(&self) -> Result<(), String> {
        self.validate()?;
        let inventory = self.inventory()?;
        let mut errors = Vec::new();

        for entry in &inventory.files {
            let Some(expected) = fixture_role_for_path(&entry.path) else {
                continue;
            };
            if entry.role != expected {
                errors.push(
                    SemanticViolation::new(
                        &entry.path,
                        "fixture-role-classification",
                        site("inventory:fixture-role", Span::new(0, 0)),
                    )
                    .to_string(),
                );
            }
        }

        for row in &self.manifest.sources {
            let Some(expected) = profile_fixture_role(&row.profile) else {
                continue;
            };
            let compatible = match expected {
                ProfileFixtureRole::One(role) => row.role == role,
                ProfileFixtureRole::Generated => {
                    matches!(row.role, SourceRole::RawProtocol | SourceRole::GeneratedOutput)
                }
            };
            if !compatible {
                let path = selector_path(&row.selector);
                errors.push(
                    SemanticViolation::new(
                        path,
                        "fixture-cli-role",
                        site("manifest:fixture-profile", Span::new(0, 0)),
                    )
                    .to_string(),
                );
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Run the adapter after one domain gate. The domain gate remains the
    /// owner of execution, snapshots, or generator assertions; this call owns
    /// classification, semantic recurrence, and artifact provenance.
    pub fn check_gate(&self, gate: &str) -> Result<(), String> {
        self.validate()?;
        self.check_cli_recipe_inventory()?;
        let inventory = self.inventory()?;
        let mut violations = Vec::new();
        for entry in &inventory.files {
            let Some(row) = self.source_row(&entry.path) else {
                continue;
            };
            if row.role == SourceRole::GeneratedOutput
                || !proof_matches(&row.proof, gate)
                || !semantic_role(row.role)
            {
                continue;
            }
            let path = self.root.join(&entry.path);
            if path.file_name().and_then(|name| name.to_str()) == Some("package.jet") {
                continue;
            }
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("read {}: {error}", path.display()))?;
            violations.extend(self.evaluate_source(&entry.path, &source)?);
        }
        if gate == "all" || gate == "bindgen" {
            for artifact in &self.manifest.artifacts {
                let Some(path) = artifact.selector.strip_prefix("path:") else {
                    continue;
                };
                let Some(producer) = self.manifest.producers.iter().find(|producer| producer.selector == artifact.producer) else {
                    continue;
                };
                if is_no_response_protocol(&producer.protocol)
                    || Path::new(path).extension().and_then(|ext| ext.to_str()) != Some("jet")
                {
                    continue;
                }
                let source = fs::read_to_string(self.root.join(path))
                    .map_err(|error| format!("read generated artifact {path}: {error}"))?;
                violations.extend(self.evaluate_generated(&producer.selector, &source)?);
            }
        }
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }

    pub fn evaluate_source(
        &self,
        path: &str,
        source: &str,
    ) -> Result<Vec<SemanticViolation>, String> {
        let Some(row) = self.source_row(path) else {
            return Err(inventory_error(
                "unclassified eligible source",
                path,
                "inventory:source",
            ));
        };
        if !semantic_role(row.role) {
            return Ok(Vec::new());
        }
        let programs = if Path::new(path).extension().and_then(|ext| ext.to_str()) == Some("md") {
            jet_fences(source)
        } else {
            vec![source.to_string()]
        };
        let mut all = Vec::new();
        for program_source in programs {
            let program = parse_program(&program_source)?;
            self.validate_maintained_lint_allows(path, row, &program)?;
            all.extend(evaluate_program(&self.manifest, path, row, &program));
        }
        if applies(&self.manifest, path, row, "first-hour-doc-recipe")
            && path == "docs/first-hour.md"
        {
            let typed = jet_fences(source).iter().any(|fence| {
                parse_program(fence)
                    .map(|program| has_typed_cli_entry(&program))
                    .unwrap_or(false)
            });
            if !typed || !has_jet_run_command(source) {
                let kind = if typed {
                    "document:jet-run"
                } else {
                    "document:typed-cli"
                };
                all.push(SemanticViolation::new(
                    path,
                    "first-hour-doc-recipe",
                    site(kind, Span::new(0, 0)),
                ));
            }
        }
        self.apply_exceptions(path, all)
    }

    /// A maintained source may suppress one of the seven shipped semantic
    /// guidance rules only with an exact, occurrence-scoped manifest row. The
    /// compiler's ordinary statement-local `#allow` remains unchanged for
    /// user code and for negative diagnostic fixtures.
    fn validate_maintained_lint_allows(
        &self,
        path: &str,
        row: &SourceRow,
        program: &Program,
    ) -> Result<(), String> {
        if !matches!(row.role, SourceRole::CanonicalTeaching | SourceRole::ExpertLesson) {
            return Ok(());
        }
        let mut occurrences: BTreeMap<(String, String), usize> = BTreeMap::new();
        for application in &program.rule_facts {
            if application.marker.name != jet::Syntax::MARKER_ALLOW {
                continue;
            }
            let Some(target) = application.target else {
                continue;
            };
            for argument in &application.marker.args {
                let Expr::Ident(rule, _) = argument else {
                    continue;
                };
                if !maintained_guidance_lint(rule) {
                    continue;
                }
                let site = format!(
                    "allow:{rule}@{}..{}",
                    target.start, target.end
                );
                *occurrences.entry((rule.clone(), site)).or_default() += 1;
            }
        }

        for ((rule, site), count) in &occurrences {
            let matches = self
                .manifest
                .exceptions
                .iter()
                .filter(|exception| {
                    exception.rule == *rule
                        && exception.selector == format!("file:{path}")
                        && exception.site == *site
                })
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(format!(
                    "file={path}; rule={rule}; site={site}; why=maintained #allow needs one occurrence-scoped manifest reason; replacement=add one exact [exception] row"
                ));
            }
            if matches[0].expected != *count {
                return Err(format!(
                    "file={path}; rule={rule}; site={site}; why=manifest expects {} occurrence(s), found {count}; replacement=keep one reviewed #allow occurrence per manifest row",
                    matches[0].expected
                ));
            }
        }

        for exception in self.manifest.exceptions.iter().filter(|exception| {
            exception.selector == format!("file:{path}")
                && exception.site.starts_with("allow:")
        }) {
            let count = occurrences
                .get(&(exception.rule.clone(), exception.site.clone()))
                .copied()
                .unwrap_or(0);
            if count != exception.expected {
                return Err(format!(
                    "file={path}; rule={}; site={}; why=manifest expects {} occurrence(s), found {count}; replacement=remove the stale allowance or update its exact reviewed site",
                    exception.rule, exception.site, exception.expected
                ));
            }
        }
        Ok(())
    }

    pub fn evaluate_generated(
        &self,
        producer: &str,
        source: &str,
    ) -> Result<Vec<SemanticViolation>, String> {
        let Some(row) = self.manifest.producers.iter().find(|row| row.selector == producer) else {
            return Err(inventory_error(
                "unclassified generated-source producer",
                producer,
                "inventory:producer",
            ));
        };
        let program = parse_program(source)?;
        let decoders = program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Func(function) if is_response_decoder(function) => Some(function),
                _ => None,
            })
            .collect::<Vec<_>>();
        if is_no_response_protocol(&row.protocol) {
            return Ok(Vec::new());
        }
        let mut violations = if decoders.is_empty() {
            vec![SemanticViolation::new(
                producer,
                "bindgen-one-decoder",
                format!("producer:{}#decode_response@0..0", producer),
            )]
        } else {
            decoders
                .iter()
                .skip(1)
                .map(|function| {
                    SemanticViolation::new(
                        producer,
                        "bindgen-one-decoder",
                        format!(
                            "producer:{}#response-decoder@{}..{}",
                            producer, function.span.start, function.span.end
                        ),
                    )
                })
                .collect::<Vec<_>>()
        };
        if decoders.len() == 1
            && protocol_override_identity(&row.protocol).is_some()
            && ordinary_envelope_decoder(decoders[0])
        {
            let identity = protocol_override_identity(&row.protocol).unwrap_or("unknown");
            violations.push(SemanticViolation::new(
                producer,
                "generated-protocol-override",
                format!(
                    "producer:{}#protocol:{}@{}..{}",
                    producer, identity, decoders[0].span.start, decoders[0].span.end
                ),
            ));
        }
        Ok(violations)
    }

    /// Validate candidate occurrences supplied by a domain gate. The matching
    /// key is rule + exact manifest selector + stable semantic site. One
    /// exception cannot absorb a second occurrence.
    pub fn validate_occurrences(
        &self,
        selector: &str,
        candidates: &[SemanticViolation],
    ) -> Result<(), String> {
        let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
        for candidate in candidates {
            if candidate.file != selector {
                return Err(format!("candidate file {} does not match {selector}", candidate.file));
            }
            let key = (candidate.rule.clone(), candidate.site.clone());
            *counts.entry(key).or_default() += 1;
        }
        for ((rule, site), count) in counts {
            let matches = self
                .manifest
                .exceptions
                .iter()
                .filter(|exception| {
                    exception.rule == rule && exception.selector == format!("file:{selector}") && exception.site == site
                })
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(format!(
                    "file={selector}; rule={rule}; site={site}; why=occurrence has no unique exception; replacement=add one exact exception"
                ));
            }
            let expected = matches[0].expected;
            if count != expected {
                return Err(format!(
                    "file={selector}; rule={rule}; site={site}; why=expected {expected} occurrence(s), found {count}; replacement=remove the additional occurrence or add a distinct reviewed site"
                ));
            }
        }
        for exception in self
            .manifest
            .exceptions
            .iter()
            .filter(|exception| exception.selector == format!("file:{selector}"))
        {
            let count = candidates
                .iter()
                .filter(|candidate| {
                    candidate.rule == exception.rule && candidate.site == exception.site
                })
                .count();
            if count != exception.expected {
                return Err(format!(
                    "file={selector}; rule={}; site={}; why=expected {} occurrence(s), found {count}; replacement=remove the additional occurrence or update the exact reviewed site",
                    exception.rule, exception.site, exception.expected
                ));
            }
        }
        Ok(())
    }

    fn apply_exceptions(
        &self,
        path: &str,
        candidates: Vec<SemanticViolation>,
    ) -> Result<Vec<SemanticViolation>, String> {
        let mut remaining = Vec::new();
        let mut matched: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut errors = Vec::new();
        for candidate in &candidates {
            let key = (candidate.rule.clone(), candidate.site.clone());
            let exception = self.manifest.exceptions.iter().find(|exception| {
                exception.rule == candidate.rule
                    && exception.selector == format!("file:{path}")
                    && exception.site == candidate.site
            });
            if exception.is_some() {
                *matched.entry(key).or_default() += 1;
            }
        }
        for exception in self
            .manifest
            .exceptions
            .iter()
            .filter(|exception| exception.selector == format!("file:{path}"))
        {
            let actual = matched
                .get(&(exception.rule.clone(), exception.site.clone()))
                .copied()
                .unwrap_or(0);
            if actual != exception.expected {
                errors.push(format!(
                    "file={path}; rule={}; site={}; why=expected {} occurrence(s), found {}; replacement=remove the additional occurrence or update the exact reviewed site",
                    exception.rule, exception.site, exception.expected, actual
                ));
            }
        }
        let mut consumed: BTreeMap<(String, String), usize> = BTreeMap::new();
        for candidate in candidates {
            let key = (candidate.rule.clone(), candidate.site.clone());
            let exception = self.manifest.exceptions.iter().find(|exception| {
                exception.rule == candidate.rule
                    && exception.selector == format!("file:{path}")
                    && exception.site == candidate.site
            });
            if let Some(exception) = exception {
                let seen = consumed.entry(key).or_default();
                *seen += 1;
                if *seen > exception.expected {
                    remaining.push(candidate);
                }
            } else {
                remaining.push(candidate);
            }
        }
        if errors.is_empty() {
            Ok(remaining)
        } else {
            Err(errors.join("\n"))
        }
    }

    fn source_row(&self, path: &str) -> Option<&SourceRow> {
        self.manifest
            .sources
            .iter()
            .filter(|row| selector_matches(&row.selector, path))
            .max_by_key(|row| selector_specificity(&row.selector))
    }
}

fn is_response_decoder(function: &jet::AST::Func) -> bool {
    function.name == "decode_response"
        && function.params.len() == 2
        && named_type(&function.params[0].ty, "String")
        && named_type(&function.params[1].ty, "Int")
        && matches!(
            function.return_type.as_ref(),
            Some(Type::Result { ok, .. }) if named_type(ok, "DataTree")
    )
}

fn protocol_override_identity(protocol: &str) -> Option<&str> {
    protocol
        .strip_prefix("override:")
        .and_then(|body| body.split_once(':').map(|(identity, _)| identity))
}

fn ordinary_envelope_decoder(function: &jet::AST::Func) -> bool {
    let mut has_status_field = false;
    for_each_statement_expr(&function.body, |expr| {
        if let Expr::MethodCall { method, args, .. } = expr.without_parens() {
            if method == "field"
                && args.first().is_some_and(|arg| is_literal_string(&arg.expr, "ok"))
            {
                has_status_field = true;
            }
        }
    });
    has_status_field
}

impl CorpusManifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut manifest = Self::default();
        let mut section = String::new();
        for (line_number, raw) in text.lines().enumerate() {
            let line = raw.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].to_string();
                if !matches!(section.as_str(), "source" | "scope" | "producer" | "artifact" | "finding" | "exception") {
                    return Err(format!("manifest line {}: unknown section [{}]", line_number + 1, section));
                }
                continue;
            }
            let fields = line.split('\t').collect::<Vec<_>>();
            let error = |message: &str| format!("manifest line {}: {message}", line_number + 1);
            match section.as_str() {
                "source" if fields.len() == 8 => manifest.sources.push(SourceRow {
                    selector: parse_selector(fields[1]).map_err(|message| error(&message))?,
                    role: SourceRole::parse(fields[2]).map_err(|message| error(&message))?,
                    profile: nonempty(fields[3], &error("source profile is empty"))?,
                    decision: nonempty(fields[4], &error("source decision is empty"))?,
                    proof: nonempty(fields[5], &error("source proof is empty"))?,
                    owner: nonempty(fields[6], &error("source owner is empty"))?,
                    reason: nonempty(fields[7], &error("source reason is empty"))?,
                }),
                "scope" if fields.len() == 4 => manifest.scopes.push(RuleScopeRow {
                    rule: nonempty(fields[1], &error("scope rule is empty"))?,
                    selector: parse_selector(fields[2]).map_err(|message| error(&message))?,
                    reason: nonempty(fields[3], &error("scope reason is empty"))?,
                }),
                "producer" if fields.len() == 6 => manifest.producers.push(ProducerRow {
                    selector: parse_producer(fields[1]).map_err(|message| error(&message))?,
                    artifact: parse_artifact(fields[2]).map_err(|message| error(&message))?,
                    protocol: nonempty(fields[3], &error("producer protocol is empty"))?,
                    owner: nonempty(fields[4], &error("producer owner is empty"))?,
                    reason: nonempty(fields[5], &error("producer reason is empty"))?,
                }),
                "artifact" if fields.len() == 5 => manifest.artifacts.push(ArtifactRow {
                    selector: parse_artifact(fields[1]).map_err(|message| error(&message))?,
                    producer: parse_producer(fields[2]).map_err(|message| error(&message))?,
                    kind: nonempty(fields[3], &error("artifact kind is empty"))?,
                    reason: nonempty(fields[4], &error("artifact reason is empty"))?,
                }),
                "finding" if fields.len() == 5 => manifest.findings.push(FindingRow {
                    id: nonempty(fields[1], &error("finding id is empty"))?,
                    rule: nonempty(fields[2], &error("finding rule is empty"))?,
                    owner: nonempty(fields[3], &error("finding owner is empty"))?,
                    proof: nonempty(fields[4], &error("finding proof is empty"))?,
                }),
                "exception" if fields.len() == 7 => manifest.exceptions.push(ExceptionRow {
                    rule: nonempty(fields[1], &error("exception rule is empty"))?,
                    selector: parse_selector(fields[2]).map_err(|message| error(&message))?,
                    site: parse_site(fields[3]).map_err(|message| error(&message))?,
                    expected: fields[4].parse().map_err(|_| error("exception occurrence is not a positive integer"))?,
                    protocol: nonempty(fields[5], &error("exception protocol is empty"))?,
                    reason: nonempty(fields[6], &error("exception reason is empty"))?,
                }),
                _ => return Err(error("wrong field count or row outside a section")),
            }
            if fields.first() != Some(&"1") {
                return Err(error("unsupported manifest version"));
            }
        }
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        unique_selectors(&self.sources.iter().map(|row| row.selector.as_str()).collect::<Vec<_>>(), "source", &mut errors);
        let mut scope_keys = BTreeSet::new();
        for scope in &self.scopes {
            if !scope_keys.insert((&scope.rule, &scope.selector)) {
                errors.push(format!("duplicate scope {} {}", scope.rule, scope.selector));
            }
        }
        unique_selectors(&self.producers.iter().map(|row| row.selector.as_str()).collect::<Vec<_>>(), "producer", &mut errors);
        unique_selectors(&self.artifacts.iter().map(|row| row.selector.as_str()).collect::<Vec<_>>(), "artifact", &mut errors);
        unique_selectors(&self.findings.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), "finding", &mut errors);
        let rule_names = rule_registry().iter().map(|spec| spec.name).collect::<BTreeSet<_>>();
        let producer_names = self.producers.iter().map(|row| row.selector.as_str()).collect::<BTreeSet<_>>();
        for finding in &self.findings {
            if !rule_names.contains(finding.rule.as_str()) {
                errors.push(format!("finding {} names unknown rule {}", finding.id, finding.rule));
            }
            if finding.owner.is_empty() || finding.proof.is_empty() {
                errors.push(format!("finding {} has no owner or executable proof", finding.id));
            }
            if !self.scopes.iter().any(|scope| scope.rule == finding.rule)
                && !domain_guard_rule(finding.rule.as_str())
            {
                errors.push(format!(
                    "finding {} rule {} has no executable corpus or domain guard",
                    finding.id, finding.rule
                ));
            }
        }
        if self.findings.len() != 38 {
            errors.push(format!("manifest must retain all 38 audited findings, found {}", self.findings.len()));
        }
        for scope in &self.scopes {
            if !rule_names.contains(scope.rule.as_str()) {
                errors.push(format!("scope names unknown rule {}", scope.rule));
            }
            let path = selector_path(&scope.selector);
            if !self
                .sources
                .iter()
                .any(|source| selector_matches(&source.selector, path))
            {
                errors.push(format!("scope {} names an unclassified source selector {}", scope.rule, scope.selector));
            }
            if scope.reason.is_empty() {
                errors.push(format!("scope {} {} has no reason", scope.rule, scope.selector));
            }
        }
        for producer in &self.producers {
            if let Err(error) = validate_protocol(&producer.protocol) {
                errors.push(format!(
                    "{} ({error})",
                    SemanticViolation::new(
                        producer_path(&producer.selector),
                        "generated-protocol-override",
                        site("manifest:generated-protocol", Span::new(0, 0)),
                    )
                ));
            }
            if !self.artifacts.iter().any(|artifact| artifact.selector == producer.artifact && artifact.producer == producer.selector) {
                errors.push(format!("producer {} family has no artifact row", producer.selector));
            }
        }
        for artifact in &self.artifacts {
            if !producer_names.contains(artifact.producer.as_str()) {
                errors.push(format!("artifact {} names unknown producer {}", artifact.selector, artifact.producer));
            }
        }
        let mut exception_keys = BTreeSet::new();
        for exception in &self.exceptions {
            if !rule_names.contains(exception.rule.as_str()) {
                errors.push(format!("exception names unknown rule {}", exception.rule));
            }
            let key = (&exception.rule, &exception.selector, &exception.site);
            if !exception_keys.insert(key) {
                errors.push(format!("duplicate occurrence exception {} {} {}", exception.rule, exception.selector, exception.site));
            }
            if exception.expected == 0 {
                errors.push(format!("exception {} {} has zero expected occurrences", exception.rule, exception.site));
            }
            if !exception.selector.starts_with("file:") {
                errors.push(format!(
                    "exception {} {} is not occurrence-scoped to one file",
                    exception.rule, exception.site
                ));
            } else {
                let path = selector_path(&exception.selector);
                match self
                    .sources
                    .iter()
                    .filter(|source| selector_matches(&source.selector, path))
                    .max_by_key(|source| selector_specificity(&source.selector))
                {
                    None => errors.push(format!(
                        "exception {} names an unclassified source {}",
                        exception.rule, exception.selector
                    )),
                    Some(source)
                        if !exception.site.starts_with("allow:")
                            && !matches!(
                                source.role,
                                SourceRole::ExpertLesson | SourceRole::NegativeDiagnostic
                            ) =>
                    {
                        errors.push(format!(
                            "exception {} {} requires an expert-lesson or negative-diagnostic source, found {:?}",
                            exception.rule, exception.selector, source.role
                        ));
                    }
                    Some(_) => {}
                }
            }
            if exception.protocol.is_empty() || exception.reason.is_empty() {
                errors.push(format!("exception {} {} has no protocol or reason", exception.rule, exception.site));
            } else if let Err(error) = validate_exception_protocol(&exception.protocol) {
                errors.push(format!(
                    "exception {} {}: {error}",
                    exception.rule, exception.site
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}

fn validate_manifest_paths(manifest: &CorpusManifest, root: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    for source in &manifest.sources {
        let path = selector_path(&source.selector);
        let exists = match source.selector.strip_prefix("root:") {
            Some(_) => root.join(path).is_dir(),
            None => root.join(path).is_file(),
        };
        if !exists {
            errors.push(format!(
                "manifest source selector does not exist: {}",
                source.selector
            ));
        }
    }
    for producer in &manifest.producers {
        let path = producer_path(&producer.selector);
        if !root.join(path).is_file() {
            errors.push(format!(
                "manifest producer source does not exist: {}",
                producer.selector
            ));
        }
    }
    for exception in &manifest.exceptions {
        let Some(path) = exception.selector.strip_prefix("file:") else {
            continue;
        };
        if !root.join(path).is_file() {
            errors.push(format!(
                "manifest exception source does not exist: {}",
                exception.selector
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn selector_path(selector: &str) -> &str {
    selector
        .strip_prefix("root:")
        .or_else(|| selector.strip_prefix("file:"))
        .unwrap_or(selector)
}

fn producer_path(selector: &str) -> &str {
    selector
        .strip_prefix("producer:")
        .and_then(|body| body.split_once('#').map(|(path, _)| path))
        .unwrap_or(selector)
}

fn parse_selector(value: &str) -> Result<String, String> {
    let path = value
        .strip_prefix("root:")
        .or_else(|| value.strip_prefix("file:"))
        .ok_or_else(|| format!("selector must start with root: or file: ({value})"))?;
    validate_relative_path(path)?;
    Ok(value.to_string())
}

fn parse_producer(value: &str) -> Result<String, String> {
    let body = value
        .strip_prefix("producer:")
        .ok_or_else(|| format!("producer selector must start with producer: ({value})"))?;
    let Some((path, symbol)) = body.split_once('#') else {
        return Err(format!("producer selector must name a symbol ({value})"));
    };
    validate_relative_path(path)?;
    if symbol.is_empty() || symbol.contains('#') {
        return Err(format!("producer selector must name one non-empty symbol ({value})"));
    }
    Ok(value.to_string())
}

fn parse_artifact(value: &str) -> Result<String, String> {
    if let Some(path) = value.strip_prefix("path:") {
        validate_relative_path(path)?;
    } else if let Some(family) = value.strip_prefix("family:") {
        if family.is_empty() || family.contains('/') {
            return Err(format!("artifact family is not a stable identifier ({value})"));
        }
    } else {
        return Err(format!("artifact selector must start with path: or family: ({value})"));
    }
    Ok(value.to_string())
}

fn parse_site(value: &str) -> Result<String, String> {
    let (kind, span) = value
        .rsplit_once('@')
        .ok_or_else(|| format!("semantic site must carry a span ({value})"))?;
    if kind.is_empty() || kind.contains('@') {
        return Err(format!("semantic site kind is empty or malformed ({value})"));
    }
    let (start, end) = span
        .split_once("..")
        .ok_or_else(|| format!("semantic site span must use start..end ({value})"))?;
    let start = start.parse::<usize>().map_err(|_| format!("bad site start ({value})"))?;
    let end = end.parse::<usize>().map_err(|_| format!("bad site end ({value})"))?;
    if end < start {
        return Err(format!("site end precedes start ({value})"));
    }
    Ok(value.to_string())
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() || Path::new(path).is_absolute() || path.split('/').any(|part| part == ".." || part.is_empty()) {
        return Err(format!("manifest path is not a normalized relative path ({path})"));
    }
    Ok(())
}

fn nonempty(value: &str, error: &str) -> Result<String, String> {
    if value.is_empty() {
        Err(error.to_string())
    } else {
        Ok(value.to_string())
    }
}

fn unique_selectors(values: &[&str], kind: &str, errors: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(*value) {
            errors.push(format!("duplicate {kind} selector {value}"));
        }
    }
}

fn selector_matches(selector: &str, path: &str) -> bool {
    if let Some(file) = selector.strip_prefix("file:") {
        return file == path;
    }
    let Some(root) = selector.strip_prefix("root:") else {
        return false;
    };
    path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

fn selector_specificity(selector: &str) -> usize {
    selector.strip_prefix("file:").map_or(0, |file| 100_000 + file.len())
        + selector.strip_prefix("root:").map_or(0, str::len)
}

fn is_no_response_protocol(protocol: &str) -> bool {
    protocol == "no-response-protocol"
}

fn validate_protocol(protocol: &str) -> Result<(), String> {
    if protocol == "ordinary-envelope" || is_no_response_protocol(protocol) {
        return Ok(());
    }
    let Some(override_body) = protocol.strip_prefix("override:") else {
        return Err(format!(
            "generated protocol must be ordinary-envelope, no-response-protocol, or an override ({protocol})"
        ));
    };
    let Some((identity, proof)) = override_body.split_once(':') else {
        return Err(format!(
            "generated protocol override must name identity and proof ({protocol})"
        ));
    };
    if identity.is_empty()
        || proof != "differs-from-ordinary-envelope"
        || identity == "ordinary-envelope"
    {
        return Err(format!(
            "generated protocol override must prove a distinct protocol identity ({protocol})"
        ));
    }
    Ok(())
}

fn validate_exception_protocol(protocol: &str) -> Result<(), String> {
    if matches!(
        protocol,
        "fixture"
            | "expert-lesson"
            | "negative-diagnostic"
            | "canonical-teaching"
            | "raw-protocol"
            | "fixture-harness-boundary"
    ) {
        return Ok(());
    }
    if protocol.starts_with("override:") {
        return validate_protocol(protocol);
    }
    Err(format!(
        "exception protocol must name its source boundary or a generated override ({protocol})"
    ))
}

fn domain_guard_rule(rule: &str) -> bool {
    matches!(
        rule,
        "product-parser-regression"
            | "compiler-loop-fact"
            | "build-root-fact"
            | "fixture-role-classification"
            | "fixture-cli-role"
            | "bindgen-one-decoder"
    )
}

fn semantic_role(role: SourceRole) -> bool {
    matches!(role, SourceRole::CanonicalTeaching | SourceRole::ExpertLesson | SourceRole::GeneratedOutput)
}

fn fixture_role_for_path(path: &str) -> Option<SourceRole> {
    if path.starts_with("tests/ui/") || path.starts_with("tests/ui_lint/") {
        Some(SourceRole::NegativeDiagnostic)
    } else if path.starts_with("tests/fuzz/")
        || path.starts_with("tests/conformance/corpus/")
        || path.starts_with("adoption/fixtures/")
    {
        Some(SourceRole::FixtureHarnessBoundary)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
enum ProfileFixtureRole {
    One(SourceRole),
    Generated,
}

fn profile_fixture_role(profile: &str) -> Option<ProfileFixtureRole> {
    match profile {
        "negative" => Some(ProfileFixtureRole::One(SourceRole::NegativeDiagnostic)),
        "fixture" | "canonical-test" | "package-template" => {
            Some(ProfileFixtureRole::One(SourceRole::FixtureHarnessBoundary))
        }
        "protocol" => Some(ProfileFixtureRole::One(SourceRole::RawProtocol)),
        "generated" => Some(ProfileFixtureRole::Generated),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliRecipe {
    Typed,
    Builder,
    Raw,
    TypedDoc,
}

fn cli_recipe_profile(profile: &str) -> Option<CliRecipe> {
    match profile {
        "typed-cli" => Some(CliRecipe::Typed),
        "typed-cli-doc" => Some(CliRecipe::TypedDoc),
        "builder-cli" => Some(CliRecipe::Builder),
        "raw-cli" => Some(CliRecipe::Raw),
        _ => None,
    }
}

fn has_typed_cli_entry(program: &Program) -> bool {
    jet_foundation::CLISchema::entry_schema(&program.items).is_some()
}

fn has_args_builder(program: &Program) -> bool {
    let aliases = module_aliases(program);
    let expressions = program_expressions(program);
    let has_spec = expressions.iter().copied().any(|expr| {
        matches!(
            expr.without_parens(),
            Expr::MethodCall { receiver, method, .. }
                if method == "spec" && receiver_is(receiver, "args", &aliases)
        )
    });
    let has_parse = expressions.iter().copied().any(|expr| {
        matches!(
            expr.without_parens(),
            Expr::MethodCall { method, .. } if method == "parse" || method == "parse_or_exit"
        )
    });
    has_spec && has_parse
}

fn has_raw_process_boundary(program: &Program) -> bool {
    let aliases = module_aliases(program);
    program_expressions(program)
        .iter()
        .copied()
        .any(|expr| is_process_argv(expr, &aliases))
}

enum CorpusWalkNode<'a> {
    Expr(&'a Expr),
    Stmt(&'a Stmt),
    LValue(&'a jet::AST::LValue),
    ForKind(&'a ForKind),
    BindPattern(&'a BindPattern),
    Pattern(&'a Pattern),
    Fallback(&'a OrFallback),
}

fn push_corpus_statements<'a>(stack: &mut Vec<CorpusWalkNode<'a>>, body: &'a [Stmt]) {
    for statement in body.iter().rev() {
        stack.push(CorpusWalkNode::Stmt(statement));
    }
}

fn push_corpus_args<'a>(stack: &mut Vec<CorpusWalkNode<'a>>, args: &'a [CallArg]) {
    for argument in args.iter().rev() {
        stack.push(CorpusWalkNode::Expr(&argument.expr));
    }
}

/// Visit corpus expressions with an explicit work stack. The AST's shared
/// read-only visitor clones each root and recursively descends it; a source
/// corpus entry can be deeper than the test thread's stack even when it is a
/// valid program. Keeping references also avoids cloning every ancestor once
/// per visited expression.
fn for_each_statement_expr<'a>(body: &'a [Stmt], mut f: impl FnMut(&'a Expr)) {
    let mut stack = Vec::with_capacity(body.len());
    push_corpus_statements(&mut stack, body);

    while let Some(node) = stack.pop() {
        match node {
            CorpusWalkNode::Stmt(statement) => match statement {
                Stmt::Expr(expr) => stack.push(CorpusWalkNode::Expr(expr)),
                Stmt::DeferClose { close, .. } => stack.push(CorpusWalkNode::Expr(close)),
                Stmt::Val(binding) => {
                    if let Some(pattern) = binding.pattern.as_ref() {
                        stack.push(CorpusWalkNode::BindPattern(pattern));
                    }
                    stack.push(CorpusWalkNode::Expr(&binding.init));
                }
                Stmt::Assign { target, value, .. } => {
                    stack.push(CorpusWalkNode::Expr(value));
                    stack.push(CorpusWalkNode::LValue(target));
                }
                Stmt::Return(value, _) => {
                    if let Some(value) = value {
                        stack.push(CorpusWalkNode::Expr(value));
                    }
                }
                Stmt::While { cond, body, .. } => {
                    push_corpus_statements(&mut stack, body);
                    stack.push(CorpusWalkNode::Expr(cond));
                }
                Stmt::For { kind, body, .. } => {
                    push_corpus_statements(&mut stack, body);
                    stack.push(CorpusWalkNode::ForKind(kind));
                }
                Stmt::Switch {
                    subject,
                    arms,
                    else_body,
                    ..
                }
                | Stmt::ComptimeSwitch {
                    subject,
                    arms,
                    else_body,
                    ..
                } => {
                    if let Some(body) = else_body {
                        push_corpus_statements(&mut stack, body);
                    }
                    for arm in arms.iter().rev() {
                        push_corpus_statements(&mut stack, &arm.body);
                        stack.push(CorpusWalkNode::Expr(&arm.cond));
                    }
                    stack.push(CorpusWalkNode::Expr(subject));
                }
                Stmt::BreakValue(value, _) | Stmt::Yield(value, _) => {
                    stack.push(CorpusWalkNode::Expr(value));
                }
                Stmt::BreakLabelValue(_, _, value, _) => {
                    stack.push(CorpusWalkNode::Expr(value));
                }
                Stmt::Loop { body, .. }
                | Stmt::Reactive { body, .. }
                | Stmt::Shield { body, .. }
                | Stmt::Switched { body, .. }
                | Stmt::Region { body, .. }
                | Stmt::Policy { body, .. }
                | Stmt::AuthorityScope { body, .. }
                | Stmt::ComptimeBlock { body, .. }
                | Stmt::Live { body, .. }
                | Stmt::Transact { body, .. }
                | Stmt::Layout { body, .. } => push_corpus_statements(&mut stack, body),
                Stmt::Unsafe {
                    audit_expr, body, ..
                } => {
                    push_corpus_statements(&mut stack, body);
                    if let Some(expr) = audit_expr {
                        stack.push(CorpusWalkNode::Expr(expr));
                    }
                }
                Stmt::Impure {
                    reason_expr, body, ..
                } => {
                    push_corpus_statements(&mut stack, body);
                    if let Some(expr) = reason_expr {
                        stack.push(CorpusWalkNode::Expr(expr));
                    }
                }
                Stmt::CountedLoop {
                    init,
                    cond,
                    step,
                    body,
                    ..
                } => {
                    push_corpus_statements(&mut stack, body);
                    if let Some(step) = step {
                        stack.push(CorpusWalkNode::Stmt(step.as_ref()));
                    }
                    stack.push(CorpusWalkNode::Expr(cond));
                    stack.push(CorpusWalkNode::Expr(&init.init));
                }
                Stmt::TaskGroup { limit, body, .. } => {
                    push_corpus_statements(&mut stack, body);
                    if let Some(limit) = limit {
                        stack.push(CorpusWalkNode::Expr(limit));
                    }
                }
                Stmt::ContextBlock { fields, body, .. } => {
                    push_corpus_statements(&mut stack, body);
                    for (_, value, _) in fields.iter().rev() {
                        stack.push(CorpusWalkNode::Expr(value));
                    }
                }
                Stmt::AssumeDet {
                    reason_expr, body, ..
                } => {
                    push_corpus_statements(&mut stack, body);
                    stack.push(CorpusWalkNode::Expr(reason_expr));
                }
                Stmt::ComptimeIf {
                    cond,
                    then_body,
                    else_body,
                    ..
                } => {
                    if let Some(body) = else_body {
                        push_corpus_statements(&mut stack, body);
                    }
                    push_corpus_statements(&mut stack, then_body);
                    stack.push(CorpusWalkNode::Expr(cond));
                }
                Stmt::ScopeMember { args, body, .. } => {
                    push_corpus_statements(&mut stack, body);
                    for arg in args.iter().rev() {
                        stack.push(CorpusWalkNode::Expr(arg));
                    }
                }
                Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::BreakLabel(..)
                | Stmt::ContinueLabel(..) => {}
            },
            CorpusWalkNode::Expr(expr) => {
                f(expr);
                match expr {
                    Expr::Str(parts, _) => {
                        for part in parts.iter().rev() {
                            if let StrPart::Interp(inner, _) = part {
                                stack.push(CorpusWalkNode::Expr(inner));
                            }
                        }
                    }
                    Expr::ListLit(items, _) => {
                        for item in items.iter().rev() {
                            stack.push(CorpusWalkNode::Expr(item));
                        }
                    }
                    Expr::MemberSpread { base, .. }
                    | Expr::Spread(base, _)
                    | Expr::Deref(base, _)
                    | Expr::RawOf(base, _)
                    | Expr::Copy(base, _)
                    | Expr::Place(base, _, _)
                    | Expr::Field(base, _, _)
                    | Expr::Present(base, _)
                    | Expr::Ok(base, _)
                    | Expr::Err(base, _)
                    | Expr::Paren(base, _) => {
                        stack.push(CorpusWalkNode::Expr(base));
                    }
                    Expr::Try(base, _, _, note) => {
                        if let Some(note) = note {
                            stack.push(CorpusWalkNode::Expr(note));
                        }
                        stack.push(CorpusWalkNode::Expr(base));
                    }
                    Expr::MapLit(entries, _) => {
                        for (key, value) in entries.iter().rev() {
                            stack.push(CorpusWalkNode::Expr(value));
                            stack.push(CorpusWalkNode::Expr(key));
                        }
                    }
                    Expr::Index { base, index, .. } => {
                        stack.push(CorpusWalkNode::Expr(index));
                        stack.push(CorpusWalkNode::Expr(base));
                    }
                    Expr::Slice {
                        base,
                        start,
                        end,
                        range,
                        ..
                    } => {
                        if let Some(range) = range {
                            stack.push(CorpusWalkNode::Expr(range));
                        }
                        stack.push(CorpusWalkNode::Expr(end));
                        stack.push(CorpusWalkNode::Expr(start));
                        stack.push(CorpusWalkNode::Expr(base));
                    }
                    Expr::Range { start, end, .. } => {
                        stack.push(CorpusWalkNode::Expr(end));
                        stack.push(CorpusWalkNode::Expr(start));
                    }
                    Expr::Call(call) => push_corpus_args(&mut stack, &call.args),
                    Expr::Unary(_, inner, _) => stack.push(CorpusWalkNode::Expr(inner)),
                    Expr::Binary(_, lhs, rhs, _) => {
                        stack.push(CorpusWalkNode::Expr(rhs));
                        stack.push(CorpusWalkNode::Expr(lhs));
                    }
                    Expr::CompareChain { operands, .. } => {
                        for operand in operands.iter().rev() {
                            stack.push(CorpusWalkNode::Expr(operand));
                        }
                    }
                    Expr::OptField { base, .. } => stack.push(CorpusWalkNode::Expr(base)),
                    Expr::MethodCall {
                        receiver, args, ..
                    } => {
                        push_corpus_args(&mut stack, args);
                        stack.push(CorpusWalkNode::Expr(receiver));
                    }
                    Expr::StructLit { fields, .. } => {
                        for (_, _, value) in fields.iter().rev() {
                            stack.push(CorpusWalkNode::Expr(value));
                        }
                    }
                    Expr::TypedLit { body, .. } => match body {
                        jet::AST::TypedLitBody::Fields(fields) => {
                            for (_, _, value) in fields.iter().rev() {
                                stack.push(CorpusWalkNode::Expr(value));
                            }
                        }
                        jet::AST::TypedLitBody::Elements(elements) => {
                            for element in elements.iter().rev() {
                                stack.push(CorpusWalkNode::Expr(element));
                            }
                        }
                        jet::AST::TypedLitBody::Entries(entries) => {
                            for (key, value) in entries.iter().rev() {
                                stack.push(CorpusWalkNode::Expr(value));
                                stack.push(CorpusWalkNode::Expr(key));
                            }
                        }
                        jet::AST::TypedLitBody::Value(value) => {
                            stack.push(CorpusWalkNode::Expr(value));
                        }
                        jet::AST::TypedLitBody::ByteText(_) | jet::AST::TypedLitBody::Empty => {}
                    },
                    Expr::EnumLit { args, .. } => {
                        for arg in args.iter().rev() {
                            let value = match arg {
                                jet::AST::EnumLitArg::Positional(value)
                                | jet::AST::EnumLitArg::Named { expr: value, .. } => value,
                            };
                            stack.push(CorpusWalkNode::Expr(value));
                        }
                    }
                    Expr::Tainted(inner, _, _) => stack.push(CorpusWalkNode::Expr(inner)),
                    Expr::PatternTest {
                        subject, pattern, ..
                    } => {
                        stack.push(CorpusWalkNode::Pattern(pattern));
                        stack.push(CorpusWalkNode::Expr(subject));
                    }
                    Expr::OrFallback {
                        value, fallback, ..
                    } => {
                        stack.push(CorpusWalkNode::Fallback(fallback));
                        stack.push(CorpusWalkNode::Expr(value));
                    }
                    Expr::If {
                        cond,
                        then_body,
                        then_value,
                        else_body,
                        else_value,
                        ..
                    } => {
                        stack.push(CorpusWalkNode::Expr(else_value));
                        push_corpus_statements(&mut stack, else_body);
                        stack.push(CorpusWalkNode::Expr(then_value));
                        push_corpus_statements(&mut stack, then_body);
                        stack.push(CorpusWalkNode::Expr(cond));
                    }
                    Expr::TupleLit(fields, _, _) => {
                        for (_, value) in fields.iter().rev() {
                            stack.push(CorpusWalkNode::Expr(value));
                        }
                    }
                    Expr::Lambda(lambda) => match &lambda.body {
                        LambdaBody::Expr(value) => stack.push(CorpusWalkNode::Expr(value)),
                        LambdaBody::Block(body) => push_corpus_statements(&mut stack, body),
                    },
                    Expr::CallValue { callee, args, .. } => {
                        push_corpus_args(&mut stack, args);
                        stack.push(CorpusWalkNode::Expr(callee));
                    }
                    Expr::PtrFromAddr { addr, .. } => stack.push(CorpusWalkNode::Expr(addr)),
                    Expr::IncDec { operand, .. } => stack.push(CorpusWalkNode::Expr(operand)),
                    Expr::StrMatchLit(..)
                    | Expr::BinMatchLit(..)
                    | Expr::Int(..)
                    | Expr::Float(..)
                    | Expr::Bool(..)
                    | Expr::Unit(..)
                    | Expr::Char(..)
                    | Expr::Ident(..)
                    | Expr::UnitLit { .. }
                    | Expr::Absent(..)
                    | Expr::Todo { .. }
                    | Expr::NoElse(..)
                    | Expr::ReduceMarker(..)
                    | Expr::ComptimeName { .. } => {}
                }
            }
            CorpusWalkNode::LValue(lvalue) => match lvalue {
                jet::AST::LValue::Local { .. } => {}
                jet::AST::LValue::Index { base, index, .. } => {
                    stack.push(CorpusWalkNode::Expr(index));
                    stack.push(CorpusWalkNode::Expr(base));
                }
                jet::AST::LValue::Field { base, .. } => {
                    stack.push(CorpusWalkNode::Expr(base));
                }
            },
            CorpusWalkNode::ForKind(kind) => match kind {
                ForKind::Range {
                    start, end, step, ..
                } => {
                    if let Some(step) = step {
                        stack.push(CorpusWalkNode::Expr(step));
                    }
                    stack.push(CorpusWalkNode::Expr(end));
                    stack.push(CorpusWalkNode::Expr(start));
                }
                ForKind::In { collection, step } => {
                    if let Some(step) = step {
                        stack.push(CorpusWalkNode::Expr(step));
                    }
                    stack.push(CorpusWalkNode::Expr(collection));
                }
            },
            CorpusWalkNode::BindPattern(pattern) => {
                if let BindPattern::Refutable {
                    pattern, fallback, ..
                } = pattern
                {
                    stack.push(CorpusWalkNode::Fallback(fallback));
                    stack.push(CorpusWalkNode::Pattern(pattern));
                }
            }
            CorpusWalkNode::Pattern(pattern) => match pattern {
                Pattern::Or(alternatives, _) => {
                    for alternative in alternatives.iter().rev() {
                        stack.push(CorpusWalkNode::Pattern(alternative));
                    }
                }
                Pattern::Struct { fields, .. } => {
                    for field in fields.iter().rev() {
                        if let StructPatField::Value { value, .. } = field {
                            stack.push(CorpusWalkNode::Expr(value));
                        }
                    }
                }
                Pattern::Variant { .. }
                | Pattern::Present { .. }
                | Pattern::Absent(..)
                | Pattern::Ok { .. }
                | Pattern::Err { .. }
                | Pattern::Range { .. }
                | Pattern::StrMatch { .. }
                | Pattern::BinMatch { .. } => {}
            },
            CorpusWalkNode::Fallback(fallback) => match fallback {
                OrFallback::Value(value) => stack.push(CorpusWalkNode::Expr(value)),
                OrFallback::Block { body, value, .. } => {
                    if let Some(value) = value {
                        stack.push(CorpusWalkNode::Expr(value));
                    }
                    push_corpus_statements(&mut stack, body);
                }
                OrFallback::Return(Some(value), _) => {
                    stack.push(CorpusWalkNode::Expr(value));
                }
                OrFallback::Panic { args, .. } => push_corpus_args(&mut stack, args),
                OrFallback::Return(None, _)
                | OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
            },
        }
    }
}

fn program_expressions<'a>(program: &'a Program) -> Vec<&'a Expr> {
    let mut expressions = Vec::new();
    for_each_statement_expr(&program.script_body, |expr| expressions.push(expr));
    for item in &program.items {
        match item {
            Item::Func(function) => for_each_statement_expr(&function.body, |expr| expressions.push(expr)),
            Item::Struct(structure) => {
                for method in &structure.methods {
                    for_each_statement_expr(&method.body, |expr| expressions.push(expr));
                }
            }
            _ => {}
        }
    }
    expressions
}

#[derive(Debug, Default)]
struct StatementFacts {
    task_groups: Vec<(Span, bool)>,
    counted_loops: Vec<(Span, bool)>,
    manual_counter_loops: Vec<Span>,
    file_walks: Vec<Span>,
}

fn collect_program_statement_facts(
    program: &Program,
    aliases: &BTreeMap<String, String>,
    facts: &mut StatementFacts,
) {
    collect_statement_facts(&program.script_body, aliases, facts);
    for item in &program.items {
        match item {
            Item::Func(function) => collect_statement_facts(&function.body, aliases, facts),
            Item::Struct(structure) => {
                for method in &structure.methods {
                    collect_statement_facts(&method.body, aliases, facts);
                }
            }
            _ => {}
        }
    }
}

/// Collect statement-level facts once with an explicit body stack. Expression
/// facts use the corpus walk above; neither traversal consumes call-stack depth
/// proportional to the maintained source shape.
fn collect_statement_facts<'a>(
    body: &'a [Stmt],
    aliases: &BTreeMap<String, String>,
    facts: &mut StatementFacts,
) {
    let mut bodies = vec![body];
    while let Some(body) = bodies.pop() {
        for pair in body.windows(2) {
            let Some(binding) = (match &pair[0] {
                Stmt::Val(binding) => Some(binding),
                _ => None,
            }) else {
                continue;
            };
            let Stmt::For {
                kind: ForKind::In { .. },
                body: loop_body,
                span,
                ..
            } = &pair[1]
            else {
                continue;
            };
            if binding.mutable
                && matches!(binding.init.without_parens(), Expr::Int(value, ..) if *value == 0)
                && manual_counter_body(&binding.name, loop_body)
            {
                facts.manual_counter_loops.push(*span);
            }
        }

        if let Some(span) = body_file_walk_filter(body, aliases) {
            facts.file_walks.push(span);
        }

        for statement in body.iter().rev() {
            match statement {
                Stmt::TaskGroup {
                    span,
                    body: task_body,
                    ..
                } => {
                    let is_single_combinator = task_body.len() == 1
                        && task_body.first().is_some_and(is_task_combinator_statement);
                    facts.task_groups.push((*span, is_single_combinator));
                    bodies.push(task_body);
                }
                Stmt::CountedLoop {
                    span, init, step, body, ..
                } => {
                    facts
                        .counted_loops
                        .push((*span, counted_loop_indexes_sequence(init, body)));
                    bodies.push(body);
                    if let Some(step) = step {
                        bodies.push(std::slice::from_ref(step.as_ref()));
                    }
                }
                Stmt::While { body, .. }
                | Stmt::For { body, .. }
                | Stmt::Loop { body, .. }
                | Stmt::Unsafe { body, .. }
                | Stmt::Impure { body, .. }
                | Stmt::Reactive { body, .. }
                | Stmt::Shield { body, .. }
                | Stmt::Switched { body, .. }
                | Stmt::Region { body, .. }
                | Stmt::Policy { body, .. }
                | Stmt::Layout { body, .. }
                | Stmt::AuthorityScope { body, .. }
                | Stmt::ComptimeBlock { body, .. }
                | Stmt::ContextBlock { body, .. }
                | Stmt::Live { body, .. }
                | Stmt::AssumeDet { body, .. }
                | Stmt::Transact { body, .. }
                | Stmt::ScopeMember { body, .. } => bodies.push(body),
                Stmt::Switch {
                    arms, else_body, ..
                }
                | Stmt::ComptimeSwitch {
                    arms, else_body, ..
                } => {
                    if let Some(body) = else_body {
                        bodies.push(body);
                    }
                    for arm in arms.iter().rev() {
                        bodies.push(&arm.body);
                    }
                }
                Stmt::ComptimeIf {
                    then_body, else_body, ..
                } => {
                    if let Some(body) = else_body {
                        bodies.push(body);
                    }
                    bodies.push(then_body);
                }
                Stmt::Expr(_)
                | Stmt::Val(_)
                | Stmt::Assign { .. }
                | Stmt::Return(..)
                | Stmt::Break(..)
                | Stmt::BreakValue(..)
                | Stmt::Continue(..)
                | Stmt::BreakLabel(..)
                | Stmt::BreakLabelValue(..)
                | Stmt::ContinueLabel(..)
                | Stmt::Yield(..)
                | Stmt::DeferClose { .. } => {}
            }
        }
    }
}

fn proof_matches(proof: &str, gate: &str) -> bool {
    gate == "all"
        || proof == gate
        || (gate == "golden" && proof == "golden-and-dev-corpus")
        || (gate == "dev-corpus" && proof == "golden-and-dev-corpus")
        || (gate == "suite" && proof == "suite-goldens")
        || (gate == "package" && proof == "package-template-gate")
        || (gate == "dogfood" && proof == "dogfood-gates")
        || (gate == "dogfood" && proof == "dogfood/jetpack and workload gates")
        || (gate == "fixture" && proof == "ui-snapshot-gate")
        || (gate == "fixture" && proof == "fuzz-gate")
        || (gate == "docs" && proof == "doc-gate")
        || (gate == "site" && proof == "site-gate")
        || (gate == "bindgen" && proof == "generator-IR-renderer-registry")
        || (gate == "bindgen" && proof == "bindgen-registry")
        || (gate == "conformance" && proof == "conformance-gate")
        || (gate == "workload" && proof == "compiled-workload-gate")
        || (gate == "workload" && proof == "workload-gate")
}

fn skip_directory(name: &str) -> bool {
    matches!(name, ".git" | ".claude" | ".agent-worktrees" | "plugins" | "node_modules" | "target")
        || name.starts_with("target-")
}

fn discover_manifest_files(
    root: &Path,
    manifest: &CorpusManifest,
    out: &mut Vec<String>,
) -> Result<(), String> {
    for source in &manifest.sources {
        if let Some(path) = source.selector.strip_prefix("root:") {
            discover_files(root, &root.join(path), out)?;
        } else if let Some(path) = source.selector.strip_prefix("file:") {
            if root.join(path).is_file() {
                out.push(path.to_string());
            }
        }
    }
    Ok(())
}

fn discover_files(
    root: &Path,
    current: &Path,
    out: &mut Vec<String>,
) -> Result<(), String> {
    // Keep directory traversal off the call stack. Symlinked directories are
    // excluded before they can re-enter a manifest root.
    let mut pending = vec![current.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let metadata = fs::symlink_metadata(&directory)
            .map_err(|error| format!("stat {}: {error}", directory.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("read {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("read directory entry: {error}"))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("stat {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(skip_directory)
                {
                    continue;
                }
                pending.push(path);
            } else if file_type.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some("jet")
            {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| format!("relative path {}: {error}", path.display()))?
                    .to_str()
                    .ok_or_else(|| format!("non-UTF8 corpus path: {}", path.display()))?
                    .to_string();
                out.push(relative);
            }
        }
    }
    Ok(())
}

fn is_jet_program(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) != Some("package.jet")
}

fn jet_fences(source: &str) -> Vec<String> {
    markdown_fences(source, "jet")
}

fn shell_fences(source: &str) -> Vec<String> {
    let mut fences = markdown_fences(source, "bash");
    fences.extend(markdown_fences(source, "sh"));
    fences.extend(markdown_fences(source, "shell"));
    fences
}

fn markdown_fences(source: &str, language: &str) -> Vec<String> {
    let mut fences = Vec::new();
    let mut current = None;
    let opener = format!("```{language}");
    for line in source.lines() {
        let trimmed = line.trim();
        if current.is_none() {
            if trimmed == opener.as_str() {
                current = Some(String::new());
            }
        } else if trimmed == "```" {
            fences.push(current.take().unwrap_or_default());
        } else if let Some(code) = &mut current {
            code.push_str(line);
            code.push('\n');
        }
    }
    fences
}

fn has_jet_run_command(source: &str) -> bool {
    shell_fences(source).iter().any(|fence| {
        fence.lines().any(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return false;
            }
            let mut words = line.split_whitespace().take_while(|word| *word != "#");
            while let Some(word) = words.next() {
                if word == "jet" && words.next() == Some("run") {
                    return true;
                }
            }
            false
        })
    })
}

fn parse_program(source: &str) -> Result<Program, String> {
    jet_foundation::CompilerStack::run_on_compiler_stack(|| {
        let (tokens, lexer_diagnostics) = jet::Lexer::lex(source);
        if !lexer_diagnostics.is_empty() {
            return Err(format!("Jet lexer rejected corpus source: {lexer_diagnostics:?}"));
        }
        jet::Parser::parse(&tokens)
            .map_err(|diagnostics| format!("Jet parser rejected corpus source: {diagnostics:?}"))
    })
}

fn evaluate_program(
    manifest: &CorpusManifest,
    path: &str,
    row: &SourceRow,
    program: &Program,
) -> Vec<SemanticViolation> {
    let mut violations = Vec::new();
    let aliases = module_aliases(program);
    let expressions = program_expressions(program);
    let mut statement_facts = StatementFacts::default();
    collect_program_statement_facts(program, &aliases, &mut statement_facts);
    let mut argv_sites = Vec::new();
    let mut functions = Vec::new();
    let mut structures = Vec::new();
    for item in &program.items {
        match item {
            Item::Func(function) => {
                functions.push(function);
            }
            Item::Struct(structure) => {
                structures.push(structure);
            }
            _ => {}
        }
    }

    if applies(manifest, path, row, "first-hour-doc-recipe")
        && path == "examples/features/types/typed_literal_forms.jet"
    {
        if let Some(expression) = expressions.iter().find(|expr| is_build_command_literal(expr)) {
            violations.push(SemanticViolation::new(
                path,
                "first-hour-doc-recipe",
                site("literal:jet-build", expression.span()),
            ));
        }
    }

    for expr in &expressions {
        if is_process_argv(expr, &aliases) {
            argv_sites.push(expr.span());
        }
        if applies(manifest, path, row, "raw-cli-fixed-shape") && is_process_argv(expr, &aliases) {
            violations.push(SemanticViolation::new(path, "raw-cli-fixed-shape", site("call:process.argv", expr.span())));
        }
        if applies(manifest, path, row, "raw-cli-builder-shape") && is_process_argv_view(expr, &aliases) {
            violations.push(SemanticViolation::new(path, "raw-cli-builder-shape", site("call:process.argv.skip", expr.span())));
        }
        if applies(manifest, path, row, "duration-constant-safe") && is_constant_duration_constructor(expr, &aliases) {
            violations.push(SemanticViolation::new(path, "duration-constant-safe", site("call:Duration.constructor", expr.span())));
        }
        if applies(manifest, path, row, "unit-scalar-rewrap") && is_unit_scalar_rewrap(expr, &aliases) {
            violations.push(SemanticViolation::new(path, "unit-scalar-rewrap", site("call:unit-rewrap", expr.span())));
        }
        if applies(manifest, path, row, "readonly-copy") && is_readonly_copy(expr, &aliases) {
            violations.push(SemanticViolation::new(path, "readonly-copy", site("call:readonly-copy", expr.span())));
        }
        if applies(manifest, path, row, "error-identity") && is_identity_fallback(expr, &aliases) {
            violations.push(SemanticViolation::new(path, "error-identity", site("expr:error-propagation", expr.span())));
        }
        if applies(manifest, path, row, "http-wrapper-json") && is_http_ceremony(expr, &functions) {
            violations.push(SemanticViolation::new(path, "http-wrapper-json", site("call:http-wrapper", expr.span())));
        }
        if applies(manifest, path, row, "http-message-text") && is_http_message_text(expr) {
            violations.push(SemanticViolation::new(path, "http-message-text", site("call:response.text", expr.span())));
        }
        if applies(manifest, path, row, "http-typed-json") && is_raw_http_json_body(expr, &functions) {
            violations.push(SemanticViolation::new(path, "http-typed-json", site("call:request.body", expr.span())));
        }
        if applies(manifest, path, row, "delimited-reader-config") && is_raw_delimited_split(expr) {
            violations.push(SemanticViolation::new(path, "delimited-reader-config", site("call:delimited-split", expr.span())));
        }
        if applies(manifest, path, row, "plain-format-fact")
            && is_redundant_fixed_cleanup(expr)
        {
            violations.push(SemanticViolation::new(
                path,
                "plain-format-fact",
                site("call:Fixed.replace", expr.span()),
            ));
        }
        if applies(manifest, path, row, "dogfood-path-containment") && is_string_prefix_containment(expr) {
            violations.push(SemanticViolation::new(path, "dogfood-path-containment", site("call:path.starts_with", expr.span())));
        }
        if applies(manifest, path, row, "dogfood-json") && is_hand_json_wire(expr) {
            violations.push(SemanticViolation::new(path, "dogfood-json", site("literal:json-wire", expr.span())));
        }
        if applies(manifest, path, row, "dogfood-url-query") && is_url_query_reparse(expr) {
            violations.push(SemanticViolation::new(path, "dogfood-url-query", site("call:url.query.split", expr.span())));
        }
        if applies(manifest, path, row, "dogfood-list-equality") && is_named_list_equality(expr) {
            violations.push(SemanticViolation::new(path, "dogfood-list-equality", site("fn:same_strings", expr.span())));
        }
        if applies(manifest, path, row, "dogfood-directory-setup") && is_non_idempotent_directory_call(expr, &aliases) {
            violations.push(SemanticViolation::new(path, "dogfood-directory-setup", site("call:fs.create_dir", expr.span())));
        }
        if applies(manifest, path, row, "dogfood-ascii") && is_ascii_replacement_chain(expr) {
            violations.push(SemanticViolation::new(path, "dogfood-ascii", site("call:String.replace", expr.span())));
        }
        if applies(manifest, path, row, "crypto-naming-fact")
            && is_legacy_digest_call(expr, &aliases)
        {
            violations.push(SemanticViolation::new(
                path,
                "crypto-naming-fact",
                site("call:crypto.sha256_bytes", expr.span()),
            ));
        }
    }

    if applies(manifest, path, row, "raw-cli-process-boundary") && argv_sites.len() > 1 {
        for span in argv_sites.into_iter().skip(1) {
            violations.push(SemanticViolation::new(path, "raw-cli-process-boundary", site("call:process.argv", span)));
        }
    }
    if applies(manifest, path, row, "entry-implicit")
        && program.items.iter().any(|item| matches!(item, Item::Func(function) if function.name == "run"))
    {
        let span = program
            .items
            .iter()
            .find_map(|item| match item { Item::Func(function) if function.name == "run" => Some(function.span), _ => None })
            .unwrap_or(Span { start: 0, end: 0 });
        violations.push(SemanticViolation::new(path, "entry-implicit", site("fn:run", span)));
    }
    if applies(manifest, path, row, "codable-structural") {
        for structure in structures {
            if bare_structural_codable(structure) {
                violations.push(SemanticViolation::new(path, "codable-structural", site(&format!("type:{}", structure.name), structure.name_span)));
            }
        }
    }
    if applies(manifest, path, row, "effect-row-inference") {
        for function in &functions {
            if function.name == "run" && redundant_effect_row(function, &aliases) {
                violations.push(SemanticViolation::new(
                    path,
                    "effect-row-inference",
                    site(&format!("fn:{}", function.name), function.span),
                ));
            }
        }
    }
    if applies(manifest, path, row, "task-one-child") {
        for (span, is_single_combinator) in statement_facts.task_groups {
            if is_single_combinator {
                violations.push(SemanticViolation::new(path, "task-one-child", site("stmt:task.group", span)));
            }
        }
    }
    if applies(manifest, path, row, "indexed-sequence") {
        for span in statement_facts.manual_counter_loops {
            violations.push(SemanticViolation::new(
                path,
                "indexed-sequence",
                site("loop:manual-counter", span),
            ));
        }
        for (span, is_sequence_index) in statement_facts.counted_loops {
            if is_sequence_index {
                violations.push(SemanticViolation::new(path, "indexed-sequence", site("loop:counted-sequence", span)));
            }
        }
    }
    if applies(manifest, path, row, "datatree-domain-policy") {
        violations.extend(datatree_domain_policy_violations(path, program));
    }
    if applies(manifest, path, row, "walk-files-filter") {
        if let Some(span) = statement_facts.file_walks.first().copied() {
            violations.push(SemanticViolation::new(
                path,
                "walk-files-filter",
                site("call:fs.walk", span),
            ));
        }
    }
    if applies(manifest, path, row, "regex-capture-recipe")
        && !has_anchored_regex_capture(program, &aliases)
    {
        let span = functions
            .first()
            .map_or(Span { start: 0, end: 0 }, |function| function.span);
        violations.push(SemanticViolation::new(
            path,
            "regex-capture-recipe",
            site("recipe:regex-capture", span),
        ));
    }
    if applies(manifest, path, row, "http-unused-request") {
        for function in &functions {
            if function.params.iter().any(|parameter| {
                matches!(&parameter.ty, Type::Named(name) if name == "HTTPRequest")
                    && has_unused_parameter(function, &parameter.name)
            }) {
                violations.push(SemanticViolation::new(
                    path,
                    "http-unused-request",
                    site("param:req", function.span),
                ));
            }
        }
    }
    violations
}

fn applies(manifest: &CorpusManifest, path: &str, row: &SourceRow, rule: &str) -> bool {
    if !semantic_role(row.role) {
        return false;
    }
    if matches!(rule, "raw-cli-fixed-shape" | "raw-cli-builder-shape")
        && !matches!(row.profile.as_str(), "typed-cli" | "typed-cli-doc")
    {
        return false;
    }
    manifest
        .scopes
        .iter()
        .any(|scope| scope.rule == rule && selector_matches(&scope.selector, path))
}

fn maintained_guidance_lint(name: &str) -> bool {
    matches!(
        name,
        "process_args_view"
            | "message_text"
            | "redundant_fixed_cleanup"
            | "unit_scalar_rewrap"
            | "path_containment_string_prefix"
            | "complete_ascii_case_ladder"
            | "walk_files_filter"
    )
}

fn redundant_effect_row(function: &jet::AST::Func, aliases: &BTreeMap<String, String>) -> bool {
    let Some(declared) = function.declared_effects.as_ref() else {
        return false;
    };
    if declared.is_empty() {
        return false;
    }
    let mut inferred = BTreeSet::new();
    for_each_statement_expr(&function.body, |expr| {
        if let Some(effect) = expression_effect(expr, aliases) {
            inferred.insert(effect);
        }
    });
    !inferred.is_empty()
        && declared.iter().all(|(name, _)| {
            inferred
                .iter()
                .any(|effect| effect.eq_ignore_ascii_case(name))
        })
        && inferred.len() == declared.len()
}

fn expression_effect(expr: &Expr, aliases: &BTreeMap<String, String>) -> Option<&'static str> {
    match expr.without_parens() {
        Expr::Call(call) => {
            if matches!(call.name.as_str(), "print" | "println" | "eprint" | "eprintln") {
                return Some("IO");
            }
            let (module, _) = call.name.rsplit_once('.')?;
            effect_for_module(module, aliases)
        }
        Expr::MethodCall { receiver, .. } => {
            let module = match receiver.without_parens() {
                Expr::Ident(name, _) => name.as_str(),
                _ => return None,
            };
            effect_for_module(module, aliases)
        }
        _ => None,
    }
}

fn effect_for_module(module: &str, aliases: &BTreeMap<String, String>) -> Option<&'static str> {
    let resolved = aliases.get(module).map(String::as_str).unwrap_or(module);
    match resolved {
        "core.files" | "core.fs" => Some("FS"),
        "core.process" | "core.exec" => Some("Exec"),
        "core.time" => Some("Time"),
        "core.net" | "core.net.url" | "core.http" | "core.http.client" => Some("Net"),
        "core.io" => Some("IO"),
        _ => None,
    }
}

fn module_aliases(program: &Program) -> BTreeMap<String, String> {
    program
        .imports
        .iter()
        .filter_map(|import| import.core_module_path().map(|module| (import.import_alias(), module)))
        .collect()
}

fn counted_loop_indexes_sequence(init: &jet::AST::Binding, body: &[Stmt]) -> bool {
    let mut indexes_sequence = false;
    for_each_statement_expr(body, |expr| {
        if let Expr::Index { index, .. } = expr.without_parens() {
            if matches!(index.without_parens(), Expr::Ident(name, _) if name == &init.name) {
                indexes_sequence = true;
            }
        }
    });
    indexes_sequence
}

fn manual_counter_body(counter: &str, body: &[Stmt]) -> bool {
    let mut increments = 0;
    let mut used = false;
    for statement in body {
        if matches!(
            statement,
            Stmt::Assign {
                target: jet::AST::LValue::Local { name, .. },
                op: Some(BinOp::Add),
                value,
                ..
            } if name == counter
                && matches!(value.without_parens(), Expr::Int(value, ..) if *value == 1)
        ) {
            increments += 1;
            continue;
        }
        for_each_statement_expr(std::slice::from_ref(statement), |expr| {
            if matches!(expr.without_parens(), Expr::Ident(name, _) if name == counter) {
                used = true;
            }
        });
    }
    increments == 1 && used
}

fn named_type(ty: &Type, expected: &str) -> bool {
    matches!(ty, Type::Named(name) if name == expected)
        || matches!(
            (ty, expected),
            (Type::Bool, "Bool") | (Type::String, "String") | (Type::Int, "Int")
        )
}

fn datatree_domain_policy_violations(path: &str, program: &Program) -> Vec<SemanticViolation> {
    let mut violations = Vec::new();
    for item in &program.items {
        let Item::Func(function) = item else {
            continue;
        };
        if is_named_tower_truthiness_policy(path, &function.name) {
            continue;
        }
        let returns_bool = function
            .return_type
            .as_ref()
            .is_some_and(|ty| named_type(ty, "Bool"));
        let data_tree_pair = function.params.len() == 2
            && function
                .params
                .iter()
                .all(|parameter| data_tree_domain_value(&parameter.ty));
        if returns_bool
            && data_tree_pair
            && (body_returns_constant_bool(&function.body)
                || compares_parameter_pair(function))
        {
            violations.push(SemanticViolation::new(
                path,
                "datatree-domain-policy",
                site(&format!("fn:{}", function.name), function.span),
            ));
            continue;
        }
        if function.params.len() == 1
            && returns_bool
            && named_type(&function.params[0].ty, "DataTree")
            && body_returns_constant_bool(&function.body)
        {
            violations.push(SemanticViolation::new(
                path,
                "datatree-domain-policy",
                site(&format!("fn:{}", function.name), function.span),
            ));
        }
    }
    violations
}

/// This is the one occurrence-scoped domain reason: Tower preserves its
/// JavaScript truthiness policy in these exact named functions.
fn is_named_tower_truthiness_policy(path: &str, name: &str) -> bool {
    path == "dogfood/tower/run.jet"
        && matches!(
            name,
            "javascript_truthy"
                | "javascript_truthy_null"
                | "javascript_truthy_bool"
                | "javascript_truthy_int"
                | "javascript_truthy_float"
                | "javascript_truthy_text"
                | "javascript_truthy_array"
                | "javascript_truthy_object"
        )
}

fn body_returns_constant_bool(body: &[Stmt]) -> bool {
    body.len() == 1
        && match &body[0] {
            Stmt::Return(Some(expr), _) | Stmt::Expr(expr) => {
                matches!(expr.without_parens(), Expr::Bool(..))
            }
            _ => false,
        }
}

fn data_tree_domain_value(ty: &Type) -> bool {
    named_type(ty, "DataTree")
        || matches!(ty, Type::List(inner) if named_type(inner, "DataTree"))
}

fn compares_parameter_pair(function: &jet::AST::Func) -> bool {
    let Some(left_name) = function.params.first().map(|param| param.name.as_str()) else {
        return false;
    };
    let Some(right_name) = function.params.get(1).map(|param| param.name.as_str()) else {
        return false;
    };
    let mut found = false;
    for_each_statement_expr(&function.body, |expr| {
        let Expr::Binary(BinOp::Eq, left, right, _) = expr.without_parens() else {
            return;
        };
        let direct = |value: &Expr, expected: &str| {
            matches!(value.without_parens(), Expr::Ident(name, _) if name == expected)
        };
        if (direct(left, left_name) && direct(right, right_name))
            || (direct(left, right_name) && direct(right, left_name))
        {
            found = true;
        }
    });
    found
}

fn receiver_is(expr: &Expr, expected: &str, aliases: &BTreeMap<String, String>) -> bool {
    match expr.without_parens() {
        Expr::Ident(name, _) if name == expected => true,
        Expr::Ident(name, _) => aliases.get(name).is_some_and(|module| module == &format!("core.{expected}")),
        _ => false,
    }
}

fn is_process_argv(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    matches!(
        expr.without_parens(),
        Expr::MethodCall { receiver, method, .. } if method == "argv" && receiver_is(receiver, "process", aliases)
    )
}

fn is_process_argv_view(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = expr.without_parens()
    else {
        return false;
    };
    method == "skip"
        && args.first().is_some_and(|arg| {
            matches!(arg.expr.without_parens(), Expr::Int(value, ..) if *value == 1)
        })
        && is_process_argv(receiver, aliases)
}

fn is_legacy_digest_call(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    let Expr::MethodCall {
        receiver,
        method,
        ..
    } = expr.without_parens()
    else {
        return false;
    };
    method == "sha256_bytes"
        && matches!(
            receiver.without_parens(),
            Expr::Ident(name, _) if aliases
                .get(name)
                .is_some_and(|module| module == "core.crypto")
        )
}

fn is_constant_duration_constructor(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    let Expr::MethodCall { receiver, method, args, .. } = expr.without_parens() else {
        return false;
    };
    if !receiver_is(receiver, "Duration", aliases) || !matches!(method.as_str(), "nanoseconds" | "microseconds" | "milliseconds" | "seconds" | "minutes" | "hours" | "days") {
        return false;
    }
    let Some(argument) = args.first() else {
        return false;
    };
    is_nonnegative_numeric_literal(&argument.expr) && is_constant_expr(&argument.expr)
}

fn is_nonnegative_numeric_literal(expr: &Expr) -> bool {
    match expr.without_parens() {
        Expr::Int(value, ..) => *value >= 0 && *value < i64::MAX,
        Expr::Float(value, ..) => value.is_finite() && *value >= 0.0 && *value < i64::MAX as f64,
        _ => false,
    }
}

fn is_constant_expr(expr: &Expr) -> bool {
    let mut expressions = vec![expr];
    while let Some(expr) = expressions.pop() {
        match expr.without_parens() {
            Expr::Str(parts, _) if parts.iter().all(|part| matches!(part, StrPart::Lit(_))) => {}
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::UnitLit { .. } => {}
            Expr::ListLit(items, _) => expressions.extend(items),
            Expr::TupleLit(items, _, _) => {
                expressions.extend(items.iter().map(|(_, item)| item));
            }
            Expr::MapLit(entries, _) => {
                for (key, value) in entries {
                    expressions.push(key);
                    expressions.push(value);
                }
            }
            Expr::Unary(_, inner, _) => expressions.push(inner),
            Expr::Binary(_, left, right, _) => {
                expressions.push(left);
                expressions.push(right);
            }
            _ => return false,
        }
    }
    true
}

fn is_redundant_fixed_cleanup(expr: &Expr) -> bool {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = expr.without_parens()
    else {
        return false;
    };
    method == "replace"
        && args.len() == 2
        && is_literal_string(&args[0].expr, ",")
        && is_literal_string(&args[1].expr, "")
        && is_plain_fixed_projection(receiver)
}

fn is_plain_fixed_projection(expr: &Expr) -> bool {
    matches!(
        expr.without_parens(),
        Expr::Str(parts, _)
            if parts.len() == 1
                && matches!(&parts[0], StrPart::Interp(_, StrFormat::Fixed(_)))
    )
}

fn is_literal_string(expr: &Expr, expected: &str) -> bool {
    matches!(
        expr.without_parens(),
        Expr::Str(parts, _)
            if parts.len() == 1 && matches!(&parts[0], StrPart::Lit(text) if text == expected)
    )
}

fn is_unit_scalar_rewrap(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = expr.without_parens()
    else {
        return false;
    };
    let is_constructor = method.starts_with("from_")
        || (receiver_is(receiver, "Duration", aliases)
            && matches!(method.as_str(), "nanoseconds" | "microseconds" | "milliseconds" | "seconds" | "minutes" | "hours" | "days"));
    if !is_constructor || args.len() != 1 {
        return false;
    }
    let Expr::Binary(operator, left, right, _) = args[0].expr.without_parens() else {
        return false;
    };
    if !matches!(operator, BinOp::Mul | BinOp::Div) {
        return false;
    }
    (is_unit_projection(left) && is_direct_scalar(right))
        || (matches!(operator, BinOp::Mul)
            && is_direct_scalar(left)
            && is_unit_projection(right))
}

fn is_unit_projection(expr: &Expr) -> bool {
    matches!(
        expr.without_parens(),
        Expr::MethodCall { method, args, .. }
            if args.is_empty() && method == "raw"
    ) || matches!(
        expr.without_parens(),
        Expr::MethodCall { method, .. } if method == "in"
    )
}

fn is_direct_scalar(expr: &Expr) -> bool {
    let mut current = expr;
    loop {
        match current.without_parens() {
            Expr::Int(..) | Expr::Float(..) | Expr::Ident(..) => return true,
            Expr::Unary(UnOp::Neg, inner, _) => {
                return matches!(inner.without_parens(), Expr::Int(..) | Expr::Float(..));
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } if matches!(receiver.without_parens(), Expr::Ident(name, _) if name == "Float")
                && method == "from_int"
                && args.len() == 1 => current = &args[0].expr,
            _ => return false,
        }
    }
}

fn is_readonly_copy(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    let (module, method, args) = match expr.without_parens() {
        Expr::Call(call) => {
            let Some((module, method)) = call.name.rsplit_once('.') else {
                return false;
            };
            (module, method, &call.args)
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } if receiver_is(receiver, "files", aliases) => ("files", method.as_str(), args),
        _ => return false,
    };
    let module_is_files = module == "fs"
        || module == "files"
        || aliases
            .get(module)
            .is_some_and(|path| path == "core.files");
    module_is_files
        && matches!(method, "read" | "read_bytes" | "read_at" | "read_link" | "stat" | "list_dir")
        && args.iter().any(|arg| {
            arg.convention == AccessConvention::Read
                && matches!(arg.expr.without_parens(), Expr::Copy(_, _))
        })
}

fn is_identity_fallback(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    let Expr::OrFallback {
        value, fallback, ..
    } = expr.without_parens()
    else {
        return false;
    };
    let jet::AST::OrFallback::Return(error, _) = fallback else {
        return false;
    };
    let unchanged_error = error.as_deref().is_none_or(is_err_constructor);
    unchanged_error && is_io_call(value, aliases)
}

fn is_err_constructor(expr: &Expr) -> bool {
    matches!(
        expr.without_parens(),
        Expr::Call(call) if call.name == "Err"
    ) || matches!(
        expr.without_parens(),
        Expr::EnumLit { variant, .. } if variant == "Err"
    ) || matches!(expr.without_parens(), Expr::Err(_, _))
}

fn is_io_call(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    match expr.without_parens() {
        Expr::Call(call) => {
            let Some((module, _)) = call.name.rsplit_once('.') else {
                return false;
            };
            is_io_module(module, aliases)
        }
        Expr::MethodCall { receiver, .. } => {
            receiver_is(receiver, "files", aliases)
                || receiver_is(receiver, "io", aliases)
                || receiver_is(receiver, "env", aliases)
        }
        _ => false,
    }
}

fn is_io_module(module: &str, aliases: &BTreeMap<String, String>) -> bool {
    matches!(module, "fs" | "files" | "io" | "env")
        || aliases.get(module).is_some_and(|path| {
            matches!(path.as_str(), "core.files" | "core.io" | "core.env")
        })
}

fn is_http_ceremony(expr: &Expr, functions: &[&jet::AST::Func]) -> bool {
    matches!(
        expr.without_parens(),
        Expr::MethodCall { receiver, method, args, .. }
            if method == "body" && !args.is_empty() && is_http_request_value(receiver, functions)
    )
}

fn is_http_message_text(expr: &Expr) -> bool {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = expr.without_parens()
    else {
        return false;
    };
    method == "text"
        && !args.is_empty()
        && matches!(
            receiver.without_parens(),
            Expr::MethodCall { method, .. } if method == "body"
        )
}

fn is_raw_http_json_body(expr: &Expr, functions: &[&jet::AST::Func]) -> bool {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = expr.without_parens()
    else {
        return false;
    };
    method == "body" && !args.is_empty() && is_http_request_value(receiver, functions)
}

fn is_http_request_value(expr: &Expr, functions: &[&jet::AST::Func]) -> bool {
    let Expr::Ident(name, _) = expr.without_parens() else {
        return false;
    };
    functions.iter().any(|function| {
        function.params.iter().any(|parameter| {
            parameter.name == *name && named_type(&parameter.ty, "HTTPRequest")
        })
    })
}

fn is_raw_delimited_split(expr: &Expr) -> bool {
    let Expr::MethodCall { method, args, .. } = expr.without_parens() else {
        return false;
    };
    if method != "split" {
        return false;
    }
    args.first().is_some_and(|arg| matches!(arg.expr.without_parens(), Expr::Str(parts, _) if parts.len() == 1 && matches!(&parts[0], StrPart::Lit(text) if text == "\t")))
}

fn is_build_command_literal(expr: &Expr) -> bool {
    let Expr::Str(parts, _) = expr.without_parens() else {
        return false;
    };
    parts.iter().any(|part| {
        matches!(part, StrPart::Lit(text) if text.contains("jet build"))
    })
}

fn is_hand_json_wire(expr: &Expr) -> bool {
    let Expr::Str(parts, _) = expr.without_parens() else {
        return false;
    };
    let has_interpolation = parts.iter().any(|part| matches!(part, StrPart::Interp(_, _)));
    let has_object_shape = parts.iter().any(|part| {
        matches!(part, StrPart::Lit(text) if text.contains('{') && text.contains(':'))
    });
    has_interpolation && has_object_shape
}

fn is_string_prefix_containment(expr: &Expr) -> bool {
    let Expr::MethodCall {
        method, args, ..
    } = expr.without_parens()
    else {
        return false;
    };
    method == "starts_with"
        && args.first().is_some_and(|arg| {
            matches!(
                arg.expr.without_parens(),
                Expr::Str(parts, _)
                    if parts.iter().any(|part| {
                        matches!(part, StrPart::Interp(_, _))
                            || matches!(part, StrPart::Lit(text) if text.len() > 1 && text.ends_with('/'))
                    })
            )
        })
}

fn is_url_query_reparse(expr: &Expr) -> bool {
    let Expr::MethodCall { method, args, .. } = expr.without_parens() else {
        return false;
    };
    method == "split"
        && args.first().is_some_and(|arg| {
            matches!(
                arg.expr.without_parens(),
                Expr::Str(parts, _)
                    if parts.len() == 1
                        && matches!(&parts[0], StrPart::Lit(text) if text == "&" || text == "=")
            )
        })
}

fn is_named_list_equality(expr: &Expr) -> bool {
    matches!(expr.without_parens(), Expr::Call(call) if call.name == "same_strings")
        || matches!(
            expr.without_parens(),
            Expr::MethodCall { method, .. } if method == "same_strings"
        )
}

fn is_non_idempotent_directory_call(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    match expr.without_parens() {
        Expr::Call(call) => {
            let Some((module, method)) = call.name.rsplit_once('.') else {
                return false;
            };
            method == "create_dir"
                && (module == "fs"
                    || module == "files"
                    || aliases
                        .get(module)
                        .is_some_and(|path| path == "core.files"))
        }
        Expr::MethodCall {
            receiver, method, ..
        } => method == "create_dir" && receiver_is(receiver, "files", aliases),
        _ => false,
    }
}

fn is_ascii_replacement_chain(expr: &Expr) -> bool {
    let Some(pairs) = replacement_chain_pairs(expr) else {
        return false;
    };
    if pairs.len() != 26 {
        return false;
    }
    let lower = ('A'..='Z').zip('a'..='z').collect::<BTreeSet<_>>();
    let upper = ('a'..='z').zip('A'..='Z').collect::<BTreeSet<_>>();
    let actual = pairs.into_iter().collect::<BTreeSet<_>>();
    actual == lower || actual == upper
}

fn replacement_chain_pairs(expr: &Expr) -> Option<Vec<(char, char)>> {
    let mut current = expr;
    let mut pairs = Vec::new();
    loop {
        let Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } = current.without_parens()
        else {
            pairs.reverse();
            return Some(pairs);
        };
        if method != "replace"
            || args.len() != 2
            || !args.iter().all(|arg| is_single_ascii_literal(&arg.expr))
        {
            pairs.reverse();
            return Some(pairs);
        }
        pairs.push((
            single_ascii_literal(&args[0].expr)?,
            single_ascii_literal(&args[1].expr)?,
        ));
        current = receiver;
    }
}

fn is_single_ascii_literal(expr: &Expr) -> bool {
    single_ascii_literal(expr).is_some()
}

fn single_ascii_literal(expr: &Expr) -> Option<char> {
    let Expr::Str(parts, _) = expr.without_parens() else {
        return None;
    };
    if parts.len() != 1 {
        return None;
    }
    let StrPart::Lit(text) = &parts[0] else {
        return None;
    };
    let mut chars = text.chars();
    let character = chars.next()?;
    (chars.next().is_none() && character.is_ascii()).then_some(character)
}

fn is_file_walk(expr: &Expr, aliases: &BTreeMap<String, String>) -> bool {
    matches!(
        expr.without_parens(),
        Expr::MethodCall { receiver, method, .. }
            if method == "walk" && receiver_is(receiver, "files", aliases)
    )
}

fn body_file_walk_filter(body: &[Stmt], aliases: &BTreeMap<String, String>) -> Option<Span> {
    for (index, statement) in body.iter().enumerate() {
        if let Stmt::Val(binding) = statement {
            if let Some(Stmt::For {
                kind: ForKind::In { collection, .. },
                body: loop_body,
                ..
            }) = body.get(index + 1)
            {
                let same_collection = matches!(
                    collection.without_parens(),
                    Expr::Ident(name, _) if name == &binding.name
                );
                if same_collection
                    && is_file_walk(&binding.init, aliases)
                    && body_has_negative_directory_filter(loop_body)
                {
                    return Some(binding.init.span());
                }
            }
        }
        if let Stmt::For {
            kind: ForKind::In { collection, .. },
            body: loop_body,
            ..
        } = statement
        {
            if is_file_walk(collection, aliases) && body_has_negative_directory_filter(loop_body) {
                return Some(collection.span());
            }
        }
    }
    None
}

fn body_has_negative_directory_filter(body: &[Stmt]) -> bool {
    let mut found = false;
    for_each_statement_expr(body, |expr| {
        if is_negative_directory_filter(expr) {
            found = true;
        }
    });
    found
}

fn is_negative_directory_filter(expr: &Expr) -> bool {
    matches!(
        expr.without_parens(),
        Expr::Unary(UnOp::Not, value, _) if is_directory_field(value)
    )
}

fn is_directory_field(expr: &Expr) -> bool {
    matches!(
        expr.without_parens(),
        Expr::Field(_, field, _) if field == "is_dir"
    )
}

fn has_anchored_regex_capture(program: &Program, aliases: &BTreeMap<String, String>) -> bool {
    if body_has_anchored_regex_capture(&program.script_body, aliases) {
        return true;
    }
    for item in &program.items {
        match item {
            Item::Func(function) if body_has_anchored_regex_capture(&function.body, aliases) => {
                return true;
            }
            Item::Struct(structure) => {
                if structure
                    .methods
                    .iter()
                    .any(|method| body_has_anchored_regex_capture(&method.body, aliases))
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn body_has_anchored_regex_capture(body: &[Stmt], aliases: &BTreeMap<String, String>) -> bool {
    let mut expressions = Vec::new();
    for_each_statement_expr(body, |expr| expressions.push(expr));
    let anchored_pattern = expressions.iter().copied().any(|expr| match expr.without_parens() {
        Expr::StructLit { type_name, fields, .. } if type_name == "Regex" => fields
            .first()
            .is_some_and(|(_, _, value)| is_anchored_literal(value)),
        Expr::TypedLit { body, .. } => match body {
            jet::AST::TypedLitBody::Value(value) => is_anchored_literal(value),
            _ => false,
        },
        _ => false,
    });
    let matched = expressions.iter().copied().any(|expr| {
        matches!(expr.without_parens(), Expr::Call(call) if call.name == "re.match")
            || matches!(
                expr.without_parens(),
                Expr::MethodCall { receiver, method, .. }
                    if method == "match"
                        && aliases
                            .get(match receiver.without_parens() {
                                Expr::Ident(name, _) => name.as_str(),
                                _ => "",
                            })
                            .is_some_and(|module| module == "core.regex")
            )
    });
    let captured = expressions.iter().copied().any(|expr| {
        matches!(
            expr.without_parens(),
            Expr::MethodCall { method, .. } if method == "group"
        )
    });
    anchored_pattern && matched && captured
}

fn is_anchored_literal(expr: &Expr) -> bool {
    let Expr::Str(parts, _) = expr.without_parens() else {
        return false;
    };
    let Some(StrPart::Lit(text)) = parts.first() else {
        return false;
    };
    parts.len() == 1 && text.starts_with('^') && text.ends_with('$')
}

fn has_unused_parameter(function: &jet::AST::Func, parameter: &str) -> bool {
    if !function.params.iter().any(|param| param.name == parameter) {
        return false;
    }
    let mut used = false;
    for_each_statement_expr(&function.body, |expr| {
        if matches!(expr.without_parens(), Expr::Ident(name, _) if name == parameter) {
            used = true;
        }
    });
    !used
}

fn is_task_combinator_statement(statement: &Stmt) -> bool {
    match statement {
        Stmt::Expr(expr) => is_task_combinator_expr(expr),
        Stmt::Val(binding) => is_task_combinator_expr(&binding.init),
        _ => false,
    }
}

fn is_task_combinator_expr(expr: &Expr) -> bool {
    let mut current = expr;
    loop {
        match current.without_parens() {
            Expr::Call(call) => {
                return matches!(
                    call.name.as_str(),
                    "task.race"
                        | "task.any"
                        | jet::Syntax::INTERNAL_TASK_RACE_METHOD
                        | jet::Syntax::INTERNAL_TASK_ANY_METHOD
                );
            }
            Expr::MethodCall { receiver, method, .. } => {
                return matches!(receiver.without_parens(), Expr::Ident(name, _)
                    if name == "task" || name == jet::Syntax::INTERNAL_TASK_RECEIVER)
                    && matches!(
                        method.as_str(),
                        "race"
                            | "any"
                            | jet::Syntax::INTERNAL_TASK_RACE_METHOD
                            | jet::Syntax::INTERNAL_TASK_ANY_METHOD
                    );
            }
            Expr::OrFallback { value, .. } => current = value,
            _ => return false,
        }
    }
}

fn bare_structural_codable(structure: &jet::AST::StructDef) -> bool {
    structure.type_params.is_empty()
        && structure.type_markers.len() == 1
        && structure.type_markers.first().is_some_and(|marker| marker.name == "Codable" && !marker.negated)
        && structure.fields.iter().all(|field| field.serde_markers.is_empty() && field.computed.is_none() && field.default.is_none())
        && !structure.is_published_schema
}

fn site(kind: &str, span: Span) -> String {
    format!("{}@{}..{}", kind, span.start, span.end)
}

fn inventory_error(message: &str, file: &str, site_kind: &str) -> String {
    format!(
        "{message}: {file}; {}",
        SemanticViolation::new(
            file,
            "corpus-inventory",
            site(site_kind, Span::new(0, 0)),
        )
    )
}
