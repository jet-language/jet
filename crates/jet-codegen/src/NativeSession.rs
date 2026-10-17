//! Resident native session host for D-EXEC1 / #4720.
//!
//! This is an internal compiler host, not a native-plugin API. It loads one
//! canonical runtime, retains every installed turn image, and invokes the
//! existing selector-generated pointer-to-cells adapter. Native binding places
//! live in the runtime's `jet_rt_session_*` slots, never in rendered records.
//! The caller must supply checked, compiler-produced code from the same build.
//! Panics/traps currently end the host process; recoverable interrupt requires
//! MIR-visible status propagation through canonical cleanup (not longjmp).

use std::ffi::{c_void, CStr};
use crate::NativeLibraryImage::Image;
use std::io;
use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;

// JET_VETTED_UNSAFE_BEGIN: native_session_loader
// AUDIT: a host loads only its own checked compiler output and verified runtime.
// All symbols have explicit C signatures. The runtime and turn images remain
// loaded until their last call; session destruction runs original drop glue
// before images unload. No Rust types or Rust ABI cross this boundary. The
// pointer-to-cells entry borrows two aligned words for its entire invocation.

fn name(bytes: &'static [u8]) -> &'static CStr {
    CStr::from_bytes_with_nul(bytes).expect("compiler-owned symbol is terminated")
}

type Create = extern "C" fn() -> u64;
type Lifecycle = extern "C" fn(u64) -> bool;
type Destroy = unsafe extern "C" fn(u64) -> bool;
type Adapter = unsafe extern "C" fn(*mut u64);
type RuntimeIdentity = unsafe extern "C" fn(*mut u8);

struct TurnImage {
    // Keep this even if no slot was constructed: stored function/closure
    // payloads can reference any installed image, not only their drop image.
    _image: Image,
    entry: Adapter,
}

/// One owning worker thread and one shared runtime instance. Not Send/Sync:
/// sema's borrow rules do not make arbitrary stored native payloads thread-safe.
pub struct SessionHost {
    _runtime: Image,
    identity: [u8; 32],
    id: u64,
    begin: Lifecycle,
    end: Lifecycle,
    destroy: Destroy,
    runtime_create: *mut c_void,
    turns: Vec<TurnImage>,
    _owning_thread: PhantomData<Rc<()>>,
}

impl SessionHost {
    /// # Safety
    /// `runtime_path` must be the trusted, verified native pack runtime for this
    /// build. `identity` is that artifact's SHA-256 from its admitted manifest.
    /// No other runtime instance may already be installed in this worker. The
    /// runtime must implement the signatures of `jet_rt_session_*` below.
    pub unsafe fn open(runtime_path: &Path, identity: [u8; 32]) -> io::Result<Self> {
        let runtime = unsafe { Image::open(runtime_path, true) }?;
        let runtime_create = runtime.symbol(name(b"jet_rt_session_create\0"))?;
        // SAFETY: the trusted same-build runtime exports these C signatures.
        let create: Create = unsafe { std::mem::transmute(runtime_create) };
        let begin: Lifecycle = unsafe { std::mem::transmute(runtime.symbol(name(b"jet_rt_session_begin\0"))?) };
        let end: Lifecycle = unsafe { std::mem::transmute(runtime.symbol(name(b"jet_rt_session_end\0"))?) };
        let destroy: Destroy = unsafe { std::mem::transmute(runtime.symbol(name(b"jet_rt_session_destroy\0"))?) };
        let id = create();
        if id == 0 { return Err(io::Error::other("native session handle space exhausted")); }
        Ok(Self { _runtime: runtime, identity, id, begin, end, destroy, runtime_create, turns: Vec::new(), _owning_thread: PhantomData })
    }

    /// Install a turn, retaining its code until reset/destruction. The generated
    /// manifest function writes the exact runtime artifact SHA-256 into 32
    /// caller-owned bytes. The entry is the existing pointer-to-cells adapter
    /// for `(session: u64) -> status: u64`, not an alternative call convention.
    ///
    /// # Safety
    /// `path` must contain trusted compiler-produced code and dependencies.
    /// Both manifest and entry signatures must match the contract above. The
    /// program must obey slot initialization/borrow/move/drop obligations and
    /// must not statically embed another copy of the runtime or Prelude state.
    pub unsafe fn install(&mut self, path: &Path) -> io::Result<usize> {
        let image = unsafe { Image::open(path, false) }?;
        // Reject a statically duplicated slot runtime, even if the manifest
        // claims the same artifact. Imported symbols must bind our instance.
        if image.symbol(name(b"jet_rt_session_create\0"))? != self.runtime_create {
            return Err(io::Error::other("turn is bound to a different runtime instance"));
        }
        let manifest: RuntimeIdentity = unsafe { std::mem::transmute(image.symbol(name(b"jet_session_runtime_identity\0"))?) };
        let mut identity = [0u8; 32];
        unsafe { manifest(identity.as_mut_ptr()); }
        if identity != self.identity {
            return Err(io::Error::other("turn and resident runtime have different build identities"));
        }
        let entry: Adapter = unsafe { std::mem::transmute(image.symbol(name(b"jet_session_turn\0"))?) };
        let index = self.turns.len();
        self.turns.push(TurnImage { _image: image, entry });
        Ok(index)
    }

    /// Run one previously installed checked turn. The result is the canonical
    /// native status word; no value decoding or policy lives in this host.
    pub fn run(&mut self, turn: usize) -> io::Result<u64> {
        let entry = self.turns.get(turn).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "native turn is not installed"))?.entry;
        if !(self.begin)(self.id) { return Err(io::Error::other("native session cannot begin a turn")); }
        let mut cells = [self.id, u64::MAX];
        // SAFETY: install established this exact generated adapter signature;
        // its image and runtime remain retained, and cells remain on our stack.
        unsafe { entry(cells.as_mut_ptr()); }
        if !(self.end)(self.id) { return Err(io::Error::other("native session cannot finish a turn")); }
        Ok(cells[1])
    }

    /// Reset drops stored owners before unloading code, then creates an empty
    /// session against the SAME resident runtime (including its Core state).
    pub fn reset(&mut self) -> io::Result<()> {
        // SAFETY: run is synchronous and no frames/borrows survive its return.
        if !unsafe { (self.destroy)(self.id) } {
            return Err(io::Error::other("native session still has an active turn"));
        }
        self.id = 0;
        self.turns.clear();
        let create: Create = unsafe { std::mem::transmute(self.runtime_create) };
        self.id = create();
        if self.id == 0 { return Err(io::Error::other("native session handle space exhausted")); }
        Ok(())
    }
}

