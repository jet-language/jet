// D-LIB-CALLGRANT1=A: one load-site adapter for pinned `.jetlib` artifacts.
// The caller grants read roots; identity and declared effects are checked
// before the native payload is copied to a file or mapped by the OS.

#[derive(Clone, Debug)]
pub struct JetModGrant {
    pub read: Vec<String>,
}

pub struct JetMod {
    handle: usize,
    on_tick: usize,
    payload_path: String,
}

const JETLIB_MAGIC: &[u8] = b"jet-jetlib-v1\0";

fn jetlib_take<'a>(bytes: &'a [u8], what: &str) -> Result<(&'a [u8], &'a [u8]), String> {
    if bytes.len() < 4 {
        return Err(format!("truncated .jetlib header ({what} length)"));
    }
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let rest = &bytes[4..];
    if rest.len() < len {
        return Err(format!("truncated .jetlib header ({what})"));
    }
    Ok((&rest[..len], &rest[len..]))
}

fn jetlib_header(bytes: &[u8]) -> Result<(&str, Vec<String>, &[u8]), String> {
    let bytes = bytes
        .strip_prefix(JETLIB_MAGIC)
        .ok_or_else(|| "not a .jetlib artifact (bad magic)".to_string())?;
    let (version, mut rest) = jetlib_take(bytes, "compiler version")?;
    let version = std::str::from_utf8(version)
        .map_err(|_| "compiler version is not UTF-8".to_string())?;
    if rest.len() < 4 {
        return Err("truncated .jetlib header (effect count)".to_string());
    }
    let count = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
    rest = &rest[4..];
    let mut effects = Vec::with_capacity(count);
    for _ in 0..count {
        let (effect, next) = jetlib_take(rest, "effect")?;
        effects.push(
            std::str::from_utf8(effect)
                .map_err(|_| "effect name is not UTF-8".to_string())?
                .to_string(),
        );
        rest = next;
    }
    Ok((version, effects, rest))
}

fn granted_path(path: &std::path::Path, grant: &JetModGrant) -> Result<std::path::PathBuf, String> {
    if grant.read.is_empty() {
        return Err("Mod.load requires a non-empty `read` grant".to_string());
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve library path: {error}"))?;
    for root in &grant.read {
        let Ok(root) = std::fs::canonicalize(root) else { continue };
        if canonical == root || canonical.starts_with(&root) {
            return Ok(canonical);
        }
    }
    Err(format!("library path `{}` is outside the granted read roots", path.display()))
}

fn check_before_map(
    version: &str,
    effects: &[String],
    name: &str,
) -> Result<(), String> {
    // This comparison is deliberately before effect claims are trusted.
    if version != __JET_COMPILER_VERSION {
        return Err(format!(
            "E1338: library `{name}` was built by Jet `{version}`, but the loader uses Jet `{__JET_COMPILER_VERSION}`"
        ));
    }
    for effect in effects {
        // D-LIB-CALLGRANT1=A grants filesystem access by roots. There is no
        // ambient grant for another effect in this first loader surface.
        if effect != "FS" && !effect.starts_with("FS.") {
            return Err(format!(
                "E1339: library `{name}` declares `{effect}`, which this load site does not grant"
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
mod native {
    use super::{check_before_map, granted_path, jetlib_header, JetMod, JetModGrant};
    use std::ffi::{c_char, c_void, CString};
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(target_os = "linux")]
    #[link(name = "dl")]
    extern "C" {
        fn dlopen(path: *const c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn dlerror() -> *const c_char;
    }

    const RTLD_NOW: i32 = 2;
    const RTLD_LOCAL: i32 = 0;
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn last_error() -> String {
        // SAFETY: `dlerror` returns a process-owned NUL-terminated message or
        // null. We copy it immediately and never retain the pointer.
        unsafe {
            let ptr = dlerror();
            if ptr.is_null() {
                return "native loader failed".to_string();
            }
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }

    pub(super) fn load(path: &str, grant: &JetModGrant) -> Result<JetMod, String> {
        let source = granted_path(std::path::Path::new(path), grant)?;
        let bytes = std::fs::read(&source)
            .map_err(|error| format!("cannot read library `{}`: {error}", source.display()))?;
        let (version, effects, payload) = jetlib_header(&bytes)?;
        check_before_map(&version, &effects, &source.display().to_string())?;

        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = std::env::temp_dir().join(format!(
            "jet-mod-{}-{serial}.so",
            std::process::id()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("cannot stage library payload: {error}"))?;
        file.write_all(payload)
            .map_err(|error| format!("cannot stage library payload: {error}"))?;
        file.flush()
            .map_err(|error| format!("cannot flush library payload: {error}"))?;
        drop(file);

        let path_c = CString::new(temp.as_os_str().as_encoded_bytes())
            .map_err(|_| "library path contains NUL".to_string())?;
        let handle = unsafe { dlopen(path_c.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if handle.is_null() {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("cannot map library payload: {}", last_error()));
        }
        let symbol = CString::new("on_tick").expect("static symbol has no NUL");
        let on_tick = unsafe { dlsym(handle, symbol.as_ptr()) };
        if on_tick.is_null() {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("library has no `on_tick` export: {}", last_error()));
        }
        Ok(JetMod {
            handle: handle as usize,
            on_tick: on_tick as usize,
            payload_path: temp.to_string_lossy().into_owned(),
        })
    }

    pub(super) fn on_tick(mod_: &JetMod, dt: i64) -> Result<i64, String> {
        // SAFETY: the address was returned by `dlsym` for the fixed C ABI
        // symbol after identity/effect checks and remains live in `handle`.
        let function: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(mod_.on_tick) };
        let _keep_alive = (mod_.handle, &mod_.payload_path);
        Ok(function(dt))
    }
}

#[cfg(not(unix))]
mod native {
    use super::{JetMod, JetModGrant};

    pub(super) fn load(_path: &str, _grant: &JetModGrant) -> Result<JetMod, String> {
        Err("Mod.load is not supported on this target".to_string())
    }

    pub(super) fn on_tick(_mod: &JetMod, _dt: i64) -> Result<i64, String> {
        Err("Mod.on_tick is not supported on this target".to_string())
    }
}

pub fn jet_mod_load(path: &String, grant: &JetModGrant) -> Result<JetMod, String> {
    native::load(path, grant)
}

pub fn jet_mod_on_tick(mod_: &JetMod, dt: i64) -> Result<i64, String> {
    native::on_tick(mod_, dt)
}
