//! D-BUILD-NOCHANGE1=A: the invocation Receipt node.
//!
//! A Receipt answers one invocation closure. The closure key frames the verb,
//! the options that change what the verb checks, the compiler and Core
//! identity, the host target, the exact bytes of every module in the
//! immutable source closure, and every package authority file (manifest,
//! payload, workspace, build, lock) that owns one of those modules. A hit
//! re-renders the recorded typed diagnostics for the current terminal mode and
//! executes no `Check` node; the store log says so.
//!
//! Inputs that only the check discovers (compile-time file reads) are carried
//! in the record and re-hashed at use time. A check whose reads are not sealed
//! (audited gates such as `#Impure`, network inputs, a programmable build, or a
//! checked bundle that differs from the hashed closure) is never recorded, so
//! it checks fresh every time.

use jet_store::{BuildNodeRecord, BuildRecord, ReceiptInput, ReceiptRecord, RecordKind, Store};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const CLOSURE_SCHEMA: &str = "jet.receipt-closure/v1";
const AUTHORITY_FILES: [&str; 5] = [
    jet::Syntax::PACKAGE_FILE,
    jet::Syntax::PAYLOAD_FILE,
    jet::Syntax::WORKSPACE_FILE,
    "build.jet",
    jet::Syntax::UNIFIED_LOCK_FILE,
];

/// Store-log reason for a module whose recorded check was replayed.
pub(crate) const WHY_REUSED: &str = "reused";

/// #2517 criteria 2/6: when set, a verified Receipt does not replay; the
/// program is checked fresh and every read that check makes must be declared
/// by the verified record, or the invocation fails as a compiler defect.
pub(crate) const AUDIT_ENV: &str = "JET_CHECK_READS_AUDIT";

pub(crate) fn audit_requested() -> bool {
    std::env::var_os(AUDIT_ENV).is_some()
}

pub(crate) struct ReceiptClosure {
    verb: &'static str,
    key: String,
    project_root: PathBuf,
    program: String,
    modules: Vec<ReceiptInput>,
    sources: BTreeMap<String, String>,
    /// #2517 S1b: every check-time read made after the closure was hashed.
    /// Taken once by `sealed_inputs`; a second take finds nothing and leaves
    /// the invocation unrecorded.
    reads: std::sync::Mutex<Option<jet_foundation::CheckReads::ReadSession>>,
}

fn frame(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}