impl Drop for SessionHost {
    fn drop(&mut self) {
        if self.id != 0 {
            // SAFETY: all synchronous calls returned; images remain pinned.
            // Failure means a runtime contract violation: unloading code while
            // active would be unsafe, so fail closed rather than continuing.
            if !unsafe { (self.destroy)(self.id) } { std::process::abort(); }
        }
        self.turns.clear();
        // Explicitly clearing turns before runtime drops preserves dependencies
        // and glue lifetimes.
    }
}
// JET_VETTED_UNSAFE_END: native_session_loader

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn build(dir: &Path, crate_name: &str, source: &str) -> std::path::PathBuf {
        let input = dir.join(format!("{crate_name}.rs"));
        let output = dir.join(format!("lib{crate_name}.so"));
        fs::write(&input, source).unwrap();
        let result = Command::new("rustc").args(["--edition=2021", "--crate-type=cdylib", "--crate-name", crate_name])
            .arg(&input).arg("-o").arg(&output)
            .arg("-L").arg(dir).arg("-C").arg(format!("link-arg=-Wl,-rpath,{}", dir.display()))
            .output().unwrap();
        assert!(result.status.success(), "fixture build failed: {}", String::from_utf8_lossy(&result.stderr));
        output
    }

    #[test]
    fn real_dylib_turns_share_places_and_pin_original_drop_glue() {
        let root = std::env::var_os("JET_TEST_SCRATCH").map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = Scratch(root.join(format!("jet-session-host-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
        fs::create_dir_all(&dir.0).unwrap();
        let runtime = build(&dir.0, "jet_session_runtime", include_str!("Prelude/Core/CAbiSession.rs"));
        let identity = crate::SHA256::sha256(&fs::read(&runtime).unwrap());
        let drop_file = dir.0.join("dropped");
        let header = format!(r#"
#[link(name = "jet_session_runtime")]
extern "C" {{
    fn jet_rt_session_create() -> u64;
    fn jet_rt_session_reserve(id: u64, slot: u64, a: u64, b: u64, c: u64, d: u64,
        size: usize, align: usize, drop_glue: Option<unsafe extern "C" fn(*mut u8)>) -> *mut u8;
    fn jet_rt_session_publish(id: u64, slot: u64) -> bool;
    fn jet_rt_session_resolve(id: u64, slot: u64, a: u64, b: u64, c: u64, d: u64,
        size: usize, align: usize) -> *mut u8;
}}
#[no_mangle]
pub unsafe extern "C" fn jet_session_runtime_identity(out: *mut u8) {{
    std::ptr::copy_nonoverlapping({identity:?}.as_ptr(), out, 32);
}}
"#);
        let first = build(&dir.0, "turn_first", &format!(r#"{header}
unsafe extern "C" fn drop_value(ptr: *mut u8) {{
    assert_eq!(ptr.cast::<u64>().read(), 41);
    std::fs::write({drop_file:?}, "original turn glue ran").unwrap();
}}
#[no_mangle]
pub unsafe extern "C" fn jet_session_turn(cells: *mut u64) {{
    let id = cells.read();
    let place = jet_rt_session_reserve(id, 1, 1, 2, 3, 4, 8, 8, Some(drop_value));
    assert!(!place.is_null());
    place.cast::<u64>().write(41);
    assert!(jet_rt_session_publish(id, 1));
    cells.add(1).write(0);
}}
"#));
        let second = build(&dir.0, "turn_second", &format!(r#"{header}
#[no_mangle]
pub unsafe extern "C" fn jet_session_turn(cells: *mut u64) {{
    let place = jet_rt_session_resolve(cells.read(), 1, 1, 2, 3, 4, 8, 8);
    cells.add(1).write(if place.is_null() {{ 999 }} else {{ place.cast::<u64>().read() }});
}}
"#));
        let mismatched_header = header.replace(&format!("{identity:?}"), "[0u8; 32]");
        let mismatch = build(&dir.0, "turn_mismatch", &format!(r#"{mismatched_header}
#[no_mangle]
pub unsafe extern "C" fn jet_session_turn(cells: *mut u64) {{
    cells.add(1).write(jet_rt_session_create());
}}
"#));
        // SAFETY: these fixtures were just compiled against the canonical slot
        // runtime with explicit C signatures and no alternate Prelude copy.
        let mut host = unsafe { SessionHost::open(&runtime, identity) }.unwrap();
        assert!(unsafe { host.install(&mismatch) }.unwrap_err().to_string().contains("different build identities"));
        let first_id = unsafe { host.install(&first) }.unwrap();
        let second_id = unsafe { host.install(&second) }.unwrap();
        assert!(host.run(usize::MAX).is_err());
        assert_eq!(host.run(first_id).unwrap(), 0);
        for _ in 0..3 { assert_eq!(host.run(second_id).unwrap(), 41); }
        assert!(!drop_file.exists());
        host.reset().unwrap();
        assert_eq!(fs::read_to_string(&drop_file).unwrap(), "original turn glue ran");
        let second_id = unsafe { host.install(&second) }.unwrap();
        assert_eq!(host.run(second_id).unwrap(), 999);
        drop(host);
    }
}
