//! Parallel Cranelift compilation of a resident module (#3953), with a
//! content-addressed cache of compiled functions for dev builds.
//!
//! Lowering MIR to Cranelift IR stays on the compiling thread: it declares
//! functions and data in the module and fills runtime tables. Turning that IR
//! into machine code reads only the IR and the target ISA. So while a batch is
//! open for a module, `define_function_checked` queues each lowered function
//! instead of compiling it; the batch compiles the queue on worker threads,
//! then defines every function's bytes on the compiling thread in queue order.
//! A function's machine code depends only on its own IR, so the code is the
//! same bytes a one-thread compile produces.
//!
//! With a code store installed (Cranelift dev builds), each function compiles
//! through Cranelift's incremental cache. Its key hashes the function's IR with
//! callee names abstracted, together with the ISA and its flags; its value is
//! the compiled, position-independent code. A function whose IR an earlier
//! build, another program, or an earlier function of this build already
//! compiled is not compiled again.

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::incremental_cache::CacheKvStore;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::{ir, CodegenError, Context};
use cranelift_module::{FuncId, Module, ModuleDeclarations, ModuleError};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// Queued functions that trigger a compile while lowering continues, so the
/// IR of a large program is never held all at once.
const BATCH_LIMIT: usize = 1024;

/// Worker thread stack: Cranelift's passes are iterative, this is headroom.
const WORKER_STACK_BYTES: usize = 16 << 20;

/// Compiled functions kept across builds and programs (#3953). Keys are
/// Cranelift incremental-cache keys; values are the compiled code Cranelift
/// serialized for them. The store adds its own identity (this `jet`'s build)
/// to the key, so a value is reused only by the compiler that produced it.
pub trait CompiledCodeStore: Send + Sync {
    /// The compiled code stored for `key`, if any.
    fn load(&self, key: &[u8]) -> Option<Vec<u8>>;
    /// Keep newly compiled `(key, code)` entries for later builds. A failed
    /// write only costs a later recompile.
    fn save(&self, entries: Vec<(Vec<u8>, Vec<u8>)>);
}

thread_local! {
    static BATCH: RefCell<Option<Batch>> = const { RefCell::new(None) };
    static CODE_STORE: RefCell<Option<Arc<dyn CompiledCodeStore>>> = const { RefCell::new(None) };
}

struct Batch {
    /// The module the batch belongs to, by its declaration table: a function
    /// defined into any other module compiles at once, as without a batch.
    owner: *const ModuleDeclarations,
    pending: Vec<(FuncId, Context)>,
    /// `None` when no code store is installed.
    cache: Option<BuildCache>,
}

/// Run `work` with `store` as the code store of every batch it opens.
pub(crate) fn with_code_store<R>(
    store: Option<Arc<dyn CompiledCodeStore>>,
    work: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<Arc<dyn CompiledCodeStore>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CODE_STORE.with(|slot| *slot.borrow_mut() = previous);
        }
    }
    let _restore = Restore(CODE_STORE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), store)));
    work()
}

/// An open batch. Dropping it without `finish` discards the queued functions
/// (the compile failed) and reopens the batch it replaced, if any.
pub(crate) struct BatchGuard {
    previous: Option<Batch>,
}

/// Open a batch for the module whose declaration table is `owner`.
pub(crate) fn open(owner: &ModuleDeclarations) -> BatchGuard {
    let cache = CODE_STORE
        .with(|slot| slot.borrow().clone())
        .map(BuildCache::new);
    let batch = Batch {
        owner: owner as *const ModuleDeclarations,
        pending: Vec::new(),
        cache,
    };
    BatchGuard {
        previous: BATCH.with(|slot| slot.borrow_mut().replace(batch)),
    }
}

impl BatchGuard {
    /// Compile and define every function still queued.
    pub(crate) fn finish<M: Module + ?Sized>(self, module: &mut M) -> Result<(), String> {
        flush(module)
    }
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        BATCH.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// Queue the function in `context` on the batch open for `module`, taking it
/// out of `context`. `false` when no batch is open for `module`; the caller
/// then defines the function itself.
pub(crate) fn queue<M: Module + ?Sized>(
    module: &mut M,
    id: FuncId,
    context: &mut Context,
) -> Result<bool, String> {
    let owner = module.declarations() as *const ModuleDeclarations;
    let full = BATCH.with(|slot| {
        let mut slot = slot.borrow_mut();
        let batch = slot.as_mut().filter(|batch| batch.owner == owner)?;
        let func = std::mem::replace(&mut context.func, ir::Function::new());
        batch.pending.push((id, Context::for_function(func)));
        Some(batch.pending.len() >= BATCH_LIMIT)
    });
    match full {
        None => Ok(false),
        Some(false) => Ok(true),
        Some(true) => flush(module).map(|()| true),
    }
}

/// The explanation `define_function_checked` gives when Cranelift rejects a
/// function. `ModuleError`'s `Display` collapses a verifier failure to the bare
/// summary `Compilation error: Verifier errors`; the verifier's per-instruction
/// messages and the annotated function text are the whole diagnosis of an
/// invalid-IR bug, so they must reach the ICE.
pub(crate) fn define_error_detail(func: &ir::Function, error: ModuleError) -> String {
    match error {
        ModuleError::Compilation(CodegenError::Verifier(errors)) => {
            let mut detail = format!("Cranelift verifier rejected `{}`:\n", func.name);
            for error in &errors.0 {
                detail.push_str("  - ");
                detail.push_str(&error.to_string());
                detail.push('\n');
            }
            detail.push_str(&cranelift_codegen::print_errors::pretty_verifier_error(
                func, None, errors,
            ));
            detail
        }
        other => other.to_string(),
    }
}

fn flush<M: Module + ?Sized>(module: &mut M) -> Result<(), String> {
    let taken = BATCH.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .map(|batch| (std::mem::take(&mut batch.pending), batch.cache.take()))
    });
    let Some((mut pending, cache)) = taken else {
        return Ok(());
    };
    let compiled = compile_all(module.isa(), &mut pending, cache.as_ref());
    if let Some(cache) = cache {
        cache.save();
        BATCH.with(|slot| {
            if let Some(batch) = slot.borrow_mut().as_mut() {
                batch.cache = Some(cache);
            }
        });
    }
    compiled?;
    for (id, context) in &pending {
        let code = context
            .compiled_code()
            .ok_or_else(|| format!("Cranelift left function {id} uncompiled"))?;
        module
            .define_function_bytes(
                *id,
                &context.func,
                u64::from(code.buffer.alignment),
                code.code_buffer(),
                code.buffer.relocs(),
            )
            .map_err(|error| define_error_detail(&context.func, error))?;
        if let Some(name) = module.declarations().get_function_decl(*id).name.as_deref() {
            super::tier_cache::note_defined(name, *id, context);
        }
    }
    Ok(())
}