/// Project-relative label for a path inside the project, the absolute path
/// otherwise. Labels (not checkout paths) enter keys so two checkouts of the
/// same tree share records.
fn label(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The package roots that own the closure: for each module directory, the
/// nearest ancestor holding a package manifest, plus the nearest workspace
/// above the project root.
fn authority_roots(project_root: &Path, source_closure: &[(PathBuf, String)]) -> BTreeSet<PathBuf> {
    let mut roots = BTreeSet::new();
    roots.insert(project_root.to_path_buf());
    let mut visited = BTreeSet::new();
    for (path, _) in source_closure {
        if path.starts_with(jet::Diagnostics::CORE_SOURCE_ROOT) {
            continue;
        }
        let mut dir = path.parent();
        while let Some(current) = dir {
            if !visited.insert(current.to_path_buf()) {
                break;
            }
            if current.join(jet::Syntax::PACKAGE_FILE).is_file()
                || current.join(jet::Syntax::PAYLOAD_FILE).is_file()
            {
                roots.insert(current.to_path_buf());
                break;
            }
            dir = current.parent();
        }
    }
    let mut dir = Some(project_root);
    while let Some(current) = dir {
        if current.join(jet::Syntax::WORKSPACE_FILE).is_file() {
            roots.insert(current.to_path_buf());
            break;
        }
        dir = current.parent();
    }
    roots
}

/// Presence and digest of one authority file. Absence is part of the key: a
/// manifest that appears later changes what the check reads.
fn probe(path: &Path) -> String {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => "symlink".to_string(),
        Ok(metadata) if metadata.is_file() => match fs::read(path) {
            Ok(bytes) => jet::SHA256::sha256_hex(&bytes),
            Err(error) => format!("unreadable:{}", error.kind()),
        },
        Ok(metadata) if metadata.is_dir() => "directory".to_string(),
        Ok(_) => "other".to_string(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(error) => format!("unreadable:{}", error.kind()),
    }
}

impl ReceiptClosure {
    /// `options` names every invocation option that changes what the verb
    /// checks; rendering options (color, width, `--json`, `--verbose`) stay
    /// out because the record is re-rendered per invocation.
    pub(crate) fn new(
        verb: &'static str,
        file: &str,
        project_root: &Path,
        program: String,
        source_closure: &[(PathBuf, String)],
        options: &[(&str, String)],
    ) -> Self {
        let mut key = Vec::new();
        frame(&mut key, CLOSURE_SCHEMA.as_bytes());
        frame(&mut key, verb.as_bytes());
        frame(&mut key, env!("JET_COMPILER_BUILD_ID").as_bytes());
        frame(&mut key, env!("JET_STDLIB_BUILD_ID").as_bytes());
        frame(&mut key, env!("JET_BUILD_TARGET").as_bytes());
        let entry = fs::canonicalize(file).unwrap_or_else(|_| PathBuf::from(file));
        // Recorded diagnostic origins carry process paths, so the checkout
        // root is part of the closure: a replay never names another tree.
        frame(&mut key, b"root");
        frame(&mut key, project_root.to_string_lossy().as_bytes());
        frame(&mut key, b"entry");
        frame(&mut key, label(project_root, &entry).as_bytes());
        let mut options = options.to_vec();
        options.sort();
        for (name, value) in options {
            frame(&mut key, b"option");
            frame(&mut key, name.as_bytes());
            frame(&mut key, value.as_bytes());
        }
        let mut modules = Vec::with_capacity(source_closure.len());
        let mut sources = BTreeMap::new();
        for (path, source) in source_closure {
            let digest = jet::SHA256::sha256_hex(source.as_bytes());
            modules.push(ReceiptInput {
                path: label(project_root, path),
                digest: digest.clone(),
            });
            sources.insert(digest, source.clone());
        }
        modules.sort();
        modules.dedup();
        for module in &modules {
            frame(&mut key, b"module");
            frame(&mut key, module.path.as_bytes());
            frame(&mut key, module.digest.as_bytes());
        }
        for root in authority_roots(project_root, source_closure) {
            for name in AUTHORITY_FILES {
                let path = root.join(name);
                frame(&mut key, b"authority");
                frame(&mut key, label(project_root, &path).as_bytes());
                frame(&mut key, probe(&path).as_bytes());
            }
        }
        Self {
            verb,
            key: jet::SHA256::sha256_hex(&key),
            project_root: project_root.to_path_buf(),
            program,
            modules,
            sources,
            reads: std::sync::Mutex::new(Some(jet_foundation::CheckReads::ReadSession::begin())),
        }
    }

    /// Persisted form of a reader label: paths inside the project become
    /// project-relative so two checkouts share records.
    fn persisted_label(&self, read_label: &str) -> String {
        for kind in ["file:", "dir:"] {
            if let Some(path) = read_label.strip_prefix(kind) {
                return format!("{kind}{}", label(&self.project_root, Path::new(path)));
            }
        }
        read_label.to_string()
    }

    /// The reader label a persisted label names in this checkout.
    fn current_label(&self, label: &str) -> String {
        for kind in ["file:", "dir:"] {
            if let Some(path) = label.strip_prefix(kind) {
                let path = Path::new(path);
                if path.is_relative() {
                    return format!("{kind}{}", self.project_root.join(path).display());
                }
            }
        }
        label.to_string()
    }

    /// The recorded outcome for this closure, when every input the recorded
    /// check discovered still hashes to its recorded digest.
    pub(crate) fn lookup(&self, store: &Store) -> Option<ReceiptRecord> {
        let resolve = |revision: &str| self.sources.get(revision).cloned();
        let record = store
            .receipt(&self.project_root, &self.key, &resolve)
            .ok()
            .flatten()?;
        if record.verb != self.verb || record.modules != self.modules {
            return None;
        }
        let mut stamps = jet_store::project_state::StampTable::load(&self.project_root);
        let current = record.inputs.iter().all(|input| {
            let label = self.current_label(&input.path);
            let digest = match label.strip_prefix("file:") {
                Some(path) => stamps
                    .file_digest(Path::new(path))
                    .or_else(|| jet_foundation::CheckReads::current_digest(&label)),
                None => jet_foundation::CheckReads::current_digest(&label),
            };
            digest.as_deref() == Some(input.digest.as_str())
        });
        // The stamp table only saves rehashing; failing to persist it is safe.
        let _ = stamps.save(&self.project_root);
        current.then_some(record)
    }

    /// The checked bundle must be exactly the hashed closure, and every
    /// check-time read must be declared: a closure module with the hashed
    /// bytes, or a discovered input the record carries and re-verifies before
    /// reuse (#2517 criteria 2/6). A network input, or a read whose contents
    /// changed during the check, is unsealed, and the invocation stays
    /// unrecorded.
    pub(crate) fn sealed_inputs(
        &self,
        bundle: &jet::AST::ProgramBundle,
    ) -> Option<Vec<ReceiptInput>> {
        let reads = self.reads.lock().ok()?.take()?.finish();
        let mut checked = bundle
            .modules
            .iter()
            .map(|module| ReceiptInput {
                path: label(&self.project_root, &module.path),
                digest: jet::SHA256::sha256_hex(module.source.as_bytes()),
            })
            .collect::<Vec<_>>();
        checked.sort();
        checked.dedup();
        if checked != self.modules {
            return None;
        }
        if bundle
            .comptime_inputs
            .iter()
            .any(|input| input.path.starts_with("url:"))
        {
            return None;
        }
        let mut inputs = Vec::new();
        for read in reads {
            if read.digest == "conflict" || read.digest.starts_with("unreadable") {
                return None;
            }
            let path = self.persisted_label(&read.label);
            let in_closure = path.strip_prefix("file:").is_some_and(|module| {
                self.modules
                    .iter()
                    .any(|input| input.path == module && input.digest == read.digest)
            });
            if !in_closure {
                inputs.push(ReceiptInput {
                    path,
                    digest: read.digest,
                });
            }
        }
        Some(inputs)
    }

    /// The read audit: the labels a fresh check read that the verified
    /// `record` of the same closure does not declare with the same digest.
    /// `fresh` is the fresh check's `sealed_inputs`; an unsealed fresh check
    /// cannot be declared by a sealed record at all.
    pub(crate) fn undeclared_reads(
        record: &ReceiptRecord,
        fresh: Option<&[ReceiptInput]>,
    ) -> Vec<String> {
        let Some(fresh) = fresh else {
            return vec!["unsealed".to_string()];
        };
        let declared = record
            .inputs
            .iter()
            .map(|input| (input.path.clone(), input.digest.clone()))
            .collect::<BTreeMap<_, _>>();
        let reads = fresh
            .iter()
            .map(|input| jet_foundation::CheckReads::CheckRead {
                label: input.path.clone(),
                digest: input.digest.clone(),
            })
            .collect::<Vec<_>>();
        jet_foundation::CheckReads::undeclared(&reads, &declared)
            .into_iter()
            .map(|read| read.label)
            .collect()
    }

    pub(crate) fn publish(
        &self,
        store: &Store,
        inputs: Vec<ReceiptInput>,
        diagnostics: Vec<jet::Diagnostics::Diagnostic>,
        facts: BTreeMap<String, String>,
    ) {
        let record = ReceiptRecord::new(
            self.verb,
            self.key.clone(),
            self.modules.clone(),
            inputs,
            diagnostics,
            facts,
        );
        // A record the codec cannot carry leaves this closure unrecorded; the
        // next invocation checks fresh.
        let _ = store.publish_receipt(&self.project_root, &record);
    }

    /// Append this invocation's compiler nodes to the store log. `reused`
    /// marks a Receipt hit: no module was checked, and each named output's
    /// Compile and Link nodes were answered by the stored artifact. Otherwise
    /// every module was checked, because the bundle checker runs the closure
    /// as one unit; the reason names the changed input when there is one.
    ///
    /// `packages` holds this run's package rows (#2517 S1); a Receipt hit
    /// passes none and carries the previous rows forward as reused.
    pub(crate) fn log_nodes(
        &self,
        store: &Store,
        reused: bool,
        duration_ms: f64,
        outputs: &[String],
        packages: &[PackageRow],
        package_reuse: &[jet::Sema::PackageReuseRow],
    ) {
        use jet::Comptime::Build::{BuildNodeKind, BuildPlanNode, ContentDigest};
        let previous = store
            .last_build_record(&self.project_root, &self.program)
            .ok()
            .flatten();
        let why_ran = |node: &BuildPlanNode| {
            if reused {
                return WHY_REUSED.to_string();
            }
            let old = previous.as_ref().and_then(|record| {
                record
                    .nodes
                    .iter()
                    .find(|old| old.kind == node.kind.as_str() && old.subject == node.subject)
            });
            match old {
                None if previous.is_none() => "first-run".to_string(),
                Some(old) if old.key == node.key => "bundle".to_string(),
                _ => format!("input:{}", node.subject),
            }
        };
        let checks = self
            .modules
            .iter()
            .map(|module| {
                let source = self
                    .sources
                    .get(&module.digest)
                    .map_or(&b""[..], |source| source.as_bytes());
                BuildPlanNode::new(
                    BuildNodeKind::Check,
                    module.path.clone(),
                    vec![(
                        module.path.clone(),
                        ContentDigest::from_bytes(source).as_str().to_string(),
                    )],
                )
            })
            .collect::<Vec<_>>();
        let mut nodes = checks.clone();
        if reused {
            for output in outputs {
                let compile = BuildPlanNode::new(
                    BuildNodeKind::Compile,
                    output.clone(),
                    checks
                        .iter()
                        .map(|check| (format!("check:{}", check.subject), check.key.clone()))
                        .collect(),
                );
                let link = BuildPlanNode::new(
                    BuildNodeKind::Link,
                    output.clone(),
                    vec![(format!("compile:{output}"), compile.key.clone())],
                );
                nodes.push(compile);
                nodes.push(link);
            }
        }
        let mut nodes = nodes
            .iter()
            .map(|node| BuildNodeRecord {
                kind: node.kind.as_str().to_string(),
                key: node.key.clone(),
                subject: node.subject.clone(),
                duration_ms: if reused { 0.0 } else { duration_ms },
                why_ran: why_ran(node),
                inputs: node.inputs.clone(),
            })
            .collect::<Vec<_>>();
        if package_reuse.is_empty() {
            nodes.extend(package_nodes(packages, previous.as_ref(), reused));
        } else {
            nodes.extend(package_reuse_nodes(package_reuse));
        }
        let record = BuildRecord::new(self.program.clone(), nodes);
        let record_key = jet::SHA256::sha256_hex(record.to_json().as_bytes());
        let _ = store.publish_last_build_record(&self.project_root, &record_key, &record);
    }
}

/// #2517 S2: the shared record store, as sema's package-record store.
pub(crate) struct PackageRecords {
    store: Store,
}

impl PackageRecords {
    pub(crate) fn new(store: Store) -> Self {
        Self { store }
    }
}

impl jet::Sema::PackageRecordStore for PackageRecords {
    fn compiler_identity(&self) -> String {
        format!(
            "{}+{}",
            env!("JET_COMPILER_BUILD_ID"),
            env!("JET_STDLIB_BUILD_ID")
        )
    }

    fn load(&self, schema: &str, key: &str) -> Option<Vec<u8>> {
        let kind = RecordKind::from_schema(schema)?;
        self.store.record(kind, key).ok().flatten()
    }

    fn publish(&self, schema: &str, key: &str, bytes: &[u8]) {
        if let Some(kind) = RecordKind::from_schema(schema) {
            // A record that cannot be published only costs the next run speed.
            let _ = self.store.put_record(kind, key, bytes);
        }
    }

    fn load_untouched(&self, schema: &str, key: &str) -> Option<Vec<u8>> {
        let kind = RecordKind::from_schema(schema)?;
        self.store.record_untouched(kind, key).ok().flatten()
    }

    fn publish_batch(&self, schema: &str, touched: &[String], records: Vec<(String, Vec<u8>)>) {
        if let Some(kind) = RecordKind::from_schema(schema) {
            self.store.touch_records(kind, touched);
            // A record that cannot be published only costs the next run speed.
            let _ = self.store.put_records(kind, records);
        }
    }

    fn manifest(&self, root: &Path) -> Option<Vec<u8>> {
        // The manifest is part of the Receipt closure key already; it is read
        // here only to fold its bytes into the package source digest.
        fs::read(root.join(jet::Syntax::PACKAGE_FILE)).ok()
    }
}

/// Store-log rows of a check that sealed packages from records: `reused`
/// (green by key, no body checked), `checked+published`, or `checked` with
/// the reason the package was not recorded.
fn package_reuse_nodes(rows: &[jet::Sema::PackageReuseRow]) -> Vec<BuildNodeRecord> {
    rows.iter()
        .map(|row| BuildNodeRecord {
            kind: PACKAGE_NODE.to_string(),
            key: row.key.clone(),
            subject: row.identity.clone(),
            duration_ms: 0.0,
            why_ran: format!("{}: {}", row.reuse.as_str(), row.detail),
            inputs: vec![format!("modules:{}", row.modules)],
        })
        .collect()
}

/// Store-log node kind for one package of the checked program.
pub(crate) const PACKAGE_NODE: &str = "package";

/// The store-log package rows of one run: this run's `packages` with their
/// reuse reasons against the `previous` run, or, for a replayed run
/// (`reused`), the previous run's rows carried forward as reused.
pub(crate) fn package_nodes(
    packages: &[PackageRow],
    previous: Option<&BuildRecord>,
    reused: bool,
) -> Vec<BuildNodeRecord> {
    let previous_packages = previous
        .map(|record| {
            record
                .nodes
                .iter()
                .filter(|node| node.kind == PACKAGE_NODE)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if reused {
        return previous_packages
            .into_iter()
            .map(|node| BuildNodeRecord {
                duration_ms: 0.0,
                why_ran: WHY_REUSED.to_string(),
                ..node
            })
            .collect();
    }
    packages
        .iter()
        .map(|row| row.node(previous.is_some(), &previous_packages))
        .collect()
}

/// One package of the checked program with the keys that decide its reuse
/// (#2517 S1). The Rust checker still checks the program as one unit; these
/// rows report, per package, whether its check key or its interface changed
/// since the previous run, which is what package-level reuse will act on.
pub(crate) struct PackageRow {
    identity: String,
    key: String,
    source: String,
    interface: String,
    target: String,
    compiler: String,
    dependencies: Vec<(String, String)>,
}

impl PackageRow {
    /// Rows for every package of a checked bundle, dependencies first.
    pub(crate) fn from_bundle(bundle: &jet::AST::ProgramBundle) -> Vec<Self> {
        use jet_foundation::PackageIdentity as identity;
        let graph = identity::PackageGraph::from_bundle(bundle);
        let target = identity::target_facts_digest(&identity::target_fact_pairs(bundle));
        let compiler = format!(
            "{}+{}",
            env!("JET_COMPILER_BUILD_ID"),
            env!("JET_STDLIB_BUILD_ID")
        );
        let mut interfaces = vec![String::new(); graph.packages.len()];
        let mut rows = Vec::with_capacity(graph.packages.len());
        for component in graph.components() {
            for &index in &component {
                interfaces[index] = graph.interface_digest(bundle, index, &BTreeMap::new());
            }
            for &index in &component {
                let package = &graph.packages[index];
                // The manifest is part of the closure key already; it is read
                // here only to fold its bytes into the package source digest.
                let manifest = package
                    .root
                    .as_ref()
                    .and_then(|root| fs::read(root.join(jet::Syntax::PACKAGE_FILE)).ok());
                let source = graph.source_digest(bundle, index, manifest.as_deref());
                let dependencies = package
                    .dependencies
                    .iter()
                    .map(|&dependency| {
                        (
                            graph.packages[dependency].identity.clone(),
                            interfaces[dependency].clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let key = identity::package_check_key(
                    &compiler,
                    &target,
                    &package.identity,
                    &source,
                    &dependencies,
                );
                rows.push(Self {
                    identity: package.identity.clone(),
                    key,
                    source,
                    interface: interfaces[index].clone(),
                    target: target.clone(),
                    compiler: compiler.clone(),
                    dependencies,
                });
            }
        }
        rows
    }

    fn inputs(&self) -> Vec<String> {
        let mut inputs = vec![
            format!("compiler:{}", self.compiler),
            format!("target:{}", self.target),
            format!("source:{}", self.source),
            format!("interface:{}", self.interface),
        ];
        inputs.extend(
            self.dependencies
                .iter()
                .map(|(identity, interface)| format!("dep:{identity}={interface}")),
        );
        inputs
    }

    /// The store-log node, with the reuse reason against the previous run:
    /// `green:key` (same check key), `red:source`, `red:dependency:<id>`, or
    /// `red:toolchain`, followed by `+cutoff` when a red package kept its
    /// interface digest, so its importers stay green.
    fn node(&self, has_previous: bool, previous: &[BuildNodeRecord]) -> BuildNodeRecord {
        let inputs = self.inputs();
        let old = previous.iter().find(|node| node.subject == self.identity);
        let why_ran = match old {
            None if has_previous => "red:new".to_string(),
            None => "first-run".to_string(),
            Some(old) if old.key == self.key => "green:key".to_string(),
            Some(old) => {
                let had = |input: &String| old.inputs.contains(input);
                let reason = if !had(&inputs[2]) {
                    "red:source".to_string()
                } else if let Some((identity, _)) = self
                    .dependencies
                    .iter()
                    .zip(&inputs[4..])
                    .find(|(_, input)| !had(input))
                    .map(|(dependency, _)| dependency)
                {
                    format!("red:dependency:{identity}")
                } else {
                    "red:toolchain".to_string()
                };
                if had(&inputs[3]) {
                    format!("{reason}+cutoff")
                } else {
                    reason
                }
            }
        };
        BuildNodeRecord {
            kind: PACKAGE_NODE.to_string(),
            key: self.key.clone(),
            subject: self.identity.clone(),
            duration_ms: 0.0,
            why_ran,
            inputs,
        }
    }
}
