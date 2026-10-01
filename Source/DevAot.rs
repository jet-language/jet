//! Cranelift dev builds (#3953, owner ruling 2026-09-30: Cranelift for dev
//! builds, rustc/LLVM for release).
//!
//! A dev `jet build` lowers the checked program to the same Cranelift MIR
//! artifact `jet run` executes, compiles it with the JIT's lowering into a
//! self-contained image, and writes the executable as a copy of the prebuilt
//! `jet-aot-rt` runner with that image appended. No Rust is generated and no
//! rustc, C compiler or linker runs. The runner verifies the image (checksum
//! and runtime build identity) before running it. Programs the image cannot
//! carry are refused with a reason, and the caller falls back to rustc.
//!
//! The build cache stores the image, never the executable. The runner is
//! part of this `jet` installation (its identity is part of the cache key),
//! so a cached build is restored by copying the runner and appending the
//! stored image, and an existing output is recognized by its length and image
//! trailer instead of by hashing the whole executable. The store also keeps
//! every compiled function, so a build that changes some functions compiles
//! only those.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use jet_store::{ActionHandle, ArtifactLookup, Store};

/// The runner executable's file name.
fn runner_name() -> String {
    format!("jet-aot-rt{}", std::env::consts::EXE_SUFFIX)
}

/// The prebuilt runner shipped with this `jet`: the stripped copy the
/// packaging step installs in `jet-runtime/`, else the one beside the
/// executable. A runner from another build would refuse this compiler's
/// images, so only a runner answering this build's identity is used.
fn runner_path() -> Result<PathBuf, String> {
    let name = runner_name();
    let exe = std::env::current_exe()
        .map_err(|error| format!("`jet` couldn't locate itself: {error}"))?;
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let candidates = [
        dir.join(jet_store::runtime::PREBUILT_DIR_NAME).join(&name),
        dir.join(&name),
    ];
    let mut refused = None;
    for runner in candidates.into_iter().filter(|path| path.is_file()) {
        let probe = std::process::Command::new(&runner)
            .arg(jet_jit::RUNTIME_ID_PROBE)
            .output()
            .map_err(|error| format!("the prebuilt runner {} couldn't start: {error}", runner.display()))?;
        let runner_id = String::from_utf8_lossy(&probe.stdout);
        if probe.status.success() && runner_id.trim() == env!("JET_RUNNER_BUILD_ID") {
            return Ok(runner);
        }
        refused.get_or_insert(runner);
    }
    Err(match refused {
        Some(runner) => format!(
            "the prebuilt runner {} comes from a different Jet build",
            runner.display()
        ),
        None => format!("the prebuilt runner `{name}` is not installed beside `jet`"),
    })
}

/// Write `out` as a Cranelift dev executable for the checked `bundle` and
/// return its image, the bytes the build cache stores. `store` also keeps
/// the compiled functions, which later builds of any program reuse. `Err` is
/// the refusal reason; `out` is left untouched then.
pub(crate) fn build(
    bundle: &jet::AST::ProgramBundle,
    profile: &str,
    out: &Path,
    store: Option<&Store>,
) -> Result<Vec<u8>, String> {
    let runner = runner_path()?;
    let (mir, artifact) = jet::Interpreter::lower_for_dev_aot(bundle, profile);
    let code_store = store.map(|store| {
        Arc::new(StoreCodeCache {
            store: store.clone(),
            hits: Mutex::new(Vec::new()),
        }) as Arc<dyn jet_jit::CompiledCodeStore>
    });
    let image =
        jet_jit::compile_dev_aot_image(&mir, artifact, env!("JET_RUNNER_BUILD_ID"), code_store)?;
    write_output(&runner, &image, out)?;
    Ok(image)
}

/// Compiled functions as store action records: Cranelift's compiled code,
/// keyed by its incremental-cache key (a hash of the function's IR, target
/// and flags) together with this `jet`'s build identity, so only the
/// compiler that produced a record reuses it.
struct StoreCodeCache {
    store: Store,
    /// Records read this build, marked used together when it saves.
    hits: Mutex<Vec<ActionHandle>>,
}

impl StoreCodeCache {
    fn action(key: &[u8]) -> ActionHandle {
        const DOMAIN: &[u8] = b"jet.cranelift-code/v1\0";
        let build = env!("JET_RUNNER_BUILD_ID").as_bytes();
        let mut bytes = Vec::with_capacity(DOMAIN.len() + build.len() + 1 + key.len());
        bytes.extend_from_slice(DOMAIN);
        bytes.extend_from_slice(build);
        bytes.push(0);
        bytes.extend_from_slice(key);
        ActionHandle::from_bytes(&bytes)
    }
}

impl jet_jit::CompiledCodeStore for StoreCodeCache {
    fn load(&self, key: &[u8]) -> Option<Vec<u8>> {
        let action = Self::action(key);
        let record = self.store.get_action_untouched(&action).ok().flatten()?;
        self.hits.lock().unwrap_or_else(PoisonError::into_inner).push(action);
        Some(record)
    }

    fn save(&self, entries: Vec<(Vec<u8>, Vec<u8>)>) {
        let records = entries
            .into_iter()
            .map(|(key, code)| (Self::action(&key), code))
            .collect::<Vec<_>>();
        // A record that fails to land is compiled again by a later build.
        let _ = self.store.publish_cache_actions(&records);
        let hits = std::mem::take(&mut *self.hits.lock().unwrap_or_else(PoisonError::into_inner));
        self.store.touch_actions(&hits);
    }
}

/// Write `out` from the image `store` holds for `key` (verified by the store)
/// and this build's runner. `false` when the store has no image for `key` or
/// no runner answers; the caller then builds.
pub(crate) fn restore(store: &Store, key: &str, out: &Path) -> bool {
    let Ok(ArtifactLookup::Hit(artifact)) = store.lookup_artifact(key) else {
        return false;
    };
    let Ok(image) = fs::read(&artifact.path) else {
        return false;
    };
    let trailer_start = image.len().saturating_sub(jet_jit::DEV_AOT_TRAILER_LEN);
    if !jet_jit::is_dev_aot_trailer(&image[trailer_start..]) {
        return false;
    }
    runner_path().is_ok_and(|runner| write_output(&runner, &image, out).is_ok())
}

/// The identity of the dev executable at `path`: its length and image
/// trailer (the image's SHA-256, body length and format magic), rendered as
/// `<length>:<trailer hex>`. `None` when `path` is not a dev executable.
pub(crate) fn output_identity(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut trailer = [0u8; jet_jit::DEV_AOT_TRAILER_LEN];
    let start = length.checked_sub(trailer.len() as u64)?;
    file.seek(SeekFrom::Start(start)).ok()?;
    file.read_exact(&mut trailer).ok()?;
    jet_jit::is_dev_aot_trailer(&trailer).then(|| {
        let hex = trailer.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        format!("{length}:{hex}")
    })
}

/// Write `out` as `runner` followed by `image` through a sibling temporary
/// file renamed into place, so `out` is never a partial executable.
fn write_output(runner: &Path, image: &[u8], out: &Path) -> Result<(), String> {
    let dir = out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = out
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".dev-aot.{name}.{}", std::process::id()));
    let written = fs::copy(runner, &tmp)
        .and_then(|_| {
            let mut file = fs::OpenOptions::new().append(true).open(&tmp)?;
            file.write_all(image)
        })
        .and_then(|()| fs::rename(&tmp, out));
    written.map_err(|error| {
        let _ = fs::remove_file(&tmp);
        format!("couldn't write {}: {error}", out.display())
    })
}