/// Compile every queued function, on as many threads as the compiler's one
/// thread cap admits (`--threads N`; up to the reported cores without it).
/// The first failure in queue order wins, so the reported error does not
/// depend on thread timing.
fn compile_all(
    isa: &dyn TargetIsa,
    pending: &mut [(FuncId, Context)],
    cache: Option<&BuildCache>,
) -> Result<(), String> {
    let threads = jet_foundation::CompilerThreads::admit(pending.len(), usize::MAX);
    let queue = Mutex::new(pending.iter_mut().enumerate());
    let drain = || {
        let mut failures = Vec::new();
        loop {
            let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
            let Some((index, (_, context))) = next else {
                return failures;
            };
            if let Err(detail) = compile_one(isa, context, cache) {
                failures.push((index, detail));
            }
        }
    };
    // A panic on a worker reaches the caller like one on this thread: the
    // resident panic hook keeps it as quiet as it would be here.
    let silenced = super::runtime_host::jit_panic_window_open();
    let (mut failures, panic) = std::thread::scope(|scope| {
        let workers = (1..threads)
            .filter_map(|_| {
                std::thread::Builder::new()
                    .name("jet-cranelift".to_string())
                    .stack_size(WORKER_STACK_BYTES)
                    .spawn_scoped(scope, move || {
                        let _window = silenced.then(super::runtime_host::JitPanicWindow::enter);
                        drain()
                    })
                    .ok()
            })
            .collect::<Vec<_>>();
        // This thread drains the queue too, so a refused spawn only means
        // fewer workers.
        let mut failures = drain();
        let mut panic = None;
        for worker in workers {
            match worker.join() {
                Ok(more) => failures.extend(more),
                Err(payload) => {
                    panic.get_or_insert(payload);
                }
            }
        }
        (failures, panic)
    });
    if let Some(payload) = panic {
        std::panic::resume_unwind(payload);
    }
    failures.sort_unstable_by_key(|(index, _)| *index);
    match failures.into_iter().next() {
        Some((_, detail)) => Err(detail),
        None => Ok(()),
    }
}

fn compile_one(
    isa: &dyn TargetIsa,
    context: &mut Context,
    cache: Option<&BuildCache>,
) -> Result<(), String> {
    // Every resident module is a hot-swap JIT module (`new_jit_module`), whose
    // `define_function` makes each callee and symbol non-colocated before it
    // compiles; the queued function must compile to that same code.
    for func in context.func.dfg.ext_funcs.values_mut() {
        func.colocated = false;
    }
    for value in context.func.global_values.values_mut() {
        if let ir::GlobalValueData::Symbol { colocated, .. } = value {
            *colocated = false;
        }
    }
    let mut ctrl_plane = ControlPlane::default();
    let result = match cache {
        Some(cache) => context
            .compile_with_cache(isa, &mut CacheView(cache), &mut ctrl_plane)
            .map(|_| ()),
        None => context.compile(isa, &mut ctrl_plane).map(|_| ()),
    };
    result.map_err(|error| define_error_detail(error.func, ModuleError::Compilation(error.inner)))
}

/// The code cache of one build: the code store, plus every function this
/// build compiled, so a later function with the same key reuses it.
struct BuildCache {
    store: Arc<dyn CompiledCodeStore>,
    compiled: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    unsaved: Mutex<Vec<(Vec<u8>, Vec<u8>)>>,
}

impl BuildCache {
    fn new(store: Arc<dyn CompiledCodeStore>) -> Self {
        Self {
            store,
            compiled: Mutex::new(HashMap::new()),
            unsaved: Mutex::new(Vec::new()),
        }
    }

    fn save(&self) {
        let entries = std::mem::take(&mut *self.unsaved.lock().unwrap_or_else(PoisonError::into_inner));
        if !entries.is_empty() {
            self.store.save(entries);
        }
    }
}

struct CacheView<'a>(&'a BuildCache);

impl CacheKvStore for CacheView<'_> {
    fn get(&self, key: &[u8]) -> Option<Cow<[u8]>> {
        let built = self
            .0
            .compiled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned();
        built.or_else(|| self.0.store.load(key)).map(Cow::Owned)
    }

    fn insert(&mut self, key: &[u8], val: Vec<u8>) {
        let fresh = self
            .0
            .compiled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_vec(), val.clone())
            .is_none();
        if fresh {
            self.0
                .unsaved
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((key.to_vec(), val));
        }
    }
}
