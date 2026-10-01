//! #2517: compile-time constant values reused across checks.
//!
//! Registration evaluates every `prep` and module-value constant through the
//! MIR fragment evaluator, which lowers, optimizes and verifies a program per
//! constant. In a package-record session a successful evaluation is stored
//! under a key over everything the evaluator reads, and a later check with
//! the same key installs the stored value instead of evaluating again.
//!
//! The key covers:
//! - the compiler identity and the check environment (build facts, edition,
//!   policy; the build clock only when some source reads `$build.stamp`);
//! - the loaded source of every other module of the bundle (the imported
//!   declarations and bodies the fragment may lower come from them);
//! - the evaluating module's identity, imports, and every item that is not a
//!   top-level function (types, methods, impls, constants), span-free;
//! - the top-level functions the constant can reach by name, transitively,
//!   from its initializer or from those items;
//! - the values of the constants evaluated before it, and its initializer as
//!   checked, span-free.
//!
//! A value is recorded only when its evaluation succeeded without reading
//! a file, when the value codec carries it exactly (no closure, no told
//! report, no source span), and when decoding the stored bytes rebuilds the
//! value exactly. A stored value is installed only when its digest verifies.

use super::BodyDelta::{decode_value, encode_value};
use super::PackageRecordStore;
use crate::Comptime::CtValue;
use crate::AST::{Expr, Func, Item, ProgramBundle};
use jet_foundation::PackageIdentity::KeyHasher;
use jet_foundation::RecordCodec::{
    encode_record, record_map, RecordReader, RecordSection, RecordValue,
};
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// Schema of one stored constant value (an action record under its key).
const COMPTIME_VALUE_SCHEMA: &str = "jet.comptime-value/v1";
const SECTION_VALUE: u64 = 1;

/// The environment of one check's constant evaluations, and the store work
/// they leave for the end of registration.
pub(crate) struct ComptimeReuseEnv {
    store: Arc<dyn PackageRecordStore>,
    /// Compiler identity and check environment every key starts from.
    environment: String,
    /// Per module: digest of its identity and loaded source.
    modules: Vec<String>,
    pending: RefCell<Pending>,
}

#[derive(Default)]
struct Pending {
    touched: Vec<String>,
    records: Vec<(String, Vec<u8>)>,
}

/// The constant-evaluation environment of `bundle`, or `None` outside a
/// package-record session.
pub(crate) fn environment(bundle: &ProgramBundle) -> Option<ComptimeReuseEnv> {
    let store = super::PackageSeal::session_store()?;
    let mut hasher = KeyHasher::new(COMPTIME_VALUE_SCHEMA);
    hasher
        .field(&store.compiler_identity())
        .field(&jet_foundation::SHA256::sha256_hex(
            &super::incremental_global_environment(bundle, ""),
        ))
        .field(&jet_foundation::PackageEdition::package_edition());
    // The environment leaves the build clock out; a constant can read it
    // only through `$build.stamp`.
    if bundle
        .modules
        .iter()
        .any(|module| reads_build_stamp(&module.source))
    {
        hasher.field(&bundle.build_facts.stamp.at);
    }
    let modules = bundle
        .modules
        .iter()
        .map(|module| {
            let mut hasher = KeyHasher::new(COMPTIME_VALUE_SCHEMA);
            hasher
                .field(&module.display)
                .field(&module.path.to_string_lossy())
                .field(&module.alias)
                .field(&jet_foundation::SHA256::sha256_hex(module.source.as_bytes()));
            hasher.finish()
        })
        .collect();
    Some(ComptimeReuseEnv {
        store,
        environment: hasher.finish(),
        modules,
        pending: RefCell::new(Pending::default()),
    })
}

/// Whether `source` may read a build stamp fact: `$build` followed by
/// `.stamp`, spaces allowed between.
fn reads_build_stamp(source: &str) -> bool {
    source.match_indices("$build").any(|(at, found)| {
        source[at + found.len()..]
            .trim_start()
            .strip_prefix('.')
            .is_some_and(|rest| rest.trim_start().starts_with("stamp"))
    })
}

impl Drop for ComptimeReuseEnv {
    /// Record the uses and the new values of this check in one store
    /// transaction.
    fn drop(&mut self) {
        let pending = std::mem::take(self.pending.get_mut());
        if !pending.touched.is_empty() || !pending.records.is_empty() {
            self.store
                .publish_batch(COMPTIME_VALUE_SCHEMA, &pending.touched, pending.records);
        }
    }
}

impl ComptimeReuseEnv {
    /// The stored value under `key`, when its digest verifies.
    pub(crate) fn load(&self, key: &str) -> Option<CtValue> {
        let bytes = self.store.load_untouched(COMPTIME_VALUE_SCHEMA, key)?;
        let reader = RecordReader::open_schema(&bytes, COMPTIME_VALUE_SCHEMA).ok()?;
        let entry = reader.element(SECTION_VALUE, 0).ok()?;
        let RecordValue::Bytes(encoded) = entry.field("value").ok()? else {
            return None;
        };
        if jet_foundation::SHA256::sha256_hex(encoded) != entry.field("checked").ok()?.as_str().ok()? {
            return None;
        }
        let value = decode_value(encoded)?;
        self.pending.borrow_mut().touched.push(key.to_string());
        Some(value)
    }

    /// Store `value` under `key` for the next check, when the codec carries
    /// it exactly.
    pub(crate) fn record(&self, key: String, value: &CtValue) {
        let Ok(encoded) = encode_value(value) else {
            return;
        };
        if !decode_value(&encoded).is_some_and(|decoded| super::ItemReuse::same_debug(&decoded, value)) {
            return;
        }
        let checked = jet_foundation::SHA256::sha256_hex(&encoded);
        let entry = record_map([
            ("value", RecordValue::Bytes(encoded)),
            ("checked", RecordValue::Str(checked)),
        ]);
        if let Ok(bytes) =
            encode_record(COMPTIME_VALUE_SCHEMA, &[RecordSection::new(SECTION_VALUE, vec![entry])])
        {
            self.pending.borrow_mut().records.push((key, bytes));
        }
    }

    /// The digest of a value a later constant may read, or `None` when the
    /// codec cannot carry it (a constant reading it is then not reused).
    pub(crate) fn value_digest(&self, value: &CtValue) -> Option<String> {
        encode_value(value)
            .ok()
            .map(|encoded| jet_foundation::SHA256::sha256_hex(&encoded))
    }
}

/// One module's evaluation inputs besides the constant itself.
pub(crate) struct ModuleComptime<'a> {
    /// Digest of the module's context and of every item that is not a
    /// top-level function.
    module: String,
    /// Top-level functions by name.
    by_name: HashMap<&'a str, Vec<usize>>,
    functions: Vec<(&'a Func, OnceCell<FunctionUnit>)>,
    /// Functions every constant of the module reaches through its
    /// non-function items.
    always: BTreeSet<usize>,
}

struct FunctionUnit {
    digest: String,
    names: Vec<String>,
}

impl<'a> ModuleComptime<'a> {
    /// `context` names the module's evaluation context (its import tables
    /// and base directory), rendered deterministically by the caller.
    pub(crate) fn new(
        env: &ComptimeReuseEnv,
        module_idx: usize,
        items: &'a [Item],
        context: &str,
    ) -> Self {
        let mut hasher = KeyHasher::new(COMPTIME_VALUE_SCHEMA);
        hasher.field(&env.environment).field(&module_idx.to_string()).field(context);
        for (index, module) in env.modules.iter().enumerate() {
            if index != module_idx {
                hasher.field(module);
            }
        }
        let mut by_name = HashMap::<&str, Vec<usize>>::new();
        let mut functions = Vec::new();
        let mut seeds = Vec::new();
        for item in items {
            if let Item::Func(function) = item {
                by_name.entry(function.name.as_str()).or_default().push(functions.len());
                functions.push((function, OnceCell::new()));
                continue;
            }
            let text = crate::CanonicalAST::canonical_fragment(item);
            hasher.field(&jet_foundation::SHA256::sha256_hex(&text));
            seeds.extend(quoted_names(&text));
        }
        let mut module = Self {
            module: hasher.finish(),
            by_name,
            functions,
            always: BTreeSet::new(),
        };
        module.always = module.reach(BTreeSet::new(), seeds);
        module
    }

    /// The reuse key of the constant `name` with checked initializer `value`,
    /// or `None` when a constant evaluated before it has no digest.
    /// `globals` are the digests of the constants evaluated before it.
    pub(crate) fn key(
        &self,
        name: &str,
        value: &Expr,
        globals: &BTreeMap<String, Option<String>>,
    ) -> Option<String> {
        let text = crate::CanonicalAST::canonical_fragment(value);
        let reached = self.reach(self.always.clone(), quoted_names(&text));
        let mut hasher = KeyHasher::new(COMPTIME_VALUE_SCHEMA);
        hasher
            .field(&self.module)
            .field(name)
            .field(&jet_foundation::SHA256::sha256_hex(&text));
        for index in reached {
            hasher.field(&self.unit(index).digest);
        }
        for (global, digest) in globals {
            hasher.field(global).field(digest.as_deref()?);
        }
        Some(hasher.finish())
    }

    /// `reached` plus every top-level function reachable by name from
    /// `names`.
    fn reach(&self, mut reached: BTreeSet<usize>, mut names: Vec<String>) -> BTreeSet<usize> {
        while let Some(name) = names.pop() {
            for &index in self.by_name.get(name.as_str()).into_iter().flatten() {
                if reached.insert(index) {
                    names.extend(self.unit(index).names.iter().cloned());
                }
            }
        }
        reached
    }

    fn unit(&self, index: usize) -> &FunctionUnit {
        let (function, unit) = &self.functions[index];
        unit.get_or_init(|| {
            let text = crate::CanonicalAST::canonical_fragment(*function);
            FunctionUnit {
                digest: jet_foundation::SHA256::sha256_hex(&text),
                names: quoted_names(&text),
            }
        })
    }
}

/// Every string literal of a `Debug` rendering: the names a node refers to
/// (calls, identifiers, fields, types). Char literals are skipped whole so a
/// quote inside one does not open a string.
fn quoted_names(text: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let quote = text[at];
        if quote != b'"' && quote != b'\'' {
            at += 1;
            continue;
        }
        let start = at + 1;
        at = start;
        while at < text.len() && text[at] != quote {
            at += if text[at] == b'\\' { 2 } else { 1 };
        }
        if quote == b'"' {
            if let Some(name) = text.get(start..at.min(text.len())) {
                names.push(String::from_utf8_lossy(name).into_owned());
            }
        }
        at += 1;
    }
    names
}
